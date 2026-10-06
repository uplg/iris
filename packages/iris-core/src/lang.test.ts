import { describe, expect, it } from 'vitest';
import { normalizeLang } from './lang';

describe('normalizeLang', () => {
	it('folds ISO 639-2 bibliographic and terminologic codes onto 639-1', () => {
		expect(normalizeLang('fre')).toBe('fr');
		expect(normalizeLang('fra')).toBe('fr');
		expect(normalizeLang('ger')).toBe('de');
		expect(normalizeLang('deu')).toBe('de');
		expect(normalizeLang('rus')).toBe('ru');
	});

	it('strips a region, any case and spacing', () => {
		expect(normalizeLang(' FR-ca ')).toBe('fr');
		expect(normalizeLang('pt_BR')).toBe('pt');
		expect(normalizeLang('en')).toBe('en');
	});

	it('reads absent and undetermined tags as none', () => {
		expect(normalizeLang(null)).toBeNull();
		expect(normalizeLang('')).toBeNull();
		expect(normalizeLang('und')).toBeNull();
	});

	it('keeps an unknown code as it is', () => {
		expect(normalizeLang('xyz')).toBe('xyz');
	});
});
