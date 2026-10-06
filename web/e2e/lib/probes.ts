// In-page probes shared by the Playwright and the Zen (puppeteer) specs: plain functions handed
// to `page.evaluate`, so they carry no imports.

/** hevc.js's H.264 encode as its worker runs it (a real encode in a worker: `isConfigSupported`
 *  alone says yes on builds that then refuse to configure), with the latency mode our patch
 *  picks: quality on Gecko, which refuses realtime, realtime elsewhere. */
export function probeHevcjsEncoder(): Promise<boolean> {
	return new Promise<boolean>((resolve) => {
		const src = `
			const latencyMode = /Firefox\\//.test(navigator.userAgent) ? 'quality' : 'realtime';
			const enc = new VideoEncoder({ output: () => { postMessage(true); }, error: () => postMessage(false) });
			try {
				enc.configure({ codec: 'avc1.640028', width: 1280, height: 720, bitrate: 2e6, framerate: 24,
					hardwareAcceleration: 'no-preference', latencyMode, avc: { format: 'avc' } });
				const f = new VideoFrame(new Uint8Array(1280 * 720 * 1.5), { format: 'I420', codedWidth: 1280, codedHeight: 720, timestamp: 0 });
				enc.encode(f, { keyFrame: true }); f.close(); enc.flush().catch(() => postMessage(false));
			} catch { postMessage(false); }`;
		const w = new Worker(URL.createObjectURL(new Blob([src], { type: 'text/javascript' })));
		w.onmessage = (e) => resolve(e.data === true);
		w.onerror = () => resolve(false);
	});
}
