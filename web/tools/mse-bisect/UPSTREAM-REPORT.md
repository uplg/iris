# Draft bug report — HEVC open-GOP CRA is dropped on the MSE path (macOS)

Ready to file against Core :: Audio/Video: Playback, as a follow-up to
bug 2049615. Everything below was measured on the machine described.

---

**Summary:** Bug 2049615's fix leaves Media Source Extensions with no fallback:
an open-GOP HEVC coded frame group starting on a CRA is silently dropped, with
no error and no event, and `SourceBuffer.buffered` stays empty forever.

**Regression from:** bug 2049615 (fixed in Firefox 154), itself a regression fix
for bug 1967475.

## What happens

`MP4Demuxer.cpp` strips the keyframe flag from CRA pictures under
`#ifdef MOZ_APPLEMEDIA`, so `H265::IsKeyFrame()` — which counts only
`IDR_W_RADL` / `IDR_N_LP` — reports false. For file playback the demuxer then
falls back to the preceding IDR, which is exactly what the patch's own
`SeekHEVC` test asserts, and it works.

MSE has no preceding IDR to fall back to: the page supplies the fragments. Coded
frame processing needs a random access point after an init segment, does not get
one, and `need random access point` stays true — so every subsequent sample is
dropped too. The SourceBuffer accepts megabytes of appends, `readyState` reaches
HAVE_METADATA, `updateend` fires normally, and nothing is ever buffered. No
`error` event, no `MediaError`, no console output.

Open-GOP HEVC carries a single IDR, at t=0. So such a stream plays from the
start and can never be seeked or resumed, which is how this shows up in practice.

## Steps to reproduce

Using the patch's own reproducer:

    ffmpeg -f lavfi -i testsrc=duration=4:size=128x96:rate=30 \
      -c:v libx265 -x265-params keyint=30:min-keyint=30:open-gop=1:info=0 \
      -an test_hevc_open_gop.mp4

Remux it to fragmented MP4 starting at a mid-stream keyframe (a CRA), append the
init segment then the media segment to a `SourceBuffer` opened with
`video/mp4; codecs="hvc1.…"`, and read `buffered`.

## Measured

macOS on Apple silicon, same file, same machine:

| | file playback, seek(2.0) | MSE, mid-stream append |
| --- | --- | --- |
| Firefox 153 (without the fix) | `MediaError` 3, `AppleVTDecoder::OnDecodeError:ffffbae2` | works, `buffered=[178.1-188.1]` |
| Gecko 154 (with the fix) | works, `ct=4.00`, 61 frames, no error | `buffered=[empty]`, no error |

Container flags are correct and identical between the t=0 fragment and the
mid-stream one — `tfhd default_sample_flags=0x01010000`,
`trun first_sample_flags=0x02000000` (sync, `sample_depends_on=2`). Chrome
accepts both.

Relabelling the CRA as `IDR_N_LP` gets the fragment buffered and then dies in
VideoToolbox with `kVTVideoDecoderBadDataErr` (-12909) — the very error the
guard exists to prevent — because an IDR slice header omits
`slice_pic_order_cnt_lsb`. `BLA_N_LP`, whose slice header matches a CRA's, is
refused at the buffering stage exactly like the CRA.

## Why this is worth a separate bug

The failure is silent. A page cannot detect it: appends succeed, `updateend`
fires, `readyState` advances, and `buffered` simply never grows. There is no way
to tell "this browser will not start here" apart from "the network is slow",
which makes a fallback impossible to trigger reliably.

Two directions, either would be enough:

1. Let MSE keep the CRA keyframe flag — the concern the guard addresses is a
   fresh VideoToolbox decode session after a demuxer seek, which the MSE path
   does not perform.
2. If the frames must be dropped, surface it: a `MediaError`, or a decode-error
   event on the SourceBuffer, so a page can fall back instead of hanging.
