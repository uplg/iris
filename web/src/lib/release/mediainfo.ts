// A release's technical sheet (the tracker's parsed MediaInfo) said as facts: one line for the
// picture, one for the voices, one for the subtitles.

import type { AudioInfo, MediaInfoSummary, SubInfo } from '@iris/api/client';
import { duration } from '@iris/api/format';

const CHANNELS: Record<number, string> = { 1: 'mono', 2: 'stereo', 6: '5.1', 8: '7.1' };

export function videoWords(mi: MediaInfoSummary | null | undefined): string | null {
	const v = mi?.video;
	if (!v) return null;
	const parts = [
		v.codec,
		v.resolution,
		v.hdr,
		typeof v.fps === 'number' ? `${Math.round(v.fps * 100) / 100} fps` : null,
		v.duration_secs ? duration(v.duration_secs) : null
	];
	return parts.filter(Boolean).join(' · ') || null;
}

function track(t: AudioInfo): string {
	const extra = [t.commercial_name ?? t.codec, t.channels ? (CHANNELS[t.channels] ?? `${t.channels} channels`) : null].filter(Boolean);
	return `${t.lang ?? 'Unknown language'}${extra.length ? ` (${extra.join(', ')})` : ''}`;
}

export function audioWords(mi: MediaInfoSummary | null | undefined): string | null {
	const audio = mi?.audio ?? [];
	return audio.length ? audio.map(track).join(', ') : null;
}

function sub(s: SubInfo): string {
	const flags = [s.format, s.forced ? 'forced' : null, s.title?.toLowerCase().includes('sdh') ? 'for the deaf and hard of hearing' : null];
	const extra = flags.filter(Boolean);
	return `${s.lang ?? 'Unknown language'}${extra.length ? ` (${extra.join(', ')})` : ''}`;
}

export function subtitleWords(mi: MediaInfoSummary | null | undefined): string | null {
	const subs = mi?.subtitles ?? [];
	return subs.length ? subs.map(sub).join(', ') : null;
}
