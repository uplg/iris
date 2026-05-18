#!/usr/bin/env bash
# Build the Media3 AV1 (libgav1) decoder extension AAR and drop it into
# `app/libs/` so the Android TV app picks it up at compile time. It
# provides a software AV1 video decoder for devices without hardware
# AV1 — increasingly common since AV1 is now the default video codec on
# a lot of new WEB-DL releases, and budget Android TV boxes / older
# Chromecasts have no AV1 silicon.
#
# Like the FFmpeg extension this is the ONLY official Google path: the
# AAR is NOT on Maven (Google does not publish the native Media3
# decoder extensions prebuilt). Run once per dev machine + per Media3
# bump. It coexists with `lib-decoder-ffmpeg-*.aar` in `app/libs/` —
# each build script only cleans its own AAR, and Gradle's `fileTree`
# links every `*.aar`. No PlayerFactory change is needed:
# DefaultRenderersFactory(EXTENSION_RENDERER_MODE_ON) reflectively
# instantiates `Libgav1VideoRenderer`, exactly like the FFmpeg audio
# renderer; the platform AV1 decoder still takes precedence wherever it
# exists (zero cost on Shield / recent Google TV).
#
# Unlike FFmpeg, there is no per-ABI cross-compile loop here: the
# libgav1 native build is driven by the Media3 module's CMake
# `externalNativeBuild` during `:lib-decoder-av1:assembleRelease`, so
# AGP/CMake handles every ABI itself.
#
# Prerequisites (install via Homebrew on macOS):
#   - Android NDK ≥ 26 (https://developer.android.com/ndk)
#   - Android SDK CMake (SDK Manager → SDK Tools → CMake) — AGP's
#     externalNativeBuild needs it; the build fails with a clear
#     "CMake not found" otherwise.
#   - git, bash 5.x (the script uses array features)
#
# Usage:
#   ./scripts/build-av1-ext.sh
#
# Outputs:
#   app/libs/lib-decoder-av1-<media3-version>-release.aar
#
# Re-run after every Media3 version bump in libs.versions.toml so the
# AAR matches the `media3-exoplayer` JNI bindings at runtime (version
# mismatch = ClassNotFound / NoSuchMethod surprises in production).

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

# libgav1 + abseil are cloned at tip-of-tree per Media3's
# `decoder_av1` README. They have no Media3-aligned release tags. If a
# future Media3 bump breaks the native build, pin a known-good commit
# here (override via env) instead of chasing tip:
LIBGAV1_REF="${LIBGAV1_REF:-}"   # empty = default branch
ABSEIL_REF="${ABSEIL_REF:-}"     # empty = default branch
MEDIA3_GIT_TAG="${MEDIA3_GIT_TAG:-${MEDIA3_VERSION}}"

# ---------------------------------------------------------------------
# Toolchain. Prefer an explicit env var; otherwise scan the Android
# Studio NDK install and pick a build-friendly version. The CMake-driven
# AV1 build is happy on r27+ too, but we reuse the FFmpeg script's
# proven resolution (prefers r26, falls back) for consistency — AGP
# just needs a valid NDK, which any of these provide.
# ---------------------------------------------------------------------
SDK_NDK_DIR="${ANDROID_SDK_ROOT:-${ANDROID_HOME:-${HOME}/Library/Android/sdk}}/ndk"
NDK_PATH="${ANDROID_NDK_HOME:-${NDK_HOME:-}}"
if [[ -z "${NDK_PATH}" && -d "${SDK_NDK_DIR}" ]]; then
    for major_pattern in '^27\.' '^26\.' '^25\.' '^24\.' '^23\.'; do
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
    echo "Compatible Android NDK not found (r23–r27 expected)." >&2
    echo "  Searched: ${SDK_NDK_DIR}" >&2
    echo "  Available: $(ls "${SDK_NDK_DIR}" 2>/dev/null | tr '\n' ' ')" >&2
    echo "  Install via Android Studio → SDK Manager → SDK Tools → NDK," >&2
    echo "  or set ANDROID_NDK_HOME=/path/to/ndk explicitly." >&2
    exit 1
fi
echo "→ Using NDK at ${NDK_PATH}"
# AGP resolves the NDK from this env var (and the SDK-managed CMake on
# its own). Export so the Gradle assemble below uses the same NDK we
# just validated rather than whatever ndk.dir guesses.
export ANDROID_NDK_HOME="${NDK_PATH}"

# ---------------------------------------------------------------------
# 1. Fetch sources (Media3 + libgav1 + abseil).
# ---------------------------------------------------------------------
mkdir -p "${WORK_DIR}"
cd "${WORK_DIR}"

if [[ ! -d media ]]; then
    echo "→ Cloning Media3 source @ ${MEDIA3_GIT_TAG}…"
    git clone --depth 1 --branch "${MEDIA3_GIT_TAG}" \
        https://github.com/androidx/media.git media
fi

JNI_DIR="${WORK_DIR}/media/libraries/decoder_av1/src/main/jni"
mkdir -p "${JNI_DIR}"

if [[ ! -d "${JNI_DIR}/libgav1" ]]; then
    echo "→ Cloning libgav1…"
    git -C "${JNI_DIR}" clone \
        https://chromium.googlesource.com/codecs/libgav1
    if [[ -n "${LIBGAV1_REF}" ]]; then
        git -C "${JNI_DIR}/libgav1" checkout "${LIBGAV1_REF}"
    fi
fi
if [[ ! -d "${JNI_DIR}/libgav1/third_party/abseil-cpp" ]]; then
    echo "→ Cloning abseil-cpp (libgav1 third_party dep)…"
    git -C "${JNI_DIR}/libgav1" clone \
        https://chromium.googlesource.com/external/github.com/abseil/abseil-cpp.git \
        third_party/abseil-cpp
    if [[ -n "${ABSEIL_REF}" ]]; then
        git -C "${JNI_DIR}/libgav1/third_party/abseil-cpp" checkout "${ABSEIL_REF}"
    fi
fi

# ---------------------------------------------------------------------
# 2. Assemble the AAR via the Media3 Gradle build (CMake builds the
#    libgav1 native libs for every ABI as part of this).
# ---------------------------------------------------------------------
echo "→ Assembling AAR via Gradle (this compiles libgav1 — slow first run)…"
cd "${WORK_DIR}/media"
./gradlew :lib-decoder-av1:assembleRelease

# Media3 uses an out-of-tree build dir (`buildout/`) rather than the
# per-module `build/` default. The AAR lands there unversioned.
AAR_SRC="${WORK_DIR}/media/libraries/decoder_av1/buildout/outputs/aar"
AAR_FILE="$(find "${AAR_SRC}" -name 'lib-decoder-av1-*-release.aar' -o -name 'lib-decoder-av1-release.aar' 2>/dev/null | head -n1)"
if [[ -z "${AAR_FILE}" ]]; then
    echo "could not locate built AAR under ${AAR_SRC}" >&2
    echo "  Found: $(ls "${AAR_SRC}" 2>/dev/null | tr '\n' ' ')" >&2
    exit 1
fi

# ---------------------------------------------------------------------
# 3. Drop it next to the app — Gradle's fileTree picks it up. Only
#    clear prior AV1 AARs so the FFmpeg extension AAR is left intact.
# ---------------------------------------------------------------------
DEST_DIR="${APP_DIR}/app/libs"
mkdir -p "${DEST_DIR}"
DEST_AAR="${DEST_DIR}/lib-decoder-av1-${MEDIA3_VERSION}-release.aar"
rm -f "${DEST_DIR}"/lib-decoder-av1-*.aar
cp "${AAR_FILE}" "${DEST_AAR}"

echo
echo "✓ Done. Installed: ${DEST_AAR}"
echo "  Re-build the app and the libgav1 AV1 renderer is picked up"
echo "  automatically by PlayerFactory.kt (EXTENSION_RENDERER_MODE_ON)."
echo "  Coexists with lib-decoder-ffmpeg-*.aar — both stay in app/libs/."
