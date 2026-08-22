# Bug report draft: MSE drops an HEVC coded frame group that starts on a CRA (macOS)

To file against **Core :: Audio/Video: Playback**. Follow-up to
[bug 2049615](https://bugzilla.mozilla.org/show_bug.cgi?id=2049615).

Reproducer in `upstream/`: three 25 KB fragmented MP4s, a shell script that
regenerates them, and one self-contained HTML page.

## Summary

On macOS, Gecko 154 does not treat an HEVC CRA picture as a random access point.
On the file-playback path the demuxer falls back to the preceding IDR. On the
MSE path there is nothing to fall back to, because the page supplies the
fragments. The coded frame group is never opened: the CRA is dropped, `need
random access point` stays true, and every following sample is dropped with it.

The failure is silent. Appends succeed, `updateend` fires, `readyState` reaches
`HAVE_METADATA`, `SourceBuffer.buffered` stays empty. No `error` event, no
`MediaError`, nothing in the console.

In the open-GOP HEVC streams this affects, the only IDR is at t=0 and every
later keyframe is a CRA. Such a stream plays from the start and cannot be seeked
or resumed.

## Environment

|              |                                                                          |
| ------------ | ------------------------------------------------------------------------ |
| Affected     | Gecko 154 on macOS (measured on Zen 1.21.15b, `rv:154.0`, Apple silicon) |
| Not affected | Gecko 153 on macOS; Chrome 151 on macOS                                  |
| Content      | HEVC Main 10, open-GOP (x265 `--open-gop`)                               |

Bug 2049615 is `RESOLVED FIXED`, `target_milestone` `154 Branch`;
`cf_status_firefox152` and `cf_status_firefox153` are `wontfix`.

## Steps to reproduce

```
cd upstream && ./make-repro.sh && python3 -m http.server 8099
# open http://127.0.0.1:8099/repro.html
```

`make-repro.sh` cuts three 4 s fragmented MP4s from two open-GOP HEVC Main 10
sources that differ only in B-frame count:

| file                  | starts on         | leading pictures   |
| --------------------- | ----------------- | ------------------ |
| `A-idr-start.mp4`     | `IDR_N_LP` at t=0 | none               |
| `B-cra-clean.mp4`     | `CRA_NUT` at t=6  | none (`bframes=0`) |
| `C-cra-with-rasl.mp4` | `CRA_NUT` at t=6  | 4 RASL pictures    |

`repro.html` appends each file's init segment and first media segment to a fresh
`SourceBuffer`, waits for `updateend`, and reports `buffered` plus
`getVideoPlaybackQuality().totalVideoFrames`. It also rewrites the first slice
NAL's type in place, to establish which types the engine opens a coded frame
group on.

`B-cra-clean.mp4` is the isolated case: a CRA with no leading pictures, so the
fragment references nothing outside itself.

## Measured

Same page, same files, same machine. `frames` is
`getVideoPlaybackQuality().totalVideoFrames` after 1.5 s of playback.

| case                     | first slice NAL | Gecko 153                   | Gecko 154                   | Chrome 151                  |
| ------------------------ | --------------- | --------------------------- | --------------------------- | --------------------------- |
| A, IDR entry (control)   | `IDR_N_LP`      | `0.00-4.00` / 36 / no error | `0.00-4.00` / 37 / no error | `0.00-4.00` / 40 / no error |
| B, clean CRA entry       | `CRA_NUT`       | `0.00-4.00` / 37 / no error | empty / 0 / no error        | `0.00-4.00` / 41 / no error |
| B, relabelled `IDR_N_LP` | `IDR_N_LP`      | `0.00-4.00` / 0 / `-12909`  | `0.00-4.00` / 0 / `-12909`  | `0.00-4.00` / 0 / `-12909`  |
| B, relabelled `BLA_N_LP` | `BLA_N_LP`      | `0.00-4.00` / 37 / no error | empty / 0 / no error        | `0.00-4.00` / 42 / no error |
| C, CRA entry, RASL kept  | `CRA_NUT`       | `0.08-4.36` / 0 / `-17694`  | empty / 0 / no error        | `0.24-4.36` / 43 / no error |

Frame counts vary by a frame or two between runs, since they are whatever the
decoder produced inside the page's 1.5 s window. What does not vary is whether a
row decodes at all.

Row B is the regression. The same 25 KB fragment decodes on Gecko 153 and on
Chrome. Gecko 154 stores nothing and reports nothing.

Chrome and Gecko are both on VideoToolbox here. Chrome's error text in the
`IDR_N_LP` row reads `PipelineStatus::PIPELINE_ERROR_DECODE: Error
Domain=NSOSStatusErrorDomain Code=-12909 "(null)" (-12909):
VTDecompressionOutputCallback`, and Gecko's reads `AppleVTDecoder::OnDecodeError`.

Three further measurements from the same table:

- The `IDR_N_LP` relabel fails identically on all three engines with OSStatus
  `-12909` (`kVTVideoDecoderBadDataErr`). An IDR slice header omits
  `slice_pic_order_cnt_lsb`, so the relabelled slice no longer parses as what it
  claims to be. This row is the control for the relabelling itself.
- `BLA_N_LP` behaves exactly like `CRA_NUT` on all three engines.
- Row C, a CRA entry point with its RASL pictures kept, decodes on Chrome (43
  frames, no error) and fails on Gecko 153 with OSStatus `-17694`
  (`kVTVideoDecoderReferenceMissingErr`). On Gecko 154 the samples do not reach
  the decoder, so the row reads empty for the same reason as row B.

The same behaviour reproduces on a real 1920x960 HEVC Main 10 open-GOP file at a
CRA 178.1 s in: `buffered` empty on Gecko 154, `0.0-10.0` with 149 frames when
the same pipeline starts at the t=0 IDR instead.

## Container signalling

From the mid-stream fragment:

```
tfhd default_sample_flags = 0x01010000   (sample_depends_on=1, non-sync)
trun first_sample_flags   = 0x02000000   (sample_depends_on=2, sync)
first slice NAL           = 21 (CRA_NUT)
```

`first_sample_flags` marks the first sample sync and depending on nothing.

## Mechanism

`dom/media/platforms/agnostic/bytestreams/H265.h`. `H265NALU::IsIframe()`
returns true only for `IDR_W_RADL` and `IDR_N_LP`:

```cpp
bool IsIframe() const {
  return mNalUnitType == NAL_TYPES::IDR_W_RADL ||
         mNalUnitType == NAL_TYPES::IDR_N_LP;
}
```

`dom/media/mp4/MP4Demuxer.cpp`, `GetNextSample()`. Added by bug 2049615, this
overrides the container's sync flag with that answer:

```cpp
} else if (mType == kHEVC && !sample->mCrypto.IsEncrypted()) {
#ifdef MOZ_APPLEMEDIA
  // VideoToolbox can return a bad data error if a CRA frame is the first
  // sample after a seek. Only IDR_W_RADL/IDR_N_LP are safe starting points.
  auto isIDR = H265::IsKeyFrame(sample);
  bool keyframe = isIDR.isOk() && isIDR.unwrap();
  if (sample->mKeyframe != keyframe) {
    NS_WARNING(...);
    sample->mKeyframe = keyframe;
  }
#endif
```

`TrackBuffersManager` demuxes appended segments through this same `MP4Demuxer`,
so the override applies to MSE as well as to file playback.

## What a page can do about it

Nothing. There is no signal to key a fallback on: successful appends,
`updateend`, `readyState` at `HAVE_METADATA`, and `buffered` that never grows
are also what a slow network looks like. The only trace anywhere is the
`NS_WARNING` above, which is not present in a release build.

## Suggested fix

Chromium handles the same decoder constraint on the same platform without
demoting the CRA, and its code is worth comparing against.

**1. Treat every IRAP picture as a random access point.**
`media/formats/mp4/hevc.cc` sets `is_keyframe = true` for the full IRAP set,
`BLA_W_LP` through `RSV_IRAP_VCL23`, and `false` for the non-IRAP VCL types.
`H265NALU::IsIframe()` currently answers for `IDR_W_RADL` and `IDR_N_LP` only.

**2. Handle the VideoToolbox constraint in the VideoToolbox decoder.**
`media/gpu/mac/video_toolbox_h265_accelerator.cc`, in `SubmitSlice`:

```cpp
if (pic->no_rasl_output_flag_ &&
    (slice_hdr->nal_unit_type == H265NALU::RASL_N ||
     slice_hdr->nal_unit_type == H265NALU::RASL_R)) {
  // Drop this RASL frame, otherwise VideoToolbox will fail to decode it.
  drop_frame_ = true;
  return Status::kOk;
}

// Update frame state.
frame_is_keyframe_ = slice_hdr->irap_pic;
```

That is H.265 section 8.1.3: when a CRA begins decoding, `NoRaslOutputFlag` is
1 and its associated RASL pictures are neither decoded nor output. Doing this
in `AppleVTDecoder`, which is the component that has the constraint and knows
when it has just been created or flushed, leaves the demuxer free to report the
bitstream as it is.

The current shape puts a decoder-specific constraint into a container-level
answer, and every consumer of that answer inherits it. `MP4Demuxer` reads it for
seeking, `TrackBuffersManager` reads it for coded frame processing, and the
`#ifdef` makes Gecko's own keyframe semantics differ between macOS and the other
platforms.

The seek retry that bug 2049615 also added, extending the `kH264` fallback to
`kHEVC`, is unaffected by any of this.

**3. Whatever the fix, surface a drop.** A sample discarded during coded frame
processing currently produces nothing observable in a release build. A
`MediaError`, a SourceBuffer error event, or a console warning would let a page
fall back instead of hanging.

## Workarounds tried

|                                                                                                                     | result                                                                                                                                      |
| ------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------- |
| Every `tfhd` / `trun` sync-flag combination                                                                         | no change, the demuxer overrides the container                                                                                              |
| Init segment split off from the first `moof`+`mdat`                                                                 | no change                                                                                                                                   |
| `appendWindowStart`, `timestampOffset`, `mode="sequence"`, duplicate init append, `abort()` before the media append | no change                                                                                                                                   |
| `removeSourceBuffer` + `addSourceBuffer` on seek                                                                    | next seek fails with `MediaError 3` / "manager is detached"                                                                                 |
| Relabel the CRA `IDR_N_LP`                                                                                          | buffers, then `MediaError 3`, OSStatus `-12909`; fails the same way on Chrome                                                               |
| Relabel the CRA `BLA_N_LP`                                                                                          | `buffered` empty, as with the CRA                                                                                                           |
| WebCodecs `VideoDecoder`                                                                                            | `isConfigSupported` returns `supported: false` for `hvc1.2.4.L120.B0`, `hev1.2.4.L120.B0` and `hvc1.1.6.L93.B0`; `avc1.640028` returns true |

We ship a workaround: a WASM HEVC decoder in a worker re-encodes to H.264 via
`VideoEncoder`, so the `SourceBuffer` codec is `avc1` and Gecko never sees HEVC.
Seeking works. It replaces the hardware HEVC decoding the same browser uses for
`<video>` playback with a software decode that runs between 0.7x and 2x real
time depending on machine load.

## Attachments

| file                           |                                                |
| ------------------------------ | ---------------------------------------------- |
| `upstream/make-repro.sh`       | regenerates the three files (ffmpeg + libx265) |
| `upstream/A-idr-start.mp4`     | 25 KB, IDR entry point                         |
| `upstream/B-cra-clean.mp4`     | 25 KB, CRA entry point, no leading pictures    |
| `upstream/C-cra-with-rasl.mp4` | 26 KB, CRA entry point, RASL kept              |
| `upstream/repro.html`          | prints the table above                         |

`repro.html` derives each file's `hvc1` codec string from its own `hvcC`, so any
HEVC fragmented MP4 dropped next to it works unchanged. A short excerpt of the
real-world 1920x960 Main 10 file is available on request.
