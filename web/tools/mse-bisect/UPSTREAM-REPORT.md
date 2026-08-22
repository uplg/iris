# Bug report draft — HEVC open-GOP: a CRA start is dropped silently on the MSE path (macOS)

Ready to file against **Core :: Audio/Video: Playback**, as a follow-up to
[bug 2049615](https://bugzilla.mozilla.org/show_bug.cgi?id=2049615).

Everything below was measured on the machine described in
[Environment](#environment). The reproducer is in `upstream/`: three 25 KB
fragmented MP4s produced by a shell script, plus one self-contained HTML page.

---

## Summary

Bug 2049615 stops Firefox from treating a CRA picture as a keyframe on Apple
platforms. For file playback the demuxer then falls back to the preceding IDR
and playback works. **Media Source Extensions has nothing to fall back to** —
the page chooses the fragments — so an HEVC coded frame group starting on a CRA
is dropped, and every sample after it with it. There is no error, no event, and
no way for a page to detect it: appends succeed, `updateend` fires, `readyState`
reaches `HAVE_METADATA`, and `SourceBuffer.buffered` stays empty forever.

Open-GOP HEVC carries exactly one IDR, at t=0. Such a stream therefore plays
from the start and can never be seeked or resumed. That is how this shows up in
practice, and it is what it does to us.

There is a second finding below that we think matters more than the first: the
CRA is not the problem VideoToolbox actually has. **A CRA fragment with its RASL
leading pictures removed decodes perfectly**; it is the RASL pictures — which
the spec says must be discarded when a CRA begins decoding — that make
VideoToolbox fail. If that is right, the guard added in bug 2049615 can be
removed entirely rather than extended to MSE.

## Environment

|              |                                                                                                                                                                                                        |
| ------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Hardware     | Apple silicon, macOS                                                                                                                                                                                   |
| Affected     | Gecko 154+ on macOS (measured on Zen 1.21.15b, build 2026-08-18, `rv:154.0`)                                                                                                                           |
| Not affected | Gecko 153 and earlier on macOS; Chromium on any platform; Firefox on Windows and Linux (no `MOZ_APPLEMEDIA`)                                                                                           |
| Regressed by | [bug 2049615](https://bugzilla.mozilla.org/show_bug.cgi?id=2049615), fixed in 154 — itself an extension of [bug 1967475](https://bugzilla.mozilla.org/show_bug.cgi?id=1967475), fixed in 146 for H.264 |
| Content      | HEVC Main 10, open-GOP (x265 `--open-gop`), 1920×960 and synthetic 640×360                                                                                                                             |

Note on the milestone flags: bug 2049615 is `RESOLVED FIXED`, `target_milestone`
`154 Branch`, with `cf_status_firefox152` and `cf_status_firefox153` set to
`wontfix`. Those are backport decisions, not a reversal — 154 ships the change.

## Mechanism

Two pieces of Gecko combine.

`dom/media/platforms/agnostic/bytestreams/H265.h` (lines 130–134) counts only
IDR pictures as intra, although CRA and BLA are also IRAP pictures per the
standard:

```cpp
bool IsIframe() const {
  return mNalUnitType == NAL_TYPES::IDR_W_RADL ||
         mNalUnitType == NAL_TYPES::IDR_N_LP;
}
```

`dom/media/mp4/MP4Demuxer.cpp` (lines 565–579, added by bug 2049615) then
overrides the container's own sync-sample signalling with that answer, on Apple
platforms only:

```cpp
} else if (mType == kHEVC && !sample->mCrypto.IsEncrypted()) {
#ifdef MOZ_APPLEMEDIA
  // VideoToolbox can return a bad data error if a CRA frame is the first
  // sample after a seek. Only IDR_W_RADL/IDR_N_LP are safe starting points.
  auto isIDR = H265::IsKeyFrame(sample);
  bool keyframe = isIDR.isOk() && isIDR.unwrap();
  if (sample->mKeyframe != keyframe) {
    NS_WARNING(nsPrintfCString(
      "HEVC frame incorrectly marked as %skeyframe "
      "@ pts:%" PRId64 " dur:%" PRId64 " dts:%" PRId64,
      keyframe ? "" : "non-", sample->mTime.ToMicroseconds(),
      sample->mDuration.ToMicroseconds(),
      sample->mTimecode.ToMicroseconds())
    .get());
    sample->mKeyframe = keyframe;
  }
#endif
```

`TrackBuffersManager` demuxes appended segments through this same `MP4Demuxer`.
So on the MSE path the CRA arrives at coded frame processing already stripped of
its keyframe flag. The algorithm requires a random access point after an init
segment, does not get one, and `need random access point` stays true — which
drops that sample and, because the flag never clears, every sample after it.

The container signalling on our side is correct and unambiguous. From the
mid-stream fragment of the real file:

```
tfhd default_sample_flags = 0x01010000   (sample_depends_on=1, non-sync)
trun first_sample_flags   = 0x02000000   (sample_depends_on=2, sync)
first slice NAL           = 21 (CRA_NUT)
```

`first_sample_flags` says sync, depends on nothing. Chromium accepts it. Gecko
153 accepts it. Gecko 154 discards it.

## Steps to reproduce

`upstream/make-repro.sh` builds three fragmented MP4s from two open-GOP HEVC
Main 10 sources that differ only in B-frame count:

| file                  | starts on         | leading pictures   |
| --------------------- | ----------------- | ------------------ |
| `A-idr-start.mp4`     | `IDR_N_LP` at t=0 | none               |
| `B-cra-clean.mp4`     | `CRA_NUT` at t=6  | none (`bframes=0`) |
| `C-cra-with-rasl.mp4` | `CRA_NUT` at t=6  | 4 RASL pictures    |

`upstream/repro.html` appends each file's init segment and first media segment
to a fresh `SourceBuffer`, waits for `updateend`, then reports `buffered` and
the frame count from `getVideoPlaybackQuality()`. It also rewrites the first
slice NAL's type in place, to establish which NAL types the engine will open a
coded frame group on. Serve the directory over HTTP and open the page:

```
cd upstream && ./make-repro.sh && python3 -m http.server 8099
# then http://127.0.0.1:8099/repro.html
```

`B-cra-clean.mp4` is the isolated case. It is a complete, self-contained random
access point: nothing in the fragment references anything outside it, and it
decodes on every other engine we tried.

## Measured

### Gecko 154 (with the fix) — the regression

Real 1920×960 HEVC Main 10 open-GOP file, fragment starting at the CRA at
178.1 s, produced by our own remuxer (leading pictures excluded from the feed):

| coded frame group starts on           | `buffered`    | frames decoded                 |
| ------------------------------------- | ------------- | ------------------------------ |
| `IDR_N_LP` (t=0, the file's only IDR) | `0.0–10.0`    | 149                            |
| `CRA_NUT` (any mid-stream keyframe)   | **empty**     | 0                              |
| `CRA_NUT` relabelled `IDR_N_LP`       | `178.1–188.0` | 0, `kVTVideoDecoderBadDataErr` |
| `CRA_NUT` relabelled `BLA_N_LP`       | **empty**     | 0                              |

No error, no event, no console output in the CRA row. The appends complete
normally.

### Gecko 153 (without the fix) — same page, same files

Firefox 153 (Playwright build), synthetic files from `make-repro.sh`:

| case                          | first slice NAL | `buffered`  | frames | error                             |
| ----------------------------- | --------------- | ----------- | ------ | --------------------------------- |
| A — t=0, IDR start (control)  | `IDR_N_LP`      | `0.00–4.00` | 36     | none                              |
| B — t=6, clean CRA            | `CRA_NUT`       | `0.00–4.00` | **37** | none                              |
| B — CRA relabelled `IDR_N_LP` | `IDR_N_LP`      | `0.00–4.00` | 0      | `MediaError 3`, OSStatus `-12909` |
| B — CRA relabelled `BLA_N_LP` | `BLA_N_LP`      | `0.00–4.00` | **37** | none                              |
| C — t=6, CRA with RASL kept   | `CRA_NUT`       | `0.08–4.36` | 0      | `MediaError 3`, OSStatus `-17694` |

### File playback, both engines

Using bug 2049615's own reproducer command and its own seek target:

```
ffmpeg -f lavfi -i testsrc=duration=4:size=128x96:rate=30 \
  -c:v libx265 -x265-params keyint=30:min-keyint=30:open-gop=1:info=0 \
  -an test_hevc_open_gop.mp4
```

| engine                        | `seek(2.0)` on file playback                             | MSE, mid-stream CRA append |
| ----------------------------- | -------------------------------------------------------- | -------------------------- |
| Firefox 153 (without the fix) | `MediaError 3`, `AppleVTDecoder::OnDecodeError:ffffbae2` | works                      |
| Gecko 154 (with the fix)      | works — `ct=4.00`, 61 frames, no error                   | **`buffered` empty**       |

The inversion is exactly what the patch predicts. Its own test says so: "CRA
keyframe flags are stripped on Apple platforms, so the seek falls back to the
preceding IDR". In file playback there is always an IDR upstream to fall back
to. In MSE there is not.

## The second finding: the CRA is not what VideoToolbox chokes on

Compare rows B and C above, on the engine that still lets a CRA through:

- **B — CRA, leading pictures removed: 37 frames, no error.** VideoToolbox
  decodes a CRA as the first sample after a seek, cleanly.
- **C — the same CRA with its 4 RASL pictures kept: 0 frames**, OSStatus
  `-17694` = `kVTVideoDecoderReferenceMissingErr`.

RASL pictures associated with a CRA reference pictures that precede it. When the
CRA is the first picture of the bitstream, or the first picture after a random
access, H.265 §8.1.3 sets `NoRaslOutputFlag = 1` and those RASL pictures **are
not decoded and not output**. Feeding them to any decoder asks for references
that do not exist, and `kVTVideoDecoderReferenceMissingErr` is precisely that
complaint.

Bug 2049615's reporter was seeking in a file. The demuxer seeks to the CRA and
feeds VideoToolbox the CRA followed by its RASL pictures in decode order — case
C. We cannot check this against the original video (it is attached to a security
bug), so this is a hypothesis, but it fits every measurement we have and it
explains why the "bad data error" only ever appeared on open-GOP content with
B-frames.

If it holds, the guard in `MP4Demuxer.cpp` is treating the symptom. Discarding
RASL pictures when `NoRaslOutputFlag = 1` fixes the cause, restores CRA seeking
on file playback _and_ on MSE, and lets both the H.264 and HEVC branches of that
`#ifdef` go away.

Also worth noting from the same table: `BLA_N_LP` decodes fine on 153 (37
frames), and is refused at the buffering stage on 154 like the CRA. Whatever
replaces the current guard, `H265NALU::IsIframe()` counting only IDR is worth
revisiting on its own — BLA and CRA are IRAP pictures too.

## Why the silence is the worst part

A page cannot detect this failure. Appends succeed. `updateend` fires.
`readyState` advances to `HAVE_METADATA`. `SourceBuffer.buffered` simply never
grows, which is indistinguishable from a slow network. There is no `error`
event on the SourceBuffer, no `MediaError` on the element, and nothing in the
console — the only trace anywhere is an `NS_WARNING` in the demuxer, invisible
in a release build.

So a site cannot fall back. It cannot even tell the user what went wrong. We
found this by bisecting a player against four browsers over two days.

Either of these would be enough for us:

1. **Preferred — remove the guard and discard RASL pictures instead** when a CRA
   begins decoding, per §8.1.3. Our measurements say the CRA itself is a safe
   entry point for VideoToolbox once its RASL pictures are gone.
2. **Failing that, keep the CRA keyframe flag on the MSE path.** The concern the
   guard addresses is a fresh VideoToolbox decode session after a _demuxer_
   seek; MSE performs no such seek, and the browser is not choosing the entry
   point in the first place.
3. **At minimum, make it observable.** A `MediaError`, a decode-error event on
   the SourceBuffer, or a console warning in release builds — anything a page
   can key a fallback on instead of hanging.

## What we tried before filing

Recorded here so nobody repeats it, and because a few of these are how the
diagnosis was narrowed down.

**Container-level.** Rewrote `tfhd default_sample_flags` and `trun
first_sample_flags` by hand into every combination of `sample_depends_on` /
`sample_is_non_sync_sample` that could mean "sync". No change: the demuxer
overrides the container.

**Splitting the init segment** away from the first `moof`+`mdat`, so the
`SourceBuffer` receives `ftyp`+`moov` on its own — the shape every DASH/HLS
player uses. Correct to do anyway; no effect on this.

**`appendWindowStart`, `timestampOffset`, `mode = "sequence"`,** setting
`MediaSource.duration` before/after `addSourceBuffer`, appending the init
segment twice, `abort()` before the media append. None of it changes the
outcome. All of these were measured with the probe page, not assumed.

**Deferring `currentTime`** until a buffered range covers it. This one is a real
fix for a _different_ Firefox behaviour — setting `currentTime` while the
SourceBuffer is empty leaves a pending seek and appends stop committing — and it
is in our shipping code. It does not help here.

**Recycling the SourceBuffer** (`removeSourceBuffer` + `addSourceBuffer`) on
seek. Actively harmful: it detaches `TrackBuffersManager` out from under the
live `MediaSourceTrackDemuxer` and the next seek dies with `media error 3` /
"manager is detached". Also hits `QuotaExceededError: This MediaSource has
reached the limit of SourceBuffer objects` on Chromium.

**Relabelling the CRA as `IDR_N_LP`.** Gets past the buffering stage on 154 and
then dies in VideoToolbox with `kVTVideoDecoderBadDataErr` (-12909) — the very
error the guard exists to prevent. Expected: an IDR slice header omits
`slice_pic_order_cnt_lsb`, so the slice no longer parses as what it claims to
be. Converting a CRA to a real IDR would mean rewriting the POC of every
following picture in the GOP; that is bitstream surgery in the hot path.

**Relabelling as `BLA_N_LP`,** whose slice header layout matches a CRA's.
Refused at the buffering stage on 154, exactly like the CRA. Decodes fine on 153.

**WebCodecs instead of MSE.** `VideoDecoder.isConfigSupported` answers
`supported: false` for `hvc1.2.4.L120.B0`, `hev1.2.4.L120.B0` and
`hvc1.1.6.L93.B0` on Firefox/macOS, while `avc1.640028` answers true for both
decode and encode. So a page cannot reach the platform's HEVC decoder at all,
even though the same browser decodes HEVC in hardware for `<video>` playback.
That closes the one route that would have let us keep hardware decoding, and it
is why the workaround below has to decode in software.

**Waiting for a newer build.** Bug 2049615 is the current, intended behaviour of
Firefox 154+ on macOS. Updating does not help; it is what introduces the
problem. (A fork's product version says nothing about its Gecko base —
`navigator.userAgent` is the only thing to read. Zen 1.21 is `rv:154.0`.)

**Ruling out our own stack.** We rebuilt the failing engine in a bare page with
no framework around it (`tools/mse-bisect`) and it did not reproduce, which is
how we learned the fault was not ours. We also disabled the muxer's open-GOP
PTS-ordering relaxation, tested with and without autoplay blocking
(`media.autoplay.default=0`), with a focused and visible window, and against
both a local file and a real HTTP range server. No difference.

## The workaround we shipped

We route affected playback through [hevc.js](https://github.com/lid-labs/hevc.js),
a WASM HEVC decoder, so **Gecko never sees HEVC at all**: the WASM worker decodes
HEVC and re-encodes H.264 through `VideoEncoder`, and the `SourceBuffer` is
`avc1.…`. No CRA, no `MOZ_APPLEMEDIA` guard, seeking works.

Measured on Gecko 154 with a video-only fMP4 starting on a mid-stream CRA — the
exact fragment that yields `buffered` empty natively:

```
addSourceBuffer('video/mp4; codecs="hvc1.2.4.L120.B0"') → H.264 proxy, avc1.64002a
Transcoding segment (1539985B) [streaming]…
Streaming done (8 chunks), buffered: 187.40s
RESULT: buffered=[178.1-188.0] frames=124 rs=4 err=none
```

Two things to know if anyone else goes this way:

1. **Video-only through the proxy.** hevc.js's muxed A/V path passes audio
   through as AAC only, and Firefox has no AAC encoder in WebCodecs, so our
   audio is Opus. Handing the muxed proxy an Opus stream makes it create a
   `avc1…,mp4a.40.2` buffer and then queue every append forever, without an
   error. We split the tracks: video through the proxy, audio into a second,
   native `SourceBuffer` on the same `MediaSource`.
2. **The proxy transfers the buffer to its worker.** A `subarray` of a shared
   `ArrayBuffer` detaches the parent — the next segment arrives at 0 B and
   throws "attempting to access detached ArrayBuffer". Every `appendBuffer` needs
   to own its buffer.

This is not a fix, it is a tax. It replaces hardware HEVC decoding — which macOS
has, and which Firefox uses perfectly well for playback from t=0 — with a
software decode that runs between 0.7× and 2× real time depending on what else
the machine is doing, on content the platform decodes at a fraction of the
power. We also had to add a loading indicator to the player because seeking now
takes seconds where it used to be instant, and to size the buffering window from
measured transcode throughput so a busy machine does not stutter.

## Impact

HEVC is not exotic content any more. It is what phones record, what streaming
services ship for 4K and HDR, and what practically every modern encode of a
video file uses. Open-GOP is the default in x265 and in most encoding presets,
because it is more efficient.

Any site that plays HEVC through Media Source Extensions on macOS Firefox —
which is to say, any adaptive-streaming player, since MSE is how they all work —
can start playback at t=0 and then never seek or resume. Silently.

---

## Attachments

| file                           |                                                         |
| ------------------------------ | ------------------------------------------------------- |
| `upstream/make-repro.sh`       | regenerates the three files below with ffmpeg + libx265 |
| `upstream/A-idr-start.mp4`     | 25 KB — control, IDR entry point                        |
| `upstream/B-cra-clean.mp4`     | 25 KB — CRA entry point, no leading pictures            |
| `upstream/C-cra-with-rasl.mp4` | 26 KB — CRA entry point, RASL kept                      |
| `upstream/repro.html`          | self-contained page, prints the tables above            |

`repro.html` derives the `hvc1.…` codec string from each file's own `hvcC`, so
dropping any HEVC fragmented MP4 next to it works unchanged. If a synthetic
`testsrc` encode is not convincing enough, we can supply a short excerpt of the
real-world 1920×960 Main 10 file the whole investigation started from — say the
word and we will attach it.
