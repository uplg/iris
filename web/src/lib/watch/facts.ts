// The quiet facts line in the player's top bar, in words: where the bytes come from, what the
// picture is, how the browser plays it (« Playing from disk · 1080p HEVC · direct »).

import type { TorrentView } from '@iris/api/client';
import { percent } from '@iris/api/format';
import type { DecodeTier, Manifest, VideoTrack } from '@iris/core/manifest-client';
import { isComplete } from '#lib/torrent.ts';

const CODECS: [RegExp, string][] = [
	[/hevc|hev1|hvc1|h265|x265/i, 'HEVC'],
	[/h264|avc|x264/i, 'H.264'],
	[/av1|av01/i, 'AV1'],
	[/vp9|vp09/i, 'VP9'],
	[/vp8/i, 'VP8'],
	[/mpeg2/i, 'MPEG-2'],
	[/mpeg4|xvid|divx/i, 'MPEG-4']
];

const HDR: Record<string, string> = { hdr10: 'HDR10', hdr10_plus: 'HDR10+', dovi: 'Dolby Vision', hlg: 'HLG' };

export function pictureWords(v: VideoTrack | undefined): string | null {
	if (!v) return null;
	const codec = CODECS.find(([re]) => re.test(v.codec))?.[1] ?? v.codec.toUpperCase();
	const height = v.height ? `${v.height}p ` : '';
	const hdr = HDR[v.hdr] ? ` ${HDR[v.hdr]}` : '';
	return `${height}${codec}${hdr}`;
}

/** How the browser plays it, per tier. */
export const TIER_WORDS: Record<DecodeTier, string> = {
	A: 'direct',
	B: 'repackaged in the browser',
	C: 'decoded in the browser',
	D: 'decoded in the browser',
	E: 'converted in the browser',
	F: 'remuxed on the server'
};

export function factsLine(t: TorrentView | undefined, manifest: Manifest | undefined, tier: DecodeTier | null): string {
	const parts: string[] = [];
	if (t)
		parts.push(isComplete(t) ? 'Playing from disk' : `Playing while it downloads, ${percent(Math.min(100, Math.max(0, t.progress_pct)))}`);
	const picture = pictureWords(manifest?.video[0]);
	if (picture) parts.push(picture);
	if (tier) parts.push(TIER_WORDS[tier]);
	return parts.join(' · ');
}
