import { describe, expect, it } from 'vitest';
import { nodesToText, parseBBCode, safeHref, stripSeparators } from './bbcode.ts';
import { textLang } from './language.ts';

describe('parseBBCode', () => {
	it('nests, keeps an argument, and drops extra attributes', () => {
		expect(parseBBCode('[center][b]Hi[/b] [img scale=30%]https://x.test/a.png[/img][/center]')).toEqual([
			{
				type: 'tag',
				name: 'center',
				arg: null,
				children: [
					{ type: 'tag', name: 'b', arg: null, children: [{ type: 'text', value: 'Hi' }] },
					{ type: 'text', value: ' ' },
					{ type: 'tag', name: 'img', arg: null, children: [{ type: 'text', value: 'https://x.test/a.png' }] }
				]
			}
		]);
		expect(parseBBCode('[color=#3d85c6]x[/color]')[0]).toMatchObject({ name: 'color', arg: '#3d85c6' });
	});

	it('never loses text: a stray close stays, an unclosed open stays as text, a close ends what it holds', () => {
		expect(nodesToText(parseBBCode('a [/colorr] b'))).toBe('a [/colorr] b');
		expect(nodesToText(parseBBCode('[b]bold'))).toBe('[b]bold');
		expect(parseBBCode('[size=2][b]x[/size]')[0]).toMatchObject({ name: 'size', children: [{ name: 'b' }] });
	});

	it('links only to schemes that cannot run script', () => {
		expect(safeHref('javascript:alert(1)')).toBeNull();
		expect(safeHref(' https://ok.test ')).toBe('https://ok.test');
		expect(safeHref('magnet:?xt=urn')).toBe('magnet:?xt=urn');
	});

	it('drops lines drawn with separator glyphs', () => {
		expect(stripSeparators('Title\n━━━━━━━━━━\nBody')).toBe('Title\nBody');
	});
});

describe('textLang', () => {
	it('French when its common words outnumber the English ones', () => {
		expect(textLang('Voici la description de la release, avec les sous-titres et le film.')).toBe('fr');
		expect(textLang('This is the release, with the subtitles and the movie.')).toBeUndefined();
		expect(textLang('1080p HEVC')).toBeUndefined();
	});
});
