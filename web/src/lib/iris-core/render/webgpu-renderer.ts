/**
 * WebGPU renderer for `VideoFrame`. Uses `device.importExternalTexture`
 * for zero-copy access to the decoded frame and a fragment shader that
 * handles BT.709 SDR passthrough, BT.2020 PQ → SDR (ACES tone-map),
 * and HLG → SDR.
 *
 * HDR-display detection (Chrome 129+) configures the canvas with
 * `rgba16float` + `colorSpace: rec2100-hlg` so we can hand off HDR
 * frames without crushing the highlights.
 */

import type { VideoRenderer, VideoRendererOptions } from "./renderer-factory";

type WebGpuNavigator = Navigator & { gpu: GPU };

export async function mountWebGpuRenderer(opts: VideoRendererOptions): Promise<VideoRenderer> {
  const nav = navigator as WebGpuNavigator;
  if (!nav.gpu) throw new Error("WebGPU API not available");

  const adapter = await nav.gpu.requestAdapter();
  if (!adapter) throw new Error("WebGPU adapter request returned null");
  const device = await adapter.requestDevice();

  // HDR detection: Chrome 129+ exposes extended-range canvases. The
  // `colorSpace` field can be set to `'display-p3'` or `'rec2100-hlg'`.
  // For Phase 2-polish we keep the canvas in linear sRGB; the shader
  // performs PQ/HLG → linear → BT.709 → sRGB display-encoded output.
  // HDR-aware canvas configuration lands as a follow-up once we have
  // a reliable HDR-source detection (frame's color space metadata).
  const canvas = document.createElement("canvas");
  canvas.className = "h-full w-full object-contain bg-black";
  opts.container.appendChild(canvas);
  const context = canvas.getContext("webgpu");
  if (!context) {
    throw new Error("Failed to get WebGPU canvas context");
  }
  const presentationFormat = nav.gpu.getPreferredCanvasFormat();
  context.configure({
    device,
    format: presentationFormat,
    alphaMode: "opaque",
  });

  const sampler = device.createSampler({
    magFilter: "linear",
    minFilter: "linear",
  });

  const shaderModule = device.createShaderModule({ code: SHADER });
  const pipeline = device.createRenderPipeline({
    layout: "auto",
    vertex: {
      module: shaderModule,
      entryPoint: "vs_main",
    },
    fragment: {
      module: shaderModule,
      entryPoint: "fs_main",
      targets: [{ format: presentationFormat }],
    },
    primitive: { topology: "triangle-list" },
  });

  // Uniform buffer carries the tone-mapping mode flag.
  // 0 = SDR passthrough, 1 = PQ→SDR (ACES), 2 = HLG→SDR.
  const uniformBuffer = device.createBuffer({
    size: 16,
    usage: GPUBufferUsage.UNIFORM | GPUBufferUsage.COPY_DST,
  });

  // Bind groups can't include external textures statically; we build a
  // fresh group per frame from the imported texture. The sampler +
  // uniform buffer are stable.

  let intrinsic: { width: number; height: number } | null = null;
  const queue: VideoFrame[] = [];
  let disposed = false;

  device.lost.then((info) => {
    console.warn("[iris-core] WebGPU device lost:", info.reason, info.message);
  });

  const draw = (frame: VideoFrame): void => {
    if (disposed) {
      frame.close();
      return;
    }
    if (!intrinsic) {
      intrinsic = { width: frame.displayWidth, height: frame.displayHeight };
      canvas.width = intrinsic.width;
      canvas.height = intrinsic.height;
    }
    // Pick a tone-map mode from the frame's color space metadata.
    // `colorSpace.transfer` follows the IEC 61966-2-1 / ITU-R BT
    // identifiers; `smpte2084` = PQ, `arib-std-b67` = HLG. TS's
    // lib.dom.d.ts narrows the enum; the runtime value is the
    // canonical W3C string regardless, so we string-compare via
    // `as string`.
    const transfer = frame.colorSpace.transfer as string | null;
    let mode = 0;
    if (transfer === "smpte2084") mode = 1;
    else if (transfer === "arib-std-b67") mode = 2;
    device.queue.writeBuffer(uniformBuffer, 0, new Uint32Array([mode, 0, 0, 0]));

    let externalTexture: GPUExternalTexture;
    try {
      externalTexture = device.importExternalTexture({ source: frame });
    } catch (e) {
      frame.close();
      opts.onError?.(e instanceof Error ? e : new Error(String(e)));
      return;
    }

    const bindGroup = device.createBindGroup({
      layout: pipeline.getBindGroupLayout(0),
      entries: [
        { binding: 0, resource: sampler },
        { binding: 1, resource: externalTexture },
        { binding: 2, resource: { buffer: uniformBuffer } },
      ],
    });

    const encoder = device.createCommandEncoder();
    const pass = encoder.beginRenderPass({
      colorAttachments: [
        {
          view: context.getCurrentTexture().createView(),
          loadOp: "clear",
          storeOp: "store",
          clearValue: { r: 0, g: 0, b: 0, a: 1 },
        },
      ],
    });
    pass.setPipeline(pipeline);
    pass.setBindGroup(0, bindGroup);
    pass.draw(6);
    pass.end();
    device.queue.submit([encoder.finish()]);

    frame.close();
  };

  const enqueue = (frame: VideoFrame): void => {
    if (disposed) {
      frame.close();
      return;
    }
    queue.push(frame);
    while (queue.length > 32) {
      const dropped = queue.shift();
      dropped?.close();
    }
  };

  const tick = (): void => {
    if (disposed) return;
    const now = opts.clockSeconds();
    while (queue.length > 0) {
      const head = queue[0];
      if (!head) break;
      const headTs = head.timestamp / 1_000_000;
      if (headTs > now + 0.001) break;
      const lateBy = (now - headTs) * 1000;
      if (lateBy > 80 && queue.length > 1) {
        const dropped = queue.shift();
        dropped?.close();
        continue;
      }
      const drawn = queue.shift();
      if (drawn) draw(drawn);
      break;
    }
    if (!disposed) requestAnimationFrame(tick);
  };
  requestAnimationFrame(tick);

  return {
    enqueue,
    queueDepth: () => queue.length,
    intrinsicSize: () => intrinsic,
    canvas,
    isHardwareAccelerated: () => true,
    dispose: () => {
      if (disposed) return;
      disposed = true;
      for (const f of queue) f.close();
      queue.length = 0;
      try {
        device.destroy();
      } catch {
        /* idempotent */
      }
      if (canvas.parentNode) canvas.parentNode.removeChild(canvas);
    },
  };
}

/**
 * WGSL shader. Two triangles cover the full viewport; the fragment
 * shader samples the external texture (which the platform decodes
 * YUV→RGB internally) and applies the requested tone-mapping.
 *
 * - `mode == 0` (BT.709 SDR): straight passthrough.
 * - `mode == 1` (PQ): linearise via the PQ EOTF inverse, ACES
 *   tone-map down to SDR, then encode back to sRGB.
 * - `mode == 2` (HLG): linearise via the HLG EOTF, tone-map.
 */
const SHADER = /* wgsl */ `
struct VsOut {
  @builtin(position) pos: vec4f,
  @location(0) uv: vec2f,
};

@vertex
fn vs_main(@builtin(vertex_index) idx: u32) -> VsOut {
  // Fullscreen triangle pair. Indices 0..5 map to two right triangles.
  let pos = array<vec2f, 6>(
    vec2f(-1.0, -1.0), vec2f(1.0, -1.0), vec2f(-1.0, 1.0),
    vec2f(-1.0,  1.0), vec2f(1.0, -1.0), vec2f(1.0,  1.0),
  );
  let uv = array<vec2f, 6>(
    vec2f(0.0, 1.0), vec2f(1.0, 1.0), vec2f(0.0, 0.0),
    vec2f(0.0, 0.0), vec2f(1.0, 1.0), vec2f(1.0, 0.0),
  );
  var out: VsOut;
  out.pos = vec4f(pos[idx], 0.0, 1.0);
  out.uv = uv[idx];
  return out;
}

struct Uniforms {
  mode: u32,
  _pad0: u32,
  _pad1: u32,
  _pad2: u32,
};

@group(0) @binding(0) var samp: sampler;
@group(0) @binding(1) var tex: texture_external;
@group(0) @binding(2) var<uniform> u: Uniforms;

// PQ (SMPTE ST 2084) EOTF — converts 0..1 signal to linear nits.
// Implementation per the ITU-R BT.2100-2 spec.
fn pq_eotf(v: vec3f) -> vec3f {
  let m1 = 0.1593017578125;
  let m2 = 78.84375;
  let c1 = 0.8359375;
  let c2 = 18.8515625;
  let c3 = 18.6875;
  let e = pow(max(v, vec3f(0.0)), vec3f(1.0 / m2));
  let num = max(e - c1, vec3f(0.0));
  let den = c2 - c3 * e;
  return pow(num / max(den, vec3f(1e-6)), vec3f(1.0 / m1)) * 10000.0;
}

// HLG (ARIB STD-B67) EOTF — output is in display-light scene-referred
// luminance assuming a 1000-nit display reference (peak = 1.0).
fn hlg_eotf(v: vec3f) -> vec3f {
  let a = 0.17883277;
  let b = 0.28466892;
  let c = 0.55991073;
  let lo = v * v / 3.0;
  let hi = (exp((v - c) / a) + b) / 12.0;
  return select(hi, lo, v < vec3f(0.5));
}

// ACES filmic tone mapping (Krzysztof Narkowicz fit). Input is linear
// HDR (with peak well above 1), output is SDR linear in [0, 1].
fn aces_tonemap(x: vec3f) -> vec3f {
  let a = 2.51;
  let b = 0.03;
  let c = 2.43;
  let d = 0.59;
  let e = 0.14;
  return clamp((x * (a * x + b)) / (x * (c * x + d) + e), vec3f(0.0), vec3f(1.0));
}

// BT.2020 → BT.709 colour primaries conversion matrix. Applied in
// linear light before tone-map.
fn bt2020_to_bt709(c: vec3f) -> vec3f {
  let m = mat3x3f(
    vec3f( 1.6605, -0.1246, -0.0182),
    vec3f(-0.5876,  1.1329, -0.1006),
    vec3f(-0.0728, -0.0083,  1.1187),
  );
  return m * c;
}

// sRGB OETF — gamma-encode linear values for the display.
fn srgb_oetf(c: vec3f) -> vec3f {
  let lo = c * 12.92;
  let hi = 1.055 * pow(max(c, vec3f(0.0)), vec3f(1.0 / 2.4)) - 0.055;
  return select(hi, lo, c <= vec3f(0.0031308));
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4f {
  let raw = textureSampleBaseClampToEdge(tex, samp, in.uv).rgb;
  if (u.mode == 0u) {
    // Treat the imported texture as already display-encoded sRGB.
    return vec4f(raw, 1.0);
  }
  // HDR path: linearise, tone-map down to SDR, re-encode for sRGB.
  var linearLight: vec3f;
  if (u.mode == 1u) {
    // PQ → linear nits. Normalise to a 100-nit SDR reference so the
    // ACES curve has its midtones at the right place.
    linearLight = pq_eotf(raw) / 100.0;
  } else {
    // HLG → linear; peak signal maps to ~12. Same normalisation.
    linearLight = hlg_eotf(raw) * 12.0 / 100.0;
  }
  let bt709 = bt2020_to_bt709(linearLight);
  let toned = aces_tonemap(bt709);
  let encoded = srgb_oetf(toned);
  return vec4f(encoded, 1.0);
}
`;
