import { describe, expect, it } from 'vitest';
import { isScriptResponse } from './libav-audio-decoder.ts';

const res = (status: number, type: string | null) => new Response('', { status, headers: type ? { 'content-type': type } : {} });

describe('the libav.js variant probe', () => {
	it('takes a served module', () => {
		expect(isScriptResponse(res(200, 'text/javascript'))).toBe(true);
		expect(isScriptResponse(res(200, 'application/javascript; charset=utf-8'))).toBe(true);
	});

	it('is not fooled by an SPA fallback or a 404', () => {
		expect(isScriptResponse(res(200, 'text/html; charset=utf-8'))).toBe(false);
		expect(isScriptResponse(res(200, null))).toBe(false);
		expect(isScriptResponse(res(404, 'text/javascript'))).toBe(false);
	});
});
