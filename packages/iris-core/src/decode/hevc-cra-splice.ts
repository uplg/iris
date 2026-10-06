/**
 * HEVC splice: make the CRA picture a coded frame group starts on into an IDR.
 *
 * Gecko 154+ on macOS (`MP4Demuxer.cpp`, bug 2049615) counts only IDR pictures
 * as keyframes; a fragment run that begins on a CRA never opens a coded frame
 * group and MSE stores nothing. An open-GOP rip carries one IDR, at t=0, so
 * every seek and resume is such a run. Our bug on it: bugzilla 2065822.
 *
 * The decoder itself is fine with a CRA start once the RASL leading pictures
 * are gone (Tier B drops them already; Gecko 153 played these fragments in
 * hardware). What Gecko needs is the *label*, and the label is not free: an
 * IDR slice header omits `slice_pic_order_cnt_lsb`, the short-term RPS and
 * `slice_temporal_mvp_enabled_flag`, and an IDR resets the picture order count
 * to 0. So the splice is:
 *
 *   1. rewrite every slice segment header of the CRA picture into IDR_W_RADL
 *      syntax — drop that block, keep every other bit, re-align, keep the
 *      slice data bytes untouched;
 *   2. subtract the CRA's POC lsb from `slice_pic_order_cnt_lsb` of every
 *      later picture in the run (mod MaxPicOrderCntLsb). All reference
 *      picture sets are POC *deltas*, so a constant shift keeps every
 *      reference resolvable; the IDR sits at POC 0 exactly where the CRA was.
 *      A genuine IDR later in the run restarts the count on its own, so the
 *      shift resets there.
 *
 * Nothing else changes: mid-run CRAs stay CRAs (Gecko flags them non-key,
 * which only matters at a run start), their RASL pictures stay decodable.
 * The RPS the CRA carried listed pictures the RASL needed; an IDR has none,
 * and pictures that follow an IRAP never reference across it except as
 * `Foll` entries, which may be missing per spec (8.3.2).
 *
 * Bit-exact but bounded: parameter sets are parsed only as far as the slice
 * header needs, and anything this file does not fully understand (multilayer,
 * 3D and SCC extensions, unknown parameter set ids) throws
 * `HevcSpliceUnsupported` so the caller can fall back to the WASM transcode.
 */

import { EncodedPacket } from "mediabunny";

const NAL = {
  TRAIL_N: 0,
  RADL_N: 6,
  RASL_R: 9,
  BLA_W_LP: 16,
  IDR_W_RADL: 19,
  IDR_N_LP: 20,
  CRA_NUT: 21,
  RSV_IRAP_VCL23: 23,
  VPS: 32,
  SPS: 33,
  PPS: 34,
} as const;

const SLICE_I = 2;

export class HevcSpliceUnsupported extends Error {
  constructor(message: string) {
    super(message);
    this.name = "HevcSpliceUnsupported";
  }
}

class BitReader {
  private pos = 0;
  private readonly bytes: Uint8Array;
  constructor(bytes: Uint8Array) {
    this.bytes = bytes;
  }

  get bitPosition(): number {
    return this.pos;
  }

  get bitsLeft(): number {
    return this.bytes.length * 8 - this.pos;
  }

  u1(): number {
    if (this.pos >= this.bytes.length * 8) {
      throw new HevcSpliceUnsupported("slice header runs past the NAL unit");
    }
    const byte = this.bytes[this.pos >> 3] as number;
    const bit = (byte >> (7 - (this.pos & 7))) & 1;
    this.pos += 1;
    return bit;
  }

  u(n: number): number {
    let v = 0;
    for (let i = 0; i < n; i += 1) v = v * 2 + this.u1();
    return v;
  }

  skip(n: number): void {
    this.pos += n;
  }

  ue(): number {
    let zeros = 0;
    while (this.u1() === 0) {
      zeros += 1;
      if (zeros > 31) throw new HevcSpliceUnsupported("malformed Exp-Golomb code");
    }
    return zeros === 0 ? 0 : (1 << zeros) - 1 + this.u(zeros);
  }

  se(): number {
    const k = this.ue();
    return k % 2 === 1 ? (k + 1) / 2 : -(k / 2);
  }
}

class BitWriter {
  private out: number[] = [];
  private cur = 0;
  private nbits = 0;

  get bitPosition(): number {
    return this.out.length * 8 + this.nbits;
  }

  u1(bit: number): void {
    this.cur = (this.cur << 1) | (bit & 1);
    this.nbits += 1;
    if (this.nbits === 8) {
      this.out.push(this.cur);
      this.cur = 0;
      this.nbits = 0;
    }
  }

  u(value: number, n: number): void {
    for (let i = n - 1; i >= 0; i -= 1) this.u1((value >>> i) & 1);
  }

  /** Copy `n` bits from `src` starting at its current position. */
  copy(src: BitReader, n: number): void {
    for (let i = 0; i < n; i += 1) this.u1(src.u1());
  }

  /** `byte_alignment()`: a one bit, then zeros to the byte boundary. */
  align(): void {
    this.u1(1);
    while (this.nbits !== 0) this.u1(0);
  }

  bytes(): Uint8Array {
    if (this.nbits !== 0) throw new Error("BitWriter: unaligned");
    return Uint8Array.from(this.out);
  }
}

/** Strip emulation-prevention bytes (`00 00 03` → `00 00`). */
export function unescapeRbsp(payload: Uint8Array): Uint8Array {
  const out = new Uint8Array(payload.length);
  let n = 0;
  let zeros = 0;
  for (let i = 0; i < payload.length; i += 1) {
    const b = payload[i] as number;
    if (zeros >= 2 && b === 3) {
      zeros = 0;
      continue;
    }
    out[n] = b;
    n += 1;
    zeros = b === 0 ? zeros + 1 : 0;
  }
  return out.subarray(0, n);
}

/** Insert emulation-prevention bytes wherever `00 00 0x` (x ≤ 3) would appear. */
export function escapeRbsp(rbsp: Uint8Array): Uint8Array {
  const out: number[] = [];
  let zeros = 0;
  for (let i = 0; i < rbsp.length; i += 1) {
    const b = rbsp[i] as number;
    if (zeros >= 2 && b <= 3) {
      out.push(3);
      zeros = 0;
    }
    out.push(b);
    zeros = b === 0 ? zeros + 1 : 0;
  }
  return Uint8Array.from(out);
}

type StRps = {
  numNegative: number;
  numPositive: number;
  deltaS0: number[];
  deltaS1: number[];
  usedS0: number[];
  usedS1: number[];
};

type Sps = {
  id: number;
  chromaArrayType: number;
  separateColourPlane: boolean;
  log2MaxPocLsb: number;
  sliceSegmentAddressBits: number;
  saoEnabled: boolean;
  stRps: StRps[];
  longTermRefPicsPresent: boolean;
  numLongTermRefPicsSps: number;
  temporalMvpEnabled: boolean;
};

type Pps = {
  id: number;
  spsId: number;
  dependentSliceSegmentsEnabled: boolean;
  outputFlagPresent: boolean;
  numExtraSliceHeaderBits: number;
  sliceChromaQpOffsetsPresent: boolean;
  deblockingFilterOverrideEnabled: boolean;
  ppsDeblockingFilterDisabled: boolean;
  loopFilterAcrossSlicesEnabled: boolean;
  tilesEnabled: boolean;
  entropyCodingSyncEnabled: boolean;
  sliceSegmentHeaderExtensionPresent: boolean;
  chromaQpOffsetListEnabled: boolean;
};

function ceilLog2(n: number): number {
  let bits = 0;
  while (1 << bits < n) bits += 1;
  return bits;
}

function parseProfileTierLevel(r: BitReader, maxSubLayersMinus1: number): void {
  r.skip(2 + 1 + 5 + 32 + 4 + 43 + 1); // general_* up to inbld
  r.skip(8); // general_level_idc
  const profilePresent: number[] = [];
  const levelPresent: number[] = [];
  for (let i = 0; i < maxSubLayersMinus1; i += 1) {
    profilePresent.push(r.u1());
    levelPresent.push(r.u1());
  }
  if (maxSubLayersMinus1 > 0) {
    for (let i = maxSubLayersMinus1; i < 8; i += 1) r.skip(2);
  }
  for (let i = 0; i < maxSubLayersMinus1; i += 1) {
    if (profilePresent[i]) r.skip(88);
    if (levelPresent[i]) r.skip(8);
  }
}

function skipScalingListData(r: BitReader): void {
  for (let sizeId = 0; sizeId < 4; sizeId += 1) {
    for (let matrixId = 0; matrixId < 6; matrixId += sizeId === 3 ? 3 : 1) {
      const predModeFlag = r.u1();
      if (!predModeFlag) {
        r.ue(); // scaling_list_pred_matrix_id_delta
      } else {
        const coefNum = Math.min(64, 1 << (4 + (sizeId << 1)));
        if (sizeId > 1) r.se(); // scaling_list_dc_coef_minus8
        for (let i = 0; i < coefNum; i += 1) r.se(); // scaling_list_delta_coef
      }
    }
  }
}

/** `st_ref_pic_set(stRpsIdx)` — parsed AND derived (7.4.8), because a later
 *  set, or the slice's own, may predict from it. */
function parseStRps(r: BitReader, stRpsIdx: number, sets: StRps[], numSetsInSps: number): StRps {
  let interPred = 0;
  if (stRpsIdx !== 0) interPred = r.u1();
  if (interPred) {
    let deltaIdxMinus1 = 0;
    if (stRpsIdx === numSetsInSps) deltaIdxMinus1 = r.ue();
    const deltaRpsSign = r.u1();
    const absDeltaRpsMinus1 = r.ue();
    const refIdx = stRpsIdx - (deltaIdxMinus1 + 1);
    const ref = sets[refIdx];
    if (!ref) throw new HevcSpliceUnsupported("st_ref_pic_set predicts from a missing set");
    const deltaRps = (1 - 2 * deltaRpsSign) * (absDeltaRpsMinus1 + 1);
    const numDeltaPocs = ref.numNegative + ref.numPositive;
    const used: number[] = [];
    const useDelta: number[] = [];
    for (let j = 0; j <= numDeltaPocs; j += 1) {
      used.push(r.u1());
      useDelta.push(used[j] ? 1 : r.u1());
    }
    const out: StRps = {
      numNegative: 0,
      numPositive: 0,
      deltaS0: [],
      deltaS1: [],
      usedS0: [],
      usedS1: [],
    };
    for (let j = ref.numPositive - 1; j >= 0; j -= 1) {
      const dPoc = (ref.deltaS1[j] as number) + deltaRps;
      if (dPoc < 0 && useDelta[ref.numNegative + j]) {
        out.deltaS0.push(dPoc);
        out.usedS0.push(used[ref.numNegative + j] as number);
      }
    }
    if (deltaRps < 0 && useDelta[numDeltaPocs]) {
      out.deltaS0.push(deltaRps);
      out.usedS0.push(used[numDeltaPocs] as number);
    }
    for (let j = 0; j < ref.numNegative; j += 1) {
      const dPoc = (ref.deltaS0[j] as number) + deltaRps;
      if (dPoc < 0 && useDelta[j]) {
        out.deltaS0.push(dPoc);
        out.usedS0.push(used[j] as number);
      }
    }
    for (let j = ref.numNegative - 1; j >= 0; j -= 1) {
      const dPoc = (ref.deltaS0[j] as number) + deltaRps;
      if (dPoc > 0 && useDelta[j]) {
        out.deltaS1.push(dPoc);
        out.usedS1.push(used[j] as number);
      }
    }
    if (deltaRps > 0 && useDelta[numDeltaPocs]) {
      out.deltaS1.push(deltaRps);
      out.usedS1.push(used[numDeltaPocs] as number);
    }
    for (let j = 0; j < ref.numPositive; j += 1) {
      const dPoc = (ref.deltaS1[j] as number) + deltaRps;
      if (dPoc > 0 && useDelta[ref.numNegative + j]) {
        out.deltaS1.push(dPoc);
        out.usedS1.push(used[ref.numNegative + j] as number);
      }
    }
    out.numNegative = out.deltaS0.length;
    out.numPositive = out.deltaS1.length;
    return out;
  }
  const numNegative = r.ue();
  const numPositive = r.ue();
  if (numNegative > 16 || numPositive > 16) {
    throw new HevcSpliceUnsupported("st_ref_pic_set out of range");
  }
  const out: StRps = {
    numNegative,
    numPositive,
    deltaS0: [],
    deltaS1: [],
    usedS0: [],
    usedS1: [],
  };
  let poc = 0;
  for (let i = 0; i < numNegative; i += 1) {
    poc -= r.ue() + 1;
    out.deltaS0.push(poc);
    out.usedS0.push(r.u1());
  }
  poc = 0;
  for (let i = 0; i < numPositive; i += 1) {
    poc += r.ue() + 1;
    out.deltaS1.push(poc);
    out.usedS1.push(r.u1());
  }
  return out;
}

function parseSps(rbsp: Uint8Array): Sps {
  const r = new BitReader(rbsp);
  r.skip(16); // NAL header
  r.skip(4); // sps_video_parameter_set_id
  const maxSubLayersMinus1 = r.u(3);
  r.skip(1); // sps_temporal_id_nesting_flag
  parseProfileTierLevel(r, maxSubLayersMinus1);
  const id = r.ue();
  const chromaFormatIdc = r.ue();
  let separateColourPlane = 0;
  if (chromaFormatIdc === 3) separateColourPlane = r.u1();
  const width = r.ue();
  const height = r.ue();
  if (r.u1()) {
    r.ue();
    r.ue();
    r.ue();
    r.ue(); // conformance window
  }
  r.ue(); // bit_depth_luma_minus8
  r.ue(); // bit_depth_chroma_minus8
  const log2MaxPocLsb = r.ue() + 4;
  const subLayerOrderingInfoPresent = r.u1();
  for (
    let i = subLayerOrderingInfoPresent ? 0 : maxSubLayersMinus1;
    i <= maxSubLayersMinus1;
    i += 1
  ) {
    r.ue();
    r.ue();
    r.ue();
  }
  const log2MinCb = r.ue() + 3;
  const log2DiffMaxMinCb = r.ue();
  r.ue(); // log2_min_luma_transform_block_size_minus2
  r.ue(); // log2_diff_max_min_luma_transform_block_size
  r.ue(); // max_transform_hierarchy_depth_inter
  r.ue(); // max_transform_hierarchy_depth_intra
  if (r.u1()) {
    // scaling_list_enabled_flag
    if (r.u1()) skipScalingListData(r);
  }
  r.skip(1); // amp_enabled_flag
  const saoEnabled = r.u1() === 1;
  if (r.u1()) {
    // pcm_enabled_flag
    r.skip(4 + 4);
    r.ue();
    r.ue();
    r.skip(1);
  }
  const numStRps = r.ue();
  if (numStRps > 64) throw new HevcSpliceUnsupported("num_short_term_ref_pic_sets out of range");
  const stRps: StRps[] = [];
  for (let i = 0; i < numStRps; i += 1) stRps.push(parseStRps(r, i, stRps, numStRps));
  const longTermRefPicsPresent = r.u1() === 1;
  let numLongTermRefPicsSps = 0;
  if (longTermRefPicsPresent) {
    numLongTermRefPicsSps = r.ue();
    for (let i = 0; i < numLongTermRefPicsSps; i += 1) {
      r.skip(log2MaxPocLsb); // lt_ref_pic_poc_lsb_sps
      r.skip(1); // used_by_curr_pic_lt_sps_flag
    }
  }
  const temporalMvpEnabled = r.u1() === 1;
  // strong_intra_smoothing, VUI and extensions: nothing past this point
  // shapes the slice header.
  const ctbLog2 = log2MinCb + log2DiffMaxMinCb;
  const ctbSize = 1 << ctbLog2;
  const picSizeInCtbsY = Math.ceil(width / ctbSize) * Math.ceil(height / ctbSize);
  return {
    id,
    chromaArrayType: separateColourPlane ? 0 : chromaFormatIdc,
    separateColourPlane: separateColourPlane === 1,
    log2MaxPocLsb,
    sliceSegmentAddressBits: ceilLog2(picSizeInCtbsY),
    saoEnabled,
    stRps,
    longTermRefPicsPresent,
    numLongTermRefPicsSps,
    temporalMvpEnabled,
  };
}

function parsePps(rbsp: Uint8Array): Pps {
  const r = new BitReader(rbsp);
  r.skip(16); // NAL header
  const id = r.ue();
  const spsId = r.ue();
  const dependentSliceSegmentsEnabled = r.u1() === 1;
  const outputFlagPresent = r.u1() === 1;
  const numExtraSliceHeaderBits = r.u(3);
  r.skip(1); // sign_data_hiding_enabled_flag
  r.skip(1); // cabac_init_present_flag
  r.ue(); // num_ref_idx_l0_default_active_minus1
  r.ue(); // num_ref_idx_l1_default_active_minus1
  r.se(); // init_qp_minus26
  r.skip(1); // constrained_intra_pred_flag
  const transformSkipEnabled = r.u1();
  if (r.u1()) r.ue(); // cu_qp_delta_enabled_flag → diff_cu_qp_delta_depth
  r.se(); // pps_cb_qp_offset
  r.se(); // pps_cr_qp_offset
  const sliceChromaQpOffsetsPresent = r.u1() === 1;
  r.skip(1); // weighted_pred_flag
  r.skip(1); // weighted_bipred_flag
  r.skip(1); // transquant_bypass_enabled_flag
  const tilesEnabled = r.u1() === 1;
  const entropyCodingSyncEnabled = r.u1() === 1;
  if (tilesEnabled) {
    const numTileColumnsMinus1 = r.ue();
    const numTileRowsMinus1 = r.ue();
    const uniformSpacing = r.u1();
    if (!uniformSpacing) {
      for (let i = 0; i < numTileColumnsMinus1; i += 1) r.ue();
      for (let i = 0; i < numTileRowsMinus1; i += 1) r.ue();
    }
    r.skip(1); // loop_filter_across_tiles_enabled_flag
  }
  const loopFilterAcrossSlicesEnabled = r.u1() === 1;
  let deblockingFilterOverrideEnabled = false;
  let ppsDeblockingFilterDisabled = false;
  if (r.u1()) {
    // deblocking_filter_control_present_flag
    deblockingFilterOverrideEnabled = r.u1() === 1;
    ppsDeblockingFilterDisabled = r.u1() === 1;
    if (!ppsDeblockingFilterDisabled) {
      r.se();
      r.se();
    }
  }
  if (r.u1()) skipScalingListData(r); // pps_scaling_list_data_present_flag
  r.skip(1); // lists_modification_present_flag
  r.ue(); // log2_parallel_merge_level_minus2
  const sliceSegmentHeaderExtensionPresent = r.u1() === 1;
  let chromaQpOffsetListEnabled = false;
  if (r.u1()) {
    // pps_extension_present_flag
    const rangeExt = r.u1();
    const multilayerExt = r.u1();
    const ext3d = r.u1();
    const sccExt = r.u1();
    r.skip(4);
    if (multilayerExt || ext3d || sccExt) {
      throw new HevcSpliceUnsupported("PPS multilayer / 3D / SCC extension");
    }
    if (rangeExt) {
      if (transformSkipEnabled) r.ue(); // log2_max_transform_skip_block_size_minus2
      r.skip(1); // cross_component_prediction_enabled_flag
      chromaQpOffsetListEnabled = r.u1() === 1;
      if (chromaQpOffsetListEnabled) {
        r.ue(); // diff_cu_chroma_qp_offset_depth
        const lenMinus1 = r.ue();
        for (let i = 0; i <= lenMinus1; i += 1) {
          r.se();
          r.se();
        }
      }
      r.ue(); // log2_sao_offset_scale_luma
      r.ue(); // log2_sao_offset_scale_chroma
    }
  }
  return {
    id,
    spsId,
    dependentSliceSegmentsEnabled,
    outputFlagPresent,
    numExtraSliceHeaderBits,
    sliceChromaQpOffsetsPresent,
    deblockingFilterOverrideEnabled,
    ppsDeblockingFilterDisabled,
    loopFilterAcrossSlicesEnabled,
    tilesEnabled,
    entropyCodingSyncEnabled,
    sliceSegmentHeaderExtensionPresent,
    chromaQpOffsetListEnabled,
  };
}

/** The prefix of a slice segment header every rewrite needs: everything up
 *  to (not including) `slice_pic_order_cnt_lsb`. */
type SlicePrefix = {
  dependent: boolean;
  sliceType: number;
  /** Bit offset of `slice_pic_order_cnt_lsb` (the reader stands there). */
  pocBit: number;
  sps: Sps;
  pps: Pps;
};

function parseSlicePrefix(
  r: BitReader,
  nalType: number,
  spsMap: Map<number, Sps>,
  ppsMap: Map<number, Pps>,
): SlicePrefix {
  r.skip(16); // NAL header
  const firstSliceSegmentInPic = r.u1();
  if (nalType >= NAL.BLA_W_LP && nalType <= NAL.RSV_IRAP_VCL23) r.skip(1); // no_output_of_prior_pics_flag
  const ppsId = r.ue();
  const pps = ppsMap.get(ppsId);
  if (!pps) throw new HevcSpliceUnsupported(`slice references unknown PPS ${ppsId}`);
  const sps = spsMap.get(pps.spsId);
  if (!sps) throw new HevcSpliceUnsupported(`PPS references unknown SPS ${pps.spsId}`);
  let dependent = false;
  if (!firstSliceSegmentInPic) {
    if (pps.dependentSliceSegmentsEnabled) dependent = r.u1() === 1;
    r.skip(sps.sliceSegmentAddressBits); // slice_segment_address
  }
  if (dependent) return { dependent, sliceType: -1, pocBit: -1, sps, pps };
  r.skip(pps.numExtraSliceHeaderBits); // slice_reserved_flag[i]
  const sliceType = r.ue();
  if (pps.outputFlagPresent) r.skip(1); // pic_output_flag
  if (sps.separateColourPlane) r.skip(2); // colour_plane_id
  return { dependent, sliceType, pocBit: r.bitPosition, sps, pps };
}

/** Consume the block an IDR header omits: POC lsb, short-term RPS, long-term
 *  refs and the temporal MVP flag. The reader must stand on
 *  `slice_pic_order_cnt_lsb`. */
function skipNonIdrBlock(r: BitReader, sps: Sps): void {
  r.skip(sps.log2MaxPocLsb); // slice_pic_order_cnt_lsb
  const stRpsSpsFlag = r.u1();
  if (!stRpsSpsFlag) {
    parseStRps(r, sps.stRps.length, sps.stRps, sps.stRps.length);
  } else if (sps.stRps.length > 1) {
    r.skip(ceilLog2(sps.stRps.length)); // short_term_ref_pic_set_idx
  }
  if (sps.longTermRefPicsPresent) {
    let numLongTermSps = 0;
    if (sps.numLongTermRefPicsSps > 0) numLongTermSps = r.ue();
    const numLongTermPics = r.ue();
    for (let i = 0; i < numLongTermSps + numLongTermPics; i += 1) {
      if (i < numLongTermSps) {
        if (sps.numLongTermRefPicsSps > 1) r.skip(ceilLog2(sps.numLongTermRefPicsSps)); // lt_idx_sps
      } else {
        r.skip(sps.log2MaxPocLsb); // poc_lsb_lt
        r.skip(1); // used_by_curr_pic_lt_flag
      }
      if (r.u1()) r.ue(); // delta_poc_msb_present_flag → delta_poc_msb_cycle_lt
    }
  }
  if (sps.temporalMvpEnabled) r.skip(1); // slice_temporal_mvp_enabled_flag
}

/** Consume the rest of an I-slice header after the non-IDR block, up to (not
 *  including) `byte_alignment()`. */
function skipISliceTail(r: BitReader, sps: Sps, pps: Pps): void {
  let saoLuma = 0;
  let saoChroma = 0;
  if (sps.saoEnabled) {
    saoLuma = r.u1();
    if (sps.chromaArrayType !== 0) saoChroma = r.u1();
  }
  r.se(); // slice_qp_delta
  if (pps.sliceChromaQpOffsetsPresent) {
    r.se();
    r.se();
  }
  if (pps.chromaQpOffsetListEnabled) r.skip(1); // cu_chroma_qp_offset_enabled_flag
  let deblockingDisabled = pps.ppsDeblockingFilterDisabled;
  if (pps.deblockingFilterOverrideEnabled && r.u1()) {
    deblockingDisabled = r.u1() === 1;
    if (!deblockingDisabled) {
      r.se();
      r.se();
    }
  }
  if (pps.loopFilterAcrossSlicesEnabled && (saoLuma || saoChroma || !deblockingDisabled)) {
    r.skip(1); // slice_loop_filter_across_slices_enabled_flag
  }
  if (pps.tilesEnabled || pps.entropyCodingSyncEnabled) {
    const numEntryPointOffsets = r.ue();
    if (numEntryPointOffsets > 0) {
      const offsetLen = r.ue() + 1;
      r.skip(numEntryPointOffsets * offsetLen);
    }
  }
  if (pps.sliceSegmentHeaderExtensionPresent) {
    const len = r.ue();
    r.skip(8 * len);
  }
}

function nalType(nal: Uint8Array): number {
  return ((nal[0] as number) >> 1) & 0x3f;
}

function withNalType(nal: Uint8Array, type: number): Uint8Array {
  const out = nal.slice();
  out[0] = ((out[0] as number) & 0x81) | (type << 1);
  return out;
}

/** Parse an `hvcC` record's parameter-set arrays and NAL length size. */
export function parseHvcc(hvcc: Uint8Array): {
  nalLengthSize: number;
  sps: Map<number, Sps>;
  pps: Map<number, Pps>;
} {
  if (hvcc.length < 23) throw new HevcSpliceUnsupported("hvcC too short");
  const nalLengthSize = ((hvcc[21] as number) & 3) + 1;
  const numArrays = hvcc[22] as number;
  const sps = new Map<number, Sps>();
  const pps = new Map<number, Pps>();
  let p = 23;
  for (let a = 0; a < numArrays; a += 1) {
    const type = (hvcc[p] as number) & 0x3f;
    const count = ((hvcc[p + 1] as number) << 8) | (hvcc[p + 2] as number);
    p += 3;
    for (let i = 0; i < count; i += 1) {
      const len = ((hvcc[p] as number) << 8) | (hvcc[p + 1] as number);
      p += 2;
      const nal = hvcc.subarray(p, p + len);
      p += len;
      if (type === NAL.SPS) {
        const s = parseSps(unescapeRbsp(nal));
        sps.set(s.id, s);
      } else if (type === NAL.PPS) {
        const s = parsePps(unescapeRbsp(nal));
        pps.set(s.id, s);
      }
    }
  }
  if (sps.size === 0 || pps.size === 0) throw new HevcSpliceUnsupported("hvcC carries no SPS/PPS");
  return { nalLengthSize, sps, pps };
}

export type SplicedAccessUnit = {
  data: Uint8Array;
  /** True when this access unit now starts an IDR picture. */
  idr: boolean;
};

/**
 * One splice per coded frame group. Feed every access unit of the run, in
 * decode order, starting with the CRA the run opens on; each call returns
 * the bytes to mux in its place.
 */
export class HevcCraSplicer {
  private readonly nalLengthSize: number;
  private readonly sps: Map<number, Sps>;
  private readonly pps: Map<number, Pps>;
  /** Subtracted from every later `slice_pic_order_cnt_lsb`. */
  private pocShift = 0;
  private first = true;

  constructor(hvcc: Uint8Array) {
    const parsed = parseHvcc(hvcc);
    this.nalLengthSize = parsed.nalLengthSize;
    this.sps = parsed.sps;
    this.pps = parsed.pps;
  }

  transform(accessUnit: Uint8Array): SplicedAccessUnit {
    const nals = this.split(accessUnit);
    const out: Uint8Array[] = [];
    let idr = false;
    let changed = false;
    for (const nal of nals) {
      const type = nalType(nal);
      if (type === NAL.SPS || type === NAL.PPS) {
        // In-band parameter sets (hev1 sources) supersede the hvcC ones.
        const rbsp = unescapeRbsp(nal);
        if (type === NAL.SPS) {
          const s = parseSps(rbsp);
          this.sps.set(s.id, s);
        } else {
          const s = parsePps(rbsp);
          this.pps.set(s.id, s);
        }
        out.push(nal);
        continue;
      }
      if (type > NAL.RSV_IRAP_VCL23) {
        out.push(nal);
        continue;
      }
      if (this.first) {
        if (type === NAL.IDR_W_RADL || type === NAL.IDR_N_LP) {
          // Already what Gecko wants; nothing to shift either.
          this.pocShift = 0;
        } else if (type === NAL.CRA_NUT || (type >= NAL.BLA_W_LP && type < NAL.IDR_W_RADL)) {
          const rewritten = this.craToIdr(nal);
          out.push(rewritten.nal);
          if (rewritten.pocLsb !== null) this.pocShift = rewritten.pocLsb;
          idr = true;
          changed = true;
          continue;
        } else {
          throw new HevcSpliceUnsupported(`run starts on NAL type ${type}, not an IRAP`);
        }
      } else if (type === NAL.IDR_W_RADL || type === NAL.IDR_N_LP) {
        // A real IDR restarts the count; later lsb values are relative to it.
        this.pocShift = 0;
      } else if (this.pocShift !== 0) {
        const shifted = this.shiftPoc(nal);
        if (shifted) {
          out.push(shifted);
          changed = true;
          continue;
        }
      }
      out.push(nal);
    }
    // `first` covers the whole first picture: every slice segment of the
    // CRA is rewritten, and the shift only starts on the next picture.
    this.first = false;
    return { data: changed ? this.join(out) : accessUnit, idr };
  }

  private split(accessUnit: Uint8Array): Uint8Array[] {
    const nals: Uint8Array[] = [];
    let p = 0;
    const n = this.nalLengthSize;
    while (p + n <= accessUnit.length) {
      let len = 0;
      for (let i = 0; i < n; i += 1) len = (len << 8) | (accessUnit[p + i] as number);
      p += n;
      if (len < 2 || p + len > accessUnit.length) {
        throw new HevcSpliceUnsupported("malformed NAL length prefix");
      }
      nals.push(accessUnit.subarray(p, p + len));
      p += len;
    }
    return nals;
  }

  private join(nals: Uint8Array[]): Uint8Array {
    const n = this.nalLengthSize;
    const total = nals.reduce((sum, nal) => sum + n + nal.length, 0);
    const out = new Uint8Array(total);
    let p = 0;
    for (const nal of nals) {
      let len = nal.length;
      for (let i = n - 1; i >= 0; i -= 1) {
        out[p + i] = len & 0xff;
        len >>>= 8;
      }
      p += n;
      out.set(nal, p);
      p += nal.length;
    }
    return out;
  }

  /** Rewrite one slice segment NAL of the run-opening CRA into IDR_W_RADL. */
  private craToIdr(nal: Uint8Array): { nal: Uint8Array; pocLsb: number | null } {
    const rbsp = unescapeRbsp(nal);
    const r = new BitReader(rbsp);
    const prefix = parseSlicePrefix(r, nalType(nal), this.sps, this.pps);
    if (prefix.dependent) {
      // A dependent segment carries no POC or RPS: only the label changes.
      return { nal: withNalType(nal, NAL.IDR_W_RADL), pocLsb: null };
    }
    if (prefix.sliceType !== SLICE_I) {
      throw new HevcSpliceUnsupported(`IRAP slice of type ${prefix.sliceType}`);
    }
    const { sps, pps } = prefix;
    const blockStart = prefix.pocBit;
    const probe = new BitReader(rbsp);
    probe.skip(blockStart);
    const pocLsb = probe.u(sps.log2MaxPocLsb);
    skipNonIdrBlock(r, sps);
    const blockEnd = r.bitPosition;
    skipISliceTail(r, sps, pps);
    const tailEnd = r.bitPosition;
    // byte_alignment(): a one bit then zeros; slice data starts at the next byte.
    if (r.u1() !== 1) throw new HevcSpliceUnsupported("slice header alignment bit missing");
    const dataStart = Math.ceil(r.bitPosition / 8);

    const w = new BitWriter();
    const src = new BitReader(rbsp);
    src.skip(16);
    // NAL header, relabelled, written straight.
    w.u(((nal[0] as number) & 0x81) | (NAL.IDR_W_RADL << 1), 8);
    w.u(nal[1] as number, 8);
    w.copy(src, blockStart - 16);
    src.skip(blockEnd - blockStart);
    w.copy(src, tailEnd - blockEnd);
    w.align();
    const header = w.bytes();
    const merged = new Uint8Array(header.length + (rbsp.length - dataStart));
    merged.set(header, 0);
    merged.set(rbsp.subarray(dataStart), header.length);
    return { nal: this.escapeNal(merged), pocLsb };
  }

  /** Subtract the run's shift from `slice_pic_order_cnt_lsb`, in place in the
   *  RBSP; null when the segment has no POC field. */
  private shiftPoc(nal: Uint8Array): Uint8Array | null {
    const rbsp = unescapeRbsp(nal);
    const r = new BitReader(rbsp);
    const prefix = parseSlicePrefix(r, nalType(nal), this.sps, this.pps);
    if (prefix.dependent) return null;
    const bits = prefix.sps.log2MaxPocLsb;
    const max = 1 << bits;
    const probe = new BitReader(rbsp);
    probe.skip(prefix.pocBit);
    const lsb = probe.u(bits);
    const shifted = (((lsb - this.pocShift) % max) + max) % max;
    for (let i = 0; i < bits; i += 1) {
      const pos = prefix.pocBit + i;
      const bit = (shifted >>> (bits - 1 - i)) & 1;
      const idx = pos >> 3;
      const mask = 0x80 >> (pos & 7);
      rbsp[idx] = bit ? (rbsp[idx] as number) | mask : (rbsp[idx] as number) & ~mask;
    }
    return this.escapeNal(rbsp);
  }

  /** RBSP (with its 2-byte NAL header in front) back to a NAL unit. */
  private escapeNal(rbsp: Uint8Array): Uint8Array {
    const escaped = escapeRbsp(rbsp.subarray(2));
    const out = new Uint8Array(2 + escaped.length);
    out[0] = rbsp[0] as number;
    out[1] = rbsp[1] as number;
    out.set(escaped, 2);
    return out;
  }
}

/** Run a Mediabunny packet through the splicer; the same packet comes back
 *  when nothing had to change. */
export function splicePacket(splicer: HevcCraSplicer, packet: EncodedPacket): EncodedPacket {
  const { data, idr } = splicer.transform(packet.data);
  if (data === packet.data && (!idr || packet.type === "key")) return packet;
  return new EncodedPacket(
    data,
    idr ? "key" : packet.type,
    packet.timestamp,
    packet.duration,
    packet.sequenceNumber,
    undefined,
    packet.sideData,
  );
}

/** `VideoDecoderConfig.description` as bytes, whatever buffer shape it came in. */
export function descriptionBytes(description: AllowSharedBufferSource | undefined): Uint8Array {
  if (!description) throw new HevcSpliceUnsupported("decoder config carries no hvcC");
  if (description instanceof Uint8Array) return description;
  if (ArrayBuffer.isView(description)) {
    return new Uint8Array(description.buffer, description.byteOffset, description.byteLength);
  }
  return new Uint8Array(description);
}
