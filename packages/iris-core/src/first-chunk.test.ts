import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

// The watch and live pages import these before a tier is picked: a value import of mediabunny
// (~360 KB) from any of them lands it in the page's first chunk, Tier A and F included.
const PICK_TIER_PATH = ['manifest-client.ts', 'caps.ts', 'codec.ts', 'stream-fetch.ts', 'decode/libav-codecs.ts'];

describe('modules read before a tier is picked', () => {
	it.each(PICK_TIER_PATH)('%s imports mediabunny for its types only', (file) => {
		const source = readFileSync(new URL(file, import.meta.url), 'utf8');
		const valueImports = source
			.split('\n')
			.filter((l) => /^import (?!type ).*from '(mediabunny|\.\.?\/.*(libav-audio-decoder|webcodecs-probe|stream-source))'/.test(l));
		expect(valueImports).toEqual([]);
	});
});
