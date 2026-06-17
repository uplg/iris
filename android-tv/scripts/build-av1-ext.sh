#!/usr/bin/env bash
# Build the Media3 AV1 (dav1d) decoder extension AAR and drop it into
# `app/libs/` so the Android TV app picks it up at compile time. It
# provides a software AV1 video decoder for streams the device's
# hardware decoder can't handle — most notably **10-bit AV1**: many
# Android TV boxes decode 8-bit AV1 (and 10-bit HEVC) in hardware but
# have NO 10-bit AV1 path, so a 10-bit AV1 file falls to software. The
# platform's built-in software AV1 codec (libgav1-based on older
# firmware) is far too slow for 1080p → stutter. dav1d (VideoLAN) is
# the fast software AV1 decoder browsers and VLC use; bundling it makes
# those files smooth.
#
# Since Media3 1.9.0 the AV1 module was rewritten around **dav1d** (it
# was libgav1 before) and the renderer is now `Libdav1dVideoRenderer`.
# `DefaultRenderersFactory` reflectively instantiates that class, so no
# PlayerFactory wiring is strictly required — but see `IrisRenderersFactory`
# in PlayerFactory.kt: we force `EXTENSION_RENDERER_MODE_ON` for VIDEO so
# the platform/hardware decoder wins for what it supports (8-bit AV1) and
# dav1d only kicks in for the rest (10-bit AV1, boxes with no AV1 silicon).
#
# The AAR is NOT on Maven — Google does not publish the native Media3
# decoder extensions prebuilt; building from source is the only official
# path. Run once per dev machine and re-run after every Media3 bump in
# libs.versions.toml so the AAR matches the `media3-exoplayer` JNI
# bindings (a version mismatch = ClassNotFound / NoSuchMethod at
# runtime). It coexists with `lib-decoder-ffmpeg-*.aar` in `app/libs/`;
# each build script only cleans its own AAR and Gradle's `fileTree`
# links every `*.aar`.
#
# Prerequisites (Homebrew on macOS):
#   - Android NDK r27 (https://developer.android.com/ndk) — tested ref.
#   - Android SDK CMake (SDK Manager → SDK Tools → CMake) for AGP's
#     externalNativeBuild (the JNI wrapper).
#   - meson (>= 0.49), ninja — dav1d's build system.   brew install meson ninja
#   - nasm (>= 2.14) — needed for the x86* dav1d targets. brew install nasm
#   - git, bash 5.x
#
# Usage:
#   ./scripts/build-av1-ext.sh
#
# Outputs:
#   app/libs/lib-decoder-av1-<media3-version>-release.aar  (with native
#   libdav1d packed per-ABI — a classes-only AAR means the dav1d build
#   was skipped and the renderer is inert at runtime).

set -euo pipefail

# ---------------------------------------------------------------------
# Config — adjust if you keep checkouts elsewhere.
# ---------------------------------------------------------------------
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
APP_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
# Share the Media3 checkout with build-ffmpeg-ext.sh (same gitignored
# dir) so we don't clone androidx/media twice.
WORK_DIR="${WORK_DIR:-${APP_DIR}/.ffmpeg-ext-build}"

# Pin Media3 to whatever the app uses. Parsed out of the version
# catalog so the AAR matches `media3-exoplayer` at runtime.
MEDIA3_VERSION="$(grep -E '^media3 = ' "${APP_DIR}/gradle/libs.versions.toml" \
    | sed -E 's/.*"([^"]+)".*/\1/')"
if [[ -z "${MEDIA3_VERSION}" ]]; then
    echo "could not parse media3 version from libs.versions.toml" >&2
    exit 1
fi
MEDIA3_GIT_TAG="${MEDIA3_GIT_TAG:-${MEDIA3_VERSION}}"

# dav1d + cpu_features are cloned at tip per Media3's `decoder_av1`
# README (no Media3-aligned tags). Pin a known-good commit here (env
# override) if a future tip breaks the native build.
DAV1D_REF="${DAV1D_REF:-}"               # empty = default branch
CPU_FEATURES_REF="${CPU_FEATURES_REF:-}" # empty = default branch

# ---------------------------------------------------------------------
# Host build tools required by dav1d (meson/ninja/nasm) — fail early
# with a clear install hint rather than deep inside the meson cross build.
# ---------------------------------------------------------------------
missing=()
for tool in meson ninja nasm; do
    command -v "${tool}" >/dev/null 2>&1 || missing+=("${tool}")
done
if [[ ${#missing[@]} -gt 0 ]]; then
    echo "Missing build tool(s): ${missing[*]}" >&2
    echo "  Install on macOS with: brew install ${missing[*]}" >&2
    exit 1
fi

# ---------------------------------------------------------------------
# Toolchain. Prefer an explicit env var; otherwise scan the Android
# Studio NDK install and pick r27 (the ref the dav1d build is tested
# against), falling back to r26/r25.
# ---------------------------------------------------------------------
SDK_NDK_DIR="${ANDROID_SDK_ROOT:-${ANDROID_HOME:-${HOME}/Library/Android/sdk}}/ndk"
NDK_PATH="${ANDROID_NDK_HOME:-${NDK_HOME:-}}"
if [[ -z "${NDK_PATH}" && -d "${SDK_NDK_DIR}" ]]; then
    for major_pattern in '^27\.' '^26\.' '^25\.'; do
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
    echo "Compatible Android NDK not found (r25-r27 expected)." >&2
    echo "  Searched: ${SDK_NDK_DIR}" >&2
    echo "  Set ANDROID_NDK_HOME=/path/to/ndk explicitly." >&2
    exit 1
fi
echo "-> Using NDK at ${NDK_PATH}"
export ANDROID_NDK_HOME="${NDK_PATH}"

# Host platform = the NDK prebuilt toolchain directory name. The NDK
# only ships an x86_64 prebuilt for macOS (runs natively or via Rosetta
# on Apple Silicon), so prefer whatever actually exists under the NDK.
case "$(uname -s)" in
    Darwin) HOST_CANDIDATES=("darwin-arm64" "darwin-x86_64") ;;
    Linux) HOST_CANDIDATES=("linux-x86_64") ;;
    *)
        echo "Unsupported host $(uname -s) — build dav1d on Linux or macOS." >&2
        exit 1
        ;;
esac
HOST_PLATFORM=""
for h in "${HOST_CANDIDATES[@]}"; do
    if [[ -d "${NDK_PATH}/toolchains/llvm/prebuilt/${h}" ]]; then
        HOST_PLATFORM="${h}"
        break
    fi
done
if [[ -z "${HOST_PLATFORM}" ]]; then
    echo "No NDK prebuilt toolchain found under ${NDK_PATH}/toolchains/llvm/prebuilt/" >&2
    exit 1
fi
echo "-> Host platform ${HOST_PLATFORM}"

# ---------------------------------------------------------------------
# 1. Fetch sources (Media3 + cpu_features + dav1d).
# ---------------------------------------------------------------------
mkdir -p "${WORK_DIR}"
cd "${WORK_DIR}"

if [[ ! -d media ]]; then
    echo "-> Cloning Media3 source @ ${MEDIA3_GIT_TAG}..."
    git clone --depth 1 --branch "${MEDIA3_GIT_TAG}" \
        https://github.com/androidx/media.git media
fi

AV1_MODULE_PATH="${WORK_DIR}/media/libraries/decoder_av1/src/main"
JNI_DIR="${AV1_MODULE_PATH}/jni"
mkdir -p "${JNI_DIR}"

# Drop the stale libgav1 / abseil checkout the pre-1.9 version of this
# script left behind — the dav1d module's CMake never references them
# and a stale tree only confuses a re-run.
rm -rf "${JNI_DIR}/libgav1"

if [[ ! -d "${JNI_DIR}/cpu_features" ]]; then
    echo "-> Cloning cpu_features..."
    git -C "${JNI_DIR}" clone https://github.com/google/cpu_features
    if [[ -n "${CPU_FEATURES_REF}" ]]; then
        git -C "${JNI_DIR}/cpu_features" checkout "${CPU_FEATURES_REF}"
    fi
fi
if [[ ! -d "${JNI_DIR}/dav1d" ]]; then
    echo "-> Cloning dav1d..."
    git -C "${JNI_DIR}" clone https://code.videolan.org/videolan/dav1d.git
    if [[ -n "${DAV1D_REF}" ]]; then
        git -C "${JNI_DIR}/dav1d" checkout "${DAV1D_REF}"
    fi
fi

# Apply the Iris 10-bit-AV1 patch to the JNI wrapper. The stock Media3 dav1d
# wrapper (dav1d_jni.cc) hard-errors on ANY >8-bit frame, so a 10-bit AV1 file
# fails to decode and the TV player bounces to the server HLS remux (which
# `-c:v copy`s the same undecodable 10-bit video → black screen). The patch
# lets 10-bit through the Surface output path and down-converts it to 8-bit
# YV12. Idempotent; aborts loudly if it no longer applies after a Media3 bump.
PATCH_FILE="${SCRIPT_DIR}/dav1d-10bit-surface.patch"
if [[ -f "${PATCH_FILE}" ]]; then
    if git -C "${WORK_DIR}/media" apply --reverse --check "${PATCH_FILE}" 2>/dev/null; then
        echo "-> dav1d 10-bit patch already applied"
    elif git -C "${WORK_DIR}/media" apply --check "${PATCH_FILE}" 2>/dev/null; then
        echo "-> Applying dav1d 10-bit patch"
        git -C "${WORK_DIR}/media" apply "${PATCH_FILE}"
    else
        echo "dav1d-10bit-surface.patch no longer applies to Media3 ${MEDIA3_GIT_TAG}." >&2
        echo "  Re-generate it against the new source before shipping (else 10-bit AV1 breaks)." >&2
        exit 1
    fi
fi

# ---------------------------------------------------------------------
# 2. Cross-build libdav1d.a for every Android ABI (meson + ninja). This
#    populates jni/nativelib/<abi>/libdav1d.a, which the module's
#    CMakeLists links into the JNI wrapper.
# ---------------------------------------------------------------------
echo "-> Building libdav1d for all ABIs (meson/ninja - slow first run)..."
cd "${JNI_DIR}"
chmod +x build_dav1d.sh
# build_dav1d.sh uses `declare -A` (associative arrays = bash >= 4). macOS
# ships bash 3.2 as /bin/bash and the script's shebang is `#!/bin/bash`, so
# invoke it explicitly with a modern bash instead of `./build_dav1d.sh`.
BASH4=""
for cand in "$(command -v bash)" /opt/homebrew/bin/bash /usr/local/bin/bash; do
    if [[ -x "${cand}" ]] && "${cand}" -c 'declare -A _t' >/dev/null 2>&1; then
        BASH4="${cand}"
        break
    fi
done
if [[ -z "${BASH4}" ]]; then
    echo "build_dav1d.sh needs bash >= 4 (declare -A); none found." >&2
    echo "  Install on macOS with: brew install bash" >&2
    exit 1
fi
echo "-> dav1d build via ${BASH4}"
"${BASH4}" build_dav1d.sh "${AV1_MODULE_PATH}" "${NDK_PATH}" "${HOST_PLATFORM}"

if ! find "${JNI_DIR}/nativelib" -name 'libdav1d.a' 2>/dev/null | grep -q .; then
    echo "dav1d build produced no libdav1d.a - aborting before a hollow AAR" >&2
    exit 1
fi

# ---------------------------------------------------------------------
# 3. Assemble the AAR via the Media3 Gradle build (CMake/Ninja compiles
#    dav1d_jni.cc and links libdav1d.a per ABI, packing the .so files).
# ---------------------------------------------------------------------
echo "-> Assembling AAR via Gradle (compiles the dav1d JNI wrapper)..."
cd "${WORK_DIR}/media"
# Force a from-scratch module build. AGP's incremental externalNativeBuild
# can re-bundle a CLASSES-ONLY AAR on a re-run: a freshly rebuilt libdav1d.a
# (or a `:clean`ed module) does NOT re-trigger the CMake/JNI link, so
# libdav1dJNI.so is never re-packed and the renderer ships inert. The CMake
# CONFIGURE + build dir is `<module>/.cxx`, which lives OUTSIDE `buildout`
# and survives `:clean` — that's what makes Gradle think the native build is
# up-to-date (a 2-second "build" with no .so). Wipe BOTH `.cxx` and `buildout`
# (and disable the config cache) so CMake fully reconfigures, rebuilds and the
# .so get packed every time. (The `.so`-presence guard below is the backstop.)
rm -rf "${WORK_DIR}/media/libraries/decoder_av1/.cxx" \
    "${WORK_DIR}/media/libraries/decoder_av1/buildout"
# `--rerun-tasks` is the part that actually matters: even after CMake rebuilds
# the .so, the JNI-libs MERGE + `bundleReleaseAar` tasks stay "up-to-date" and
# ship a classes-only AAR (Gradle even restores the stale, empty JNI output
# from the build cache). Forcing every task action + disabling both caches
# makes the freshly built libdav1dJNI.so get re-merged and packed.
./gradlew --no-configuration-cache --no-build-cache --rerun-tasks \
    :lib-decoder-av1:assembleRelease

# Media3 uses an out-of-tree build dir (`buildout/`); the AAR lands
# there unversioned.
AAR_SRC="${WORK_DIR}/media/libraries/decoder_av1/buildout/outputs/aar"
# Glob rather than `find -o` (compound predicates trip some `find` shims).
AAR_FILE=""
for f in "${AAR_SRC}"/lib-decoder-av1-*-release.aar "${AAR_SRC}"/lib-decoder-av1-release.aar; do
    [[ -f "${f}" ]] && AAR_FILE="${f}" && break
done
if [[ -z "${AAR_FILE}" ]]; then
    echo "could not locate built AAR under ${AAR_SRC}" >&2
    echo "  Found: $(ls "${AAR_SRC}" 2>/dev/null | tr '\n' ' ')" >&2
    exit 1
fi

# Guard against the exact bug this rewrite fixes: a classes-only AAR
# (no native libdav1d) means the renderer is inert at runtime. Use `grep -c`
# (consumes all input) instead of `grep -q` (short-circuits on first match):
# under `set -o pipefail`, grep -q's early exit SIGPIPEs the upstream `unzip`,
# so the pipeline returns 141 and trips this guard even when a .so IS present.
so_count="$(unzip -l "${AAR_FILE}" 2>/dev/null | grep -c '\.so' || true)"
if [[ "${so_count}" -eq 0 ]]; then
    echo "Built AAR contains no native .so - the dav1d JNI did not link." >&2
    echo "  AAR: ${AAR_FILE}" >&2
    exit 1
fi

# ---------------------------------------------------------------------
# 4. Drop it next to the app — Gradle's fileTree picks it up. Only
#    clear prior AV1 AARs so the FFmpeg extension AAR is left intact.
# ---------------------------------------------------------------------
DEST_DIR="${APP_DIR}/app/libs"
mkdir -p "${DEST_DIR}"
DEST_AAR="${DEST_DIR}/lib-decoder-av1-${MEDIA3_VERSION}-release.aar"
rm -f "${DEST_DIR}"/lib-decoder-av1-*.aar
cp "${AAR_FILE}" "${DEST_AAR}"

echo
echo "Done. Installed: ${DEST_AAR}"
echo "  ($(unzip -l "${DEST_AAR}" | grep -c '\.so') native .so entries - non-zero = dav1d packed.)"
echo "  Re-build the app; PlayerFactory's IrisRenderersFactory uses dav1d"
echo "  (EXTENSION_RENDERER_MODE_ON for video) for AV1 the hardware can't decode."
echo "  Coexists with lib-decoder-ffmpeg-*.aar - both stay in app/libs/."
