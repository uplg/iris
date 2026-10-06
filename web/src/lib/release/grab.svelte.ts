// Grabbing a release to play it, from a result row or its page: the guards the server would
// otherwise hit later, said before anything is downloaded. A RAR-only release cannot be
// streamed; a release over 50 GB asks once more; a movie already in the library asks before a
// second copy (the server's `409 duplicate_in_library`). Then the player opens on the file.

import { ApiError, torrents, type TorrentPreview } from '@iris/api/client';
import { Gesture } from '#lib/gesture.svelte.ts';
import { autoFile } from './files.ts';
import { watchHref } from '#lib/paths.ts';
import { queryClient } from '#lib/query.ts';
import { KEYS, refreshLibrary } from '#lib/queries.ts';

/** Above this a grab asks twice: complete-series packs fill the shared disk, and everyone's
 * library is cleaned up sooner. */
export const HUGE_GRAB_BYTES = 50 * 1024 ** 3;

export type Need = { kind: 'archive' } | { kind: 'huge'; bytes: number } | { kind: 'duplicate'; message: string };

export interface GrabTarget {
	provider: string;
	id: string;
	tmdbId?: number | null;
	/** Already read (the release page): not asked again. */
	preview?: TorrentPreview | null;
	/** The file chosen; without one, the grab's own pick. */
	fileIdx?: number | null;
}

export interface Consent {
	huge?: boolean;
	duplicate?: boolean;
}

export class Grab {
	g = new Gesture();
	/** What the person must read (or agree to) before the grab goes on. */
	need = $state<Need | null>(null);
	#consent: Consent = {};
	#go: (href: string) => unknown;

	constructor(go: (href: string) => unknown) {
		this.#go = go;
	}

	get busy() {
		return this.g.is('grab');
	}

	/** Starts (or, with a consent, goes on with) the grab of `t`. */
	run(t: GrabTarget, consent: Consent = {}) {
		this.#consent = { ...this.#consent, ...consent };
		const given = this.#consent;
		return this.g.run(
			async () => {
				const preview = t.preview ?? (await torrents.preview(t.provider, t.id));
				const idx = t.fileIdx ?? autoFile(preview.files);
				if (!preview.streamable || idx === null) {
					this.need = { kind: 'archive' };
					return null;
				}
				if (preview.total_size_bytes > HUGE_GRAB_BYTES && !given.huge) {
					this.need = { kind: 'huge', bytes: preview.total_size_bytes };
					return null;
				}
				const res = await torrents.ingest(t.provider, t.id, t.tmdbId ?? null, given.duplicate ?? false);
				// in the library now: what lists it, and the results that said « not in library »,
				// are read again when next shown
				void refreshLibrary();
				void queryClient.invalidateQueries({ queryKey: KEYS.search, refetchType: 'none' });
				return watchHref(res.snapshot.infohash, idx);
			},
			(href) => {
				if (!href) return;
				this.need = null;
				return this.#go(href);
			},
			'grab',
			{
				refused: (e) => {
					if (!(e instanceof ApiError && e.code === 'duplicate_in_library')) return false;
					this.need = { kind: 'duplicate', message: e.message };
					return true;
				}
			}
		);
	}

	/** Leaves the grab where it was: nothing downloaded, the consent given so far forgotten. */
	cancel() {
		this.need = null;
		this.#consent = {};
	}
}
