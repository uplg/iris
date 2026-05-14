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
ENABLED_DECODERS=(
    dca       # DTS Coherent Acoustics (core + HD MA core layer)
    truehd    # Dolby TrueHD (often paired with AC3 on Blu-rays)
    mlp       # Meridian Lossless Packing
    flac      # backup — platform decoder occasionally chokes on edge cases
    alac      # Apple Lossless (rare but cheap to include)
)

# ABIs Iris targets. Drop x86_64 if you only run on real TVs (Apple
# Silicon emulators use arm64 these days).
ABIS=(armeabi-v7a arm64-v8a x86_64)

# Toolchain. Reads ANDROID_NDK_HOME or NDK_HOME; falls back to the
# Android Studio default install path on macOS.
NDK_PATH="${ANDROID_NDK_HOME:-${NDK_HOME:-${HOME}/Library/Android/sdk/ndk/27.0.12077973}}"
if [[ ! -d "${NDK_PATH}" ]]; then
    echo "Android NDK not found at ${NDK_PATH}." >&2
    echo "Set ANDROID_NDK_HOME or install via Android Studio → SDK Manager → NDK." >&2
    exit 1
fi

HOST_PLATFORM="$(uname -s | tr '[:upper:]' '[:lower:]')-$(uname -m | sed 's/x86_64/x86_64/;s/arm64/aarch64/')"

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

# The build_ffmpeg.sh shipped with Media3 takes positional args:
#   <ffmpeg-src> <ndk-path> <host-platform> <enabled-decoders…>
bash ./build_ffmpeg.sh \
    "${EXT_DIR}/jni/ffmpeg" \
    "${NDK_PATH}" \
    "${HOST_PLATFORM}" \
    "${ENABLED_DECODERS[@]}"

# ---------------------------------------------------------------------
# 3. Assemble the AAR via the Media3 Gradle build.
# ---------------------------------------------------------------------
echo "→ Assembling AAR via Gradle…"
cd "${WORK_DIR}/media"
./gradlew :lib-decoder-ffmpeg:assembleRelease

AAR_SRC="${WORK_DIR}/media/libraries/decoder_ffmpeg/build/outputs/aar"
AAR_FILE="$(find "${AAR_SRC}" -name '*-release.aar' | head -n1)"
if [[ -z "${AAR_FILE}" ]]; then
    echo "could not locate built AAR under ${AAR_SRC}" >&2
    exit 1
fi

# ---------------------------------------------------------------------
# 4. Drop it next to the app — Gradle's fileTree picks it up.
# ---------------------------------------------------------------------
DEST_DIR="${APP_DIR}/app/libs"
mkdir -p "${DEST_DIR}"
# Clear prior AARs so we never link two copies of the same shared lib.
rm -f "${DEST_DIR}"/media3-decoder-ffmpeg-*.aar
cp "${AAR_FILE}" "${DEST_DIR}/"

echo
echo "✓ Done. Installed: ${DEST_DIR}/$(basename "${AAR_FILE}")"
echo "  Re-build the app and the FFmpeg renderer will be picked up"
echo "  automatically by PlayerFactory.kt (EXTENSION_RENDERER_MODE_ON)."
