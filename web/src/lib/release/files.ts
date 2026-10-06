// The files of a torrent, in the order one picks from: videos first, episodes in order, and the
// file a grab plays when nobody chose (episode 1 of a pack, else the biggest video).

import type { FilePreview } from '@iris/api/client';
import { sceneMark } from '#lib/search/release.ts';

const episodeOf = (path: string) => {
	const m = sceneMark(path);
	return m && m.episode > 0 ? m : null;
};

/** SCENE samples are videos never to play by default (the backend's `is_main_video_file`). */
export function isSample(path: string): boolean {
	const p = path.toLowerCase();
	return p.includes('/sample/') || p.includes('.sample.') || /\bsample\b/.test(p);
}

export function sortFiles(files: readonly FilePreview[]): FilePreview[] {
	return files.toSorted((a, b) => {
		if (a.is_video !== b.is_video) return a.is_video ? -1 : 1;
		const sa = episodeOf(a.path);
		const sb = episodeOf(b.path);
		if (sa && sb) return sa.season - sb.season || sa.episode - sb.episode;
		if (sa) return -1;
		if (sb) return 1;
		return b.size_bytes - a.size_bytes;
	});
}

export function autoFile(files: readonly FilePreview[]): number | null {
	const videos = files.filter((f) => f.is_video && !isSample(f.path));
	const pool = videos.length ? videos : files;
	if (!pool.length) return null;
	const episodes = pool.filter((f) => episodeOf(f.path));
	if (episodes.length) return sortFiles(episodes)[0].index;
	return pool.reduce((best, f) => (f.size_bytes > best.size_bytes ? f : best)).index;
}

/** The main action's words for the chosen file: which episode a pack starts with. */
export function playWords(files: readonly FilePreview[], idx: number | null): string {
	const file = files.find((f) => f.index === idx);
	const ep = file ? episodeOf(file.path) : null;
	const videos = files.filter((f) => f.is_video && !isSample(f.path)).length;
	if (ep && videos > 1) return `Download and play episode ${ep.episode}`;
	return 'Download and play';
}
