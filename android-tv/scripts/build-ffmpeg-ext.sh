#!/usr/bin/env bash
# Build the Media3 FFmpeg decoder extension AAR and drop it into
# `app/libs/` so the Android TV app picks it up at compile time. The
# extension provides software decoders for DTS / DTS-HD MA / TrueHD /
# MLP and a few other audio codecs whose hardware support is patchy on
# Android TV — notably absent on the AVD emulator and budget boxes.
#
# This is the ONLY official Google path: the AAR is not on Maven
# Central. Run it once per dev machine + per Media3 bump.
#
# Prerequisites (install via Homebrew on macOS):
#   - Android NDK ≥ 27 (https://developer.android.com/ndk)
#   - yasm, nasm, automake, libtool, git
#   - bash 5.x (the script uses array features)
#
# Usage:
#   ./scripts/build-ffmpeg-ext.sh
#
# Outputs:
#   app/libs/media3-decoder-ffmpeg-<version>-release.aar
#
# Re-run after every Media3 version bump in libs.versions.toml so the
# AAR is rebuilt against the matching JNI bindings.

set -euo pipefail

# ---------------------------------------------------------------------
# Config — adjust if you keep checkouts elsewhere.
# ---------------------------------------------------------------------
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
APP_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
WORK_DIR="${WORK_DIR:-${APP_DIR}/.ffmpeg-ext-build}"

# Pin Media3 to whatever the app uses. Parsed out of the version
# catalog so the AAR matches `media3-exoplayer` at runtime — version
# mismatch = ClassNotFound / NoSuchMethod surprises in production.
MEDIA3_VERSION="$(grep -E '^media3 = ' "${APP_DIR}/gradle/libs.versions.toml" \
    | sed -E 's/.*"([^"]+)".*/\1/')"
if [[ -z "${MEDIA3_VERSION}" ]]; then
    echo "could not parse media3 version from libs.versions.toml" >&2
    exit 1
fi

# FFmpeg release. 7.1 is the latest stable line as of writing; bumps
# require re-validating the codec list against the Media3 JNI wrapper
# (some symbol names move between major versions).
FFMPEG_VERSION="${FFMPEG_VERSION:-release/7.1}"

# Codecs enabled in the build. Add anything Android can't decode
# natively; trim aggressively to keep the resulting .so files small.
# `dca` is FFmpeg's DTS decoder; `truehd` covers Atmos in TrueHD;
# `mlp` covers Meridian Lossless Packing carriers.
#
# `ac3`/`eac3` look "native" but are NOT safe to omit: since the TV
# client plays the raw `/stream` directly (no server-side transcode),
# a plain DDP2.0 (E-AC-3) reaches ExoPlayer untouched. Decoder-poor
# devices (Chromecast / Google TV builds that rely on HDMI Dolby
# passthrough and ship no usable E-AC-3 MediaCodec) then have NO
# platform renderer for it — and with these absent from the .so the
# FFmpeg fallback can't cover it either, so the audio track is dropped
# entirely ("Audio: none"). Including them is free on hardware that
# already decodes AC3/EAC3: EXTENSION_RENDERER_MODE_ON keeps the
# platform decoder ahead of the FFmpeg one wherever it exists.
# Belt-and-suspenders: cover the full practical audio universe of
# scene / WEB / remux releases. Audio decoders are tiny (KB-range each),
# so a comprehensive list barely moves the .so size while guaranteeing
# the FFmpeg fallback can decode ANY audio the platform refuses on a
# decoder-poor device. Platform decoders still win where present
# (EXTENSION_RENDERER_MODE_ON), so this is free on capable hardware.
ENABLED_DECODERS=(
    # --- Dolby / DTS family (HW support patchy off-Shield) ---
    dca       # DTS Coherent Acoustics (core + HD MA core layer)
    truehd    # Dolby TrueHD (Atmos carrier; often paired with AC3)
    mlp       # Meridian Lossless Packing
    ac3       # Dolby Digital — fallback for passthrough-only TV devices
    eac3      # Dolby Digital Plus (DDP) — the DDP2.0 "Audio: none" fix
    # --- MPEG / AAC family (universal, cheap safety net) ---
    aac       # AAC-LC / HE-AAC (MP4/MKV WEB-DL default)
    aac_latm  # AAC in MPEG-TS (LATM/LOAS framing)
    mp1       # MPEG-1 Audio Layer I
    mp2       # MPEG-1 Audio Layer II (common in TS / EU broadcast rips)
    mp3       # MPEG-1 Audio Layer III
    # --- Lossless / open ---
    flac      # platform decoder occasionally chokes on edge cases
    alac      # Apple Lossless
    opus      # Opus (WEBM / Matroska)
    vorbis    # Vorbis (WEBM / Matroska)
    # --- LPCM (Blu-ray / DVD remuxes) ---
    pcm_s16le pcm_s16be pcm_s24le pcm_s32le
    pcm_dvd   # DVD LPCM
    pcm_bluray # Blu-ray LPCM
    # --- Legacy AVI/WMV containers still floating around ---
    wmav2     # Windows Media Audio v2
    wmapro    # Windows Media Audio Pro
)

# Minimum Android API level the produced .so files target. Drives the
# compiler name (`armv7a-linux-androideabi${LEVEL}-clang`). Must be
# ≥ the `minSdk` declared in `app/build.gradle.kts` (currently 23).
ANDROID_API_LEVEL="${ANDROID_API_LEVEL:-23}"

# Host toolchain dir under `${NDK}/toolchains/llvm/prebuilt/`. Picked
# from what the NDK actually ships rather than hardcoded — the layout
# differs between major versions:
#   * NDK r25 / r26 on macOS → only `darwin-x86_64` (Apple Silicon
#     runs it through Rosetta).
#   * NDK r27+ on macOS → `darwin-aarch64` (native ARM) AND, on some
#     installs, `darwin-x86_64` for Intel hosts.
# Resolved below once `NDK_PATH` is known.
HOST_PLATFORM=""

# Toolchain. Prefer an explicit env var; otherwise scan the Android
# Studio NDK install directory and pick a build-friendly version.
#
# Why r26 specifically (and not whatever's newest): Media3's
# `build_ffmpeg.sh` invokes the legacy NDK toolchain via direct
# `gcc`-compatible wrappers (`armv7a-linux-androideabi*-clang` etc.)
# that NDK r27 removed in favour of unified `clang --target=...`
# invocations. The build fails with cryptic "compiler not found"
# errors on r27. r26 still ships both layouts. r25 also works as a
# fallback. Anything older lacks the `aarch64-linux-android21+`
# sysroot the script asks for.
SDK_NDK_DIR="${ANDROID_SDK_ROOT:-${ANDROID_HOME:-${HOME}/Library/Android/sdk}}/ndk"
NDK_PATH="${ANDROID_NDK_HOME:-${NDK_HOME:-}}"
if [[ -z "${NDK_PATH}" && -d "${SDK_NDK_DIR}" ]]; then
    # Try the build-friendly ranges in priority order. `sort -V` picks
    # the highest patch within each major; we walk majors low-to-high
    # only if the preferred one isn't installed.
    for major_pattern in '^26\.' '^25\.' '^24\.' '^23\.'; do
        candidate="$(ls -1 "${SDK_NDK_DIR}" 2>/dev/null \
            | grep -E "${major_pattern}" \
            | sort -V \
            | tail -n1)"
        if [[ -n "${candidate}" && -d "${SDK_NDK_DIR}/${candidate}" ]]; then
            NDK_PATH="${SDK_NDK_DIR}/${candidate}"
            break
        fi
    done
fi
if [[ -z "${NDK_PATH}" || ! -d "${NDK_PATH}" ]]; then
    echo "Compatible Android NDK not found (r23–r26 expected)." >&2
    echo "  Searched: ${SDK_NDK_DIR}" >&2
    echo "  Available: $(ls "${SDK_NDK_DIR}" 2>/dev/null | tr '\n' ' ')" >&2
    echo "  Install NDK r26 via Android Studio → SDK Manager → SDK Tools → NDK (Side by side)," >&2
    echo "  or set ANDROID_NDK_HOME=/path/to/ndk-r26 explicitly." >&2
    exit 1
fi
echo "→ Using NDK at ${NDK_PATH}"

# Resolve the host toolchain dir against what's actually on disk. On
# Apple Silicon, prefer the native `darwin-aarch64` build when it
# exists (NDK r27+) since the Rosetta-translated `darwin-x86_64`
# variant is ~2× slower; on Intel macs there's only `darwin-x86_64`.
PREBUILT_DIR="${NDK_PATH}/toolchains/llvm/prebuilt"
HOST_ARCH="$(uname -m)"
HOST_CANDIDATES=()
if [[ "${HOST_ARCH}" == "arm64" || "${HOST_ARCH}" == "aarch64" ]]; then
    HOST_CANDIDATES=(darwin-aarch64 darwin-x86_64)
else
    HOST_CANDIDATES=(darwin-x86_64 darwin-aarch64)
fi
for candidate in "${HOST_CANDIDATES[@]}"; do
    if [[ -d "${PREBUILT_DIR}/${candidate}" ]]; then
        HOST_PLATFORM="${candidate}"
        break
    fi
done
if [[ -z "${HOST_PLATFORM}" ]]; then
    echo "No usable toolchain under ${PREBUILT_DIR}." >&2
    echo "  Found: $(ls "${PREBUILT_DIR}" 2>/dev/null | tr '\n' ' ')" >&2
    exit 1
fi
echo "→ Using host toolchain ${HOST_PLATFORM}"

# ---------------------------------------------------------------------
# 1. Fetch sources (Media3 + FFmpeg).
# ---------------------------------------------------------------------
mkdir -p "${WORK_DIR}"
cd "${WORK_DIR}"

if [[ ! -d media ]]; then
    echo "→ Cloning Media3 source @ ${MEDIA3_VERSION}…"
    git clone --depth 1 --branch "${MEDIA3_VERSION}" \
        https://github.com/androidx/media.git media
fi

EXT_DIR="${WORK_DIR}/media/libraries/decoder_ffmpeg/src/main"
JNI_DIR="${EXT_DIR}/jni"
mkdir -p "${JNI_DIR}"

if [[ ! -d "${JNI_DIR}/ffmpeg" ]]; then
    echo "→ Cloning FFmpeg @ ${FFMPEG_VERSION}…"
    git -C "${JNI_DIR}" clone --depth 1 --branch "${FFMPEG_VERSION}" \
        https://git.ffmpeg.org/ffmpeg.git ffmpeg
fi

# ---------------------------------------------------------------------
# 2. Build FFmpeg native libs for each ABI.
# ---------------------------------------------------------------------
echo "→ Building FFmpeg native libraries…"
cd "${EXT_DIR}/jni"

# Positional args expected by Media3's `build_ffmpeg.sh`:
#   1) FFMPEG_MODULE_PATH — the `decoder_ffmpeg/src/main` dir; the
#      script descends into `${1}/jni/ffmpeg` for the configure call.
#   2) NDK_PATH — absolute path to the NDK install.
#   3) HOST_PLATFORM — `darwin-x86_64` / `darwin-aarch64` / `linux-x86_64`.
#      Must match a subdir of `${NDK_PATH}/toolchains/llvm/prebuilt/`.
#   4) ANDROID_ABI — misleadingly named, actually the minimum Android
#      API level (number, e.g. `23`). Drives the compiler binary name:
#      `armv7a-linux-androideabi${LEVEL}-clang`.
#   5..N) ENABLED_DECODERS — codec names passed to `--enable-decoder=`.
bash ./build_ffmpeg.sh \
    "${EXT_DIR}" \
    "${NDK_PATH}" \
    "${HOST_PLATFORM}" \
    "${ANDROID_API_LEVEL}" \
    "${ENABLED_DECODERS[@]}"

# ---------------------------------------------------------------------
# 3. Assemble the AAR via the Media3 Gradle build.
# ---------------------------------------------------------------------
echo "→ Assembling AAR via Gradle…"
cd "${WORK_DIR}/media"
./gradlew :lib-decoder-ffmpeg:assembleRelease

# Media3 uses an out-of-tree build directory (`buildout/`) instead of
# the per-module `build/` Gradle default — set up via their root
# `build.gradle.kts`. The AAR lands there as `lib-decoder-ffmpeg-release.aar`
# (no version suffix — Media3 doesn't stamp the version into the
# file name).
AAR_SRC="${WORK_DIR}/media/libraries/decoder_ffmpeg/buildout/outputs/aar"
AAR_FILE="$(find "${AAR_SRC}" -name 'lib-decoder-ffmpeg-*-release.aar' -o -name 'lib-decoder-ffmpeg-release.aar' 2>/dev/null | head -n1)"
if [[ -z "${AAR_FILE}" ]]; then
    echo "could not locate built AAR under ${AAR_SRC}" >&2
    echo "  Found: $(ls "${AAR_SRC}" 2>/dev/null | tr '\n' ' ')" >&2
    exit 1
fi

# ---------------------------------------------------------------------
# 4. Drop it next to the app — Gradle's fileTree picks it up.
# ---------------------------------------------------------------------
DEST_DIR="${APP_DIR}/app/libs"
mkdir -p "${DEST_DIR}"
# Clear prior AARs so we never link two copies of the same shared lib.
# Stamp the Media3 version into the destination filename ourselves so
# the user can spot a stale AAR after a Media3 bump (the upstream AAR
# isn't versioned in its file name, frustratingly).
DEST_AAR="${DEST_DIR}/lib-decoder-ffmpeg-${MEDIA3_VERSION}-release.aar"
rm -f "${DEST_DIR}"/lib-decoder-ffmpeg-*.aar
cp "${AAR_FILE}" "${DEST_AAR}"

echo
echo "✓ Done. Installed: ${DEST_AAR}"
echo "  Re-build the app and the FFmpeg renderer will be picked up"
echo "  automatically by PlayerFactory.kt (EXTENSION_RENDERER_MODE_ON)."
