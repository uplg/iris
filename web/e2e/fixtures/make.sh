#!/usr/bin/env bash
# Generates the playback bench's media into web/e2e/.media (gitignored). Every clip is synthetic:
# ffmpeg's testsrc pattern (a seconds counter and a gradient that moves every frame) and a beep
# at the start of every second, so a frozen picture or a silent track shows. Idempotent: a clip
# already there is kept. Usage: e2e/fixtures/make.sh [--force]
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
out="$here/../.media"
mkdir -p "$out"
force="${1:-}"
FFMPEG="${FFMPEG:-ffmpeg}"

# a 880 Hz beep for the first 150 ms of every second
beep="if(lt(mod(t\,1)\,0.15)\,0.5*sin(2*PI*880*t)\,0)"
# a quiet bed under it, so a channel that is wired but silent can be told from a missing one
bed="0.05*sin(2*PI*220*t)"

make() {
	local name="$1"
	shift
	if [[ -f "$out/$name" && "$force" != "--force" ]]; then
		return
	fi
	echo "fixture: $name"
	"$FFMPEG" -hide_banner -loglevel error -y "$@" "$out/$name.tmp.${name##*.}"
	mv "$out/$name.tmp.${name##*.}" "$out/$name"
}

video() {
	local size="$1" dur="$2"
	echo -f lavfi -i "testsrc=size=${size}:rate=24:duration=${dur}"
}

x264=(-c:v libx264 -preset veryfast -profile:v high -pix_fmt yuv420p -g 48 -keyint_min 48 -sc_threshold 0)
stereo48() { echo -f lavfi -i "aevalsrc=exprs=${beep}+${bed}|${beep}+${bed}:s=48000:c=stereo:d=$1"; }

# Scenario 1, 2, 6, 9: plain H.264 + AAC stereo 48 kHz. Its audio carries no language tag (the
# other clips say eng): the server's HLS remux must cope with an untagged track too.
# shellcheck disable=SC2046
make Bench.Alpha.2001.360p.WEB.H264.AAC-IRIS.mp4 $(video 640x360 60) $(stereo48 60) \
	"${x264[@]}" -c:a aac -b:a 128k -ar 48000 -movflags +faststart

# Scenario 2: the same in Matroska (Chrome plays it on Tier B).
# shellcheck disable=SC2046
make Bench.Bravo.2002.360p.WEB.H264.AAC-IRIS.mkv $(video 640x360 90) $(stereo48 90) \
	"${x264[@]}" -metadata:s:a:0 language=eng -c:a aac -b:a 128k -ar 48000

# Scenario 3, 4: HEVC open GOP (one IDR at the head, CRA keyframes after) + E-AC-3 5.1.
# shellcheck disable=SC2046
make Bench.Charlie.2003.720p.WEB.HEVC.EAC3-IRIS.mkv $(video 1280x720 60) \
	-f lavfi -i "aevalsrc=exprs=${bed}|${bed}|${beep}|0|${bed}|${bed}:s=48000:c=5.1:d=60" \
	-c:v libx265 -preset ultrafast -pix_fmt yuv420p -tag:v hev1 \
	-x265-params "log-level=error:keyint=48:min-keyint=48:scenecut=0:open-gop=1:bframes=3" \
	-metadata:s:a:0 language=eng -c:a eac3 -b:a 384k -ar 48000

# Scenario 3, 4 without libav.js: the same HEVC open GOP with AAC stereo.
# shellcheck disable=SC2046
make Bench.Kilo.2011.720p.WEB.HEVC.AAC-IRIS.mkv $(video 1280x720 60) $(stereo48 60) \
	-c:v libx265 -preset ultrafast -pix_fmt yuv420p -tag:v hev1 \
	-x265-params "log-level=error:keyint=48:min-keyint=48:scenecut=0:open-gop=1:bframes=3" \
	-metadata:s:a:0 language=eng -c:a aac -b:a 128k -ar 48000

# Scenario 1: mono audio (must reach both ears).
# shellcheck disable=SC2046
make Bench.Delta.2004.360p.WEB.H264.AAC.Mono-IRIS.mp4 $(video 640x360 45) \
	-f lavfi -i "aevalsrc=exprs=${beep}+${bed}:s=48000:c=mono:d=45" \
	"${x264[@]}" -metadata:s:a:0 language=eng -c:a aac -b:a 96k -ac 1 -ar 48000 -movflags +faststart

# Scenario 1: 5.1 AAC whose beep (the "dialogue") is in the centre channel only.
# shellcheck disable=SC2046
make Bench.Echo.2005.360p.WEB.H264.AAC5.1-IRIS.mp4 $(video 640x360 45) \
	-f lavfi -i "aevalsrc=exprs=0|0|${beep}|0|0|0:s=48000:c=5.1:d=45" \
	"${x264[@]}" -metadata:s:a:0 language=eng -c:a aac -b:a 256k -ar 48000 -movflags +faststart

# Scenario 1: no audio track at all.
# shellcheck disable=SC2046
make Bench.Foxtrot.2006.360p.WEB.H264.NoAudio-IRIS.mp4 $(video 640x360 40) \
	"${x264[@]}" -an -movflags +faststart

# Scenario 8: an ASS track (a karaoke line among plain cues), muxed into Matroska.
ass="$out/bench.ass"
if [[ ! -f "$ass" || "$force" == "--force" ]]; then
	{
		printf '[Script Info]\nScriptType: v4.00+\nPlayResX: 640\nPlayResY: 360\n\n'
		printf '[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n'
		printf 'Style: Default,Arial,36,&H00FFFFFF,&H000000FF,&H00000000,&H80000000,0,0,0,0,100,100,0,0,1,3,0,2,20,20,30,1\n\n'
		printf '[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n'
		for s in $(seq 0 2 58); do
			printf 'Dialogue: 0,0:00:%02d.00,0:00:%02d.00,Default,,0,0,0,,Cue at %d seconds\n' "$s" "$((s + 1))" "$s"
		done
		printf 'Dialogue: 0,0:00:20.00,0:00:24.00,Default,,0,0,0,,{\\k100}Ka{\\k100}ra{\\k100}o{\\k100}ke\n'
	} >"$ass"
fi
# shellcheck disable=SC2046
make Bench.Golf.2007.360p.WEB.H264.AAC.ASS-IRIS.mkv $(video 640x360 60) $(stereo48 60) -i "$ass" \
	-map 0:v -map 1:a -map 2:s "${x264[@]}" -metadata:s:a:0 language=eng -c:a aac -b:a 128k -ar 48000 -c:s ass \
	-metadata:s:s:0 language=eng

# Scenario 8: a PGS track, drawn by our own encoder (ffmpeg has none).
sup="$out/bench.sup"
if [[ ! -f "$sup" || "$force" == "--force" ]]; then
	bun "$here/pgs.ts" "$sup"
fi
# shellcheck disable=SC2046
make Bench.Hotel.2008.360p.WEB.H264.AAC.PGS-IRIS.mkv $(video 640x360 60) $(stereo48 60) -i "$sup" \
	-map 0:v -map 1:a -map 2:s "${x264[@]}" -metadata:s:a:0 language=eng -c:a aac -b:a 128k -ar 48000 -c:s copy \
	-metadata:s:s:0 language=eng

# Scenario 10 (phone, Tier B): H.264 + E-AC-3 stereo in Matroska.
# shellcheck disable=SC2046
make Bench.India.2009.360p.WEB.H264.EAC3-IRIS.mkv $(video 640x360 60) $(stereo48 60) \
	"${x264[@]}" -metadata:s:a:0 language=eng -c:a eac3 -b:a 192k -ar 48000

# Scenario 7 (resume): heavy enough (6 Mbps, two minutes) that a browser can't fetch it whole
# up front, so where the first requests land says where the player started.
# shellcheck disable=SC2046
make Bench.Juliet.2010.720p.WEB.H264.AAC-IRIS.mp4 $(video 1280x720 120) $(stereo48 120) \
	-c:v libx264 -preset ultrafast -pix_fmt yuv420p -g 48 -keyint_min 48 -sc_threshold 0 \
	-b:v 6M -minrate 6M -maxrate 6M -bufsize 3M -x264-params nal-hrd=cbr \
	-metadata:s:a:0 language=eng -c:a aac -b:a 128k -ar 48000 -movflags +faststart

# Scenario 9: two short episodes of one series (next episode, PiP).
for ep in 01 02; do
	# shellcheck disable=SC2046
	make Bench.Show.S01E${ep}.360p.WEB.H264.AAC-IRIS.mp4 $(video 640x360 30) $(stereo48 30) \
		"${x264[@]}" -metadata:s:a:0 language=eng -c:a aac -b:a 128k -ar 48000 -movflags +faststart
done

# Scenario 14 (A/V sync): 60 fps black with a white flash on the first two frames of every
# second, and a 1 kHz beep starting on the same instant (silence otherwise: a sharp onset).
# A one-minute clip, and a 30-minute one for the opt-in drift run.
sync_clip() {
	local name="$1" secs="$2"
	# shellcheck disable=SC2046
	make "$name" \
		-f lavfi -i "color=c=black:s=640x360:r=60:d=${secs},format=yuv420p,drawbox=c=white:t=fill:enable='lt(mod(n\,60)\,2)'" \
		-f lavfi -i "aevalsrc=exprs=if(lt(mod(t\,1)\,0.08)\,0.6*sin(2*PI*1000*t)\,0)|if(lt(mod(t\,1)\,0.08)\,0.6*sin(2*PI*1000*t)\,0):s=48000:c=stereo:d=${secs}" \
		-c:v libx264 -preset veryfast -profile:v high -pix_fmt yuv420p -g 120 -keyint_min 120 -sc_threshold 0 \
		-metadata:s:a:0 language=eng -c:a aac -b:a 128k -ar 48000 -movflags +faststart
}
sync_clip Bench.Lima.2012.360p.WEB.H264.AAC.Sync-IRIS.mp4 60
sync_clip Bench.Mike.2013.360p.WEB.H264.AAC.Sync-IRIS.mp4 1800

# Scenario 5: the loop the local live HLS source plays (the harness runs ffmpeg on it).
# shellcheck disable=SC2046
make live-loop.ts $(video 640x360 20) $(stereo48 20) "${x264[@]}" -c:a aac -b:a 128k -ar 48000 -f mpegts
