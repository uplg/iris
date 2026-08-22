# mse-bisect

Bench used to settle the Firefox "resume stalls, t=0 plays" hunt. It mounts the
real `mountTierB` in a bare page — no React, no chrome, no Iris app — driven by
Playwright Firefox. It reproduces the engine faithfully and does NOT reproduce
the bug, which is how we learned the fault was not in Tier B.

The conclusion and the bug report live in [`UPSTREAM-REPORT.md`](UPSTREAM-REPORT.md).
The minimal reproducer Mozilla can run is in [`upstream/`](upstream/).

    bun build ../../src/lib/iris-core/tiers/tier-b-mse.ts --outfile tierb.js \
      --format esm --target browser --external mediabunny --external libav.js
    bun build ../../src/lib/iris-core/decode/libav-audio-decoder.ts --outfile libavdec.js \
      --format esm --external mediabunny --external libav.js

    node serve.mjs &
    node drive.mjs '?at=0,178,373'                  # local file, MKV path in serve.mjs
    node drive.mjs '?src=/proxy/stream&at=82.7,373' # against a real /stream endpoint

`serve.mjs` serves the MKV with byte ranges, mediabunny's browser bundle, the
libav wasm (drop the `-iris` variant into `libavjs/` for E-AC-3) and the page.
Each run prints `buffered=[…]` per start position — empty means Firefox took the
appends and stored nothing, which is the failure signature.

AC-3 / E-AC-3 decode needs the Docker-built libav variant:

    docker build --target libav-builder -t iris-libav . && cid=$(docker create iris-libav)
    docker cp $cid:/libav-iris.wasm      libavjs/libav-6.10.9.0-iris.wasm.wasm
    docker cp $cid:/libav-iris.wasm.mjs  libavjs/libav-6.10.9.0-iris.wasm.mjs
    docker cp $cid:/libav-iris.wasm.js   libavjs/libav-6.10.9.0-iris.wasm.js

## upstream/ — the minimal reproducer

Self-contained, no Iris code, no dependencies beyond a static file server. This
is what goes on the bug. See `upstream/make-repro.sh` for how the three files
are generated and `upstream/repro.html` for what it measures.

    cd upstream && ./make-repro.sh && python3 -m http.server 8099
    # then http://127.0.0.1:8099/repro.html in the browser under test

The page derives each file's `hvc1.…` codec string from its own `hvcC`, so any
HEVC fragmented MP4 dropped next to it works unchanged.

## zen-check.html — random HEVC entry point, against the real file

Open it in the browser under test, by hand: Playwright only drives its own
Firefox, not a fork like Zen. (geckodriver can drive one, but the setup is
brittle and `upstream/repro.html` covers the same ground with less ceremony.)

    bun gen-variants.mjs /path/to/film.mkv 180   # writes midstream.mp4
    node serve.mjs &
    # then http://127.0.0.1:8099/zen-check.html

The page relabels the fragment's first slice NAL and measures what MSE does with
each type. Reading the table:

- `buffered` **empty** → the browser refuses to enter on that NAL type;
- `buffered` filled but `frames=0` → it accepts it and cannot decode it;
- `frames` > 0 with non-zero pixel variance → a real picture.

Measured on Zen 1.21.15b (`rv:154.0`), x265 open-GOP file:

| coded frame group starts on           | buffered      | frames decoded                 |
| ------------------------------------- | ------------- | ------------------------------ |
| `IDR_N_LP` (t=0, the file's only IDR) | `0.0–10.0`    | 149                            |
| `CRA_NUT` (any mid-stream keyframe)   | **empty**     | 0                              |
| `CRA_NUT`, mediabunny muxer unpatched | **empty**     | 0                              |
| `CRA_NUT` → `IDR_N_LP`                | `178.1–188.0` | 0, `kVTVideoDecoderBadDataErr` |
| `CRA_NUT` → `BLA_N_LP`                | **empty**     | 0                              |

This Gecko opens a coded frame group only on an **IDR**. CRA and BLA are refused
at the buffering stage; relabelling to IDR gets past buffering and then breaks
decode, because an IDR slice header omits `slice_pic_order_cnt_lsb`. Converting
a CRA to a real IDR would mean rewriting the POC of every following picture in
the GOP — bitstream surgery in the hot path, out of the question here.

One method note, learned the hard way: `HTMLMediaElement.play()` returns a
promise that **never settles** when playback cannot start. `await v.play().catch(…)`
then blocks forever and the `.catch` changes nothing. Never await it in a bench.

## Why this is Firefox 154+ behaving as intended, not a passing regression

The mechanism is in Gecko and Mozilla's own comment says so. See the Mechanism
section of `UPSTREAM-REPORT.md` for the quoted source; the short version is that
`H265NALU::IsIframe()` counts only IDR as an intra picture, and `MP4Demuxer.cpp`
overwrites the container's sync flag with that answer under `#ifdef MOZ_APPLEMEDIA`.

**Mind the direction of the upstream history.** Bug 1967475 (fixed in 146)
introduced the override for H.264. Bug 2049615 (fixed in **154**) extended it to
HEVC: its patch strips the keyframe flag from CRA pictures. So it is not a fix
that restores CRA seeking — it is the one that forbids it, for file playback,
where the demuxer can then fall back to a real IDR. In MSE there is nothing to
fall back to: the page supplies the fragments and Gecko discards what it is
given.

Measured here, and consistent with that reading — and note that a fork's product
version says nothing about its Gecko base, read `navigator.userAgent`:

| engine                                     | HEVC open-GOP seek in MSE           |
| ------------------------------------------ | ----------------------------------- |
| Gecko 153 (Playwright's Firefox)           | works — the CRA is still a keyframe |
| Gecko 154 (Zen 1.21.15b, build 2026-08-18) | `buffered` empty — CRA demoted      |

Updating therefore changes nothing: this is the current, intended state of
Firefox 154+ on macOS. Our container flags are correct
(`trun first_sample_flags=0x02000000`: sync, depends on nothing), identical
between the t=0 fragment and the mid-stream one, and accepted by every other
engine.

## Zen does ship the fix — and it is the fix that breaks us

Verified with the upstream patch's own reproducer, generated with its own
command, in **file** playback (not MSE), seeking to 2.0 s like its test:

    ffmpeg -f lavfi -i testsrc=duration=4:size=128x96:rate=30 \
      -c:v libx265 -x265-params keyint=30:min-keyint=30:open-gop=1:info=0 \
      -an test_hevc_open_gop.mp4

| engine          | `seek(2.0)` on file playback                                      |
| --------------- | ----------------------------------------------------------------- |
| Firefox 153     | `err=3 AppleVTDecoder::OnDecodeError:ffffbae2` — the original bug |
| Zen / Gecko 154 | `ct=4.00 frames=61 err=none` — fixed                              |

Hence the apparent inversion in our measurements, which is in fact perfectly
coherent:

|                               | file playback | MSE mid-stream   |
| ----------------------------- | ------------- | ---------------- |
| Firefox 153 (without the fix) | fails         | works            |
| Gecko 154 (with it)           | works         | `buffered` empty |

## The workaround that works: hevc.js on the UNMUXED path

`hevcjs-videoonly-probe.html` — measured on Zen / Gecko 154 with a video-only
fMP4 starting on a mid-stream CRA (the case that breaks Tier B):

    addSourceBuffer("video/mp4; codecs=\"hvc1.2.4.L120.B0\"") → H.264 proxy, avc1.64002a
    Worker transcoder ready
    Init segment parsed
    Transcoding segment (1539985B) [streaming]…
    H.264 init segment appended [streaming]
    Streaming done (8 chunks), buffered: 187.40s
    RESULT: buffered=[178.1-188.0] frames=124 rs=4 err=none

Gecko therefore never sees HEVC, never sees a CRA, and the `MP4Demuxer.cpp`
guard never bites. This works even though hevc.js's own matrix says
"Chrome/Edge/Firefox (Mac) → No — native": on macOS HEVC is native, so the lib
steps aside by default — but nothing stops it running if you install it
explicitly.

Two traps worth knowing before writing an adapter:

1. **The muxed path is AAC-only.** The README is explicit: "for muxed A/V … the
   AAC audio is passed through … (main-thread path; AAC only)". The muxed proxy
   is created by asking for `avc1…,mp4a.40.2`, and on Firefox our audio is Opus
   (no AAC encoder in WebCodecs). The append queue then never drains, without an
   error. Hence: **video alone through the proxy, audio in a native SourceBuffer
   alongside.**
2. **hevc.js transfers the buffer to its worker.** Passing a `subarray` of a
   shared buffer detaches the parent: the next segment arrives at `0B` and
   throws "attempting to access detached ArrayBuffer". Every `appendBuffer` must
   own its buffer. Tier B already does (`next.slice().buffer`).

## What Tier E costs, and what governs it

The WASM decode is the bottleneck — the H.264 encode side already goes through
WebCodecs, and Firefox's `VideoDecoder` answers `supported: false` for every
hvc1/hev1 config, so hardware HEVC decode is not reachable from the page.

Measured throughput on a 1920×960 Main 10 rip swings either side of real time
depending on what else the machine is doing. hevc.js publishes one
`SegmentPerfStat` per transcoded segment on its perf bus; Tier E subscribes to
it and uses `speedX` to size its runway, holds playback until a cushion exists
rather than stuttering into one, and caps how much media may sit in the proxy's
queue untranscoded. See the comments at the top of `tier-e-hevcjs.ts`.
