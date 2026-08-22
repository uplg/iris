#!/usr/bin/env bash
# Regenerates the three fragmented MP4s next to this script. Requires ffmpeg
# with libx265.
#
# Two open-GOP HEVC Main 10 sources, identical but for the B-frame count, are
# cut into 4 s fragmented MP4s:
#
#   A-idr-start.mp4     t=0, starts on the stream's only IDR      (control)
#   B-cra-clean.mp4     t=6, starts on a CRA, no leading pictures
#   C-cra-with-rasl.mp4 t=6, starts on a CRA, RASL pictures kept
#
# B is the isolated case: a CRA that is a complete, self-contained random
# access point. Nothing in the fragment references anything outside it, and
# every other engine decodes it. C carries the RASL pictures that ffmpeg's
# stream-copy seek leaves in; per the spec those are non-decodable when the CRA
# begins the bitstream (NoRaslOutputFlag = 1) and must be discarded.
set -euo pipefail
cd "$(dirname "$0")"

encode() { # $1 = output, $2 = bframes
  ffmpeg -v error -y -f lavfi -i testsrc=duration=12:size=640x360:rate=25 \
    -c:v libx265 -pix_fmt yuv420p10le \
    -x265-params "keyint=50:min-keyint=50:open-gop=1:bframes=$2:info=0:log-level=none" \
    -tag:v hvc1 -an "$1"
}

frag() { # $1 = output, $2 = source, rest = extra input flags
  ffmpeg -v error -y "${@:3}" -i "$2" -t 4 -c:v copy -tag:v hvc1 \
    -movflags "frag_keyframe+empty_moov+default_base_moof" \
    -frag_duration 2000000 "$1"
}

encode src-noleading.mp4 0
encode src-leading.mp4 4

frag A-idr-start.mp4     src-noleading.mp4
frag B-cra-clean.mp4     src-noleading.mp4 -ss 6
frag C-cra-with-rasl.mp4 src-leading.mp4   -ss 6

rm -f src-noleading.mp4 src-leading.mp4
ls -l A-idr-start.mp4 B-cra-clean.mp4 C-cra-with-rasl.mp4
