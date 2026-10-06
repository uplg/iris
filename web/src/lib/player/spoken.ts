// Times the way a screen reader should say them: « 32 minutes 10 seconds of 55 minutes ».

import { plural } from '@iris/api/format';

/** A position or a length in words, to the second. */
export function spokenTime(sec: number): string {
	const total = Math.max(0, Math.floor(Number.isFinite(sec) ? sec : 0));
	const h = Math.floor(total / 3600);
	const m = Math.floor((total % 3600) / 60);
	const s = total % 60;
	const parts: string[] = [];
	if (h) parts.push(plural(h, 'hour'));
	if (m) parts.push(plural(m, 'minute'));
	if (s || parts.length === 0) parts.push(plural(s, 'second'));
	return parts.join(' ');
}

/** The scrub bar's value text. */
export function positionText(current: number, total: number | null): string {
	return total && total > 0 ? `${spokenTime(current)} of ${spokenTime(total)}` : spokenTime(current);
}

/** A volume in words: « 40 percent », « Muted ». */
export function volumeText(volume: number, muted: boolean): string {
	return muted ? 'Muted' : `${Math.round(volume * 100)} percent`;
}
