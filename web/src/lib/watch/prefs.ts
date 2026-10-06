// The audio and subtitle LANGUAGE the person prefers, carried to the next episode and device:
// the series' own choice when the file belongs to one, else the account-wide default. Each pick
// sends the whole current state (the endpoint replaces it), then the cache reads it again, so
// the next episode starts in the language just picked.

import { me, type PlaybackPrefs } from '@iris/api/client';
import type { Manifest } from '@iris/core/manifest-client';
import { playbackPrefsSaved } from '#lib/queries.ts';

type Save = typeof me.savePlaybackPreferences;

/** Said in the audio/subtitles panel. */
export function keptForText(collectionId: string | null | undefined): string {
	return collectionId ? 'Kept for the whole series' : 'Kept as your default';
}

export class PlaybackChoices {
	#prefs: { audio_language: string | null; subtitle_language: string | null } = { audio_language: null, subtitle_language: null };

	constructor(
		readonly collectionId: string | null,
		readonly save: Save = (b) => me.savePlaybackPreferences(b),
		readonly saved: (collectionId: string | null) => unknown = playbackPrefsSaved
	) {}

	/** The preferences as read from the server. */
	adopt(p: PlaybackPrefs | undefined) {
		if (p) this.#prefs = { audio_language: p.audio_language ?? null, subtitle_language: p.subtitle_language ?? null };
	}

	async #send() {
		await this.save({ ...this.#prefs, ...(this.collectionId ? { collection_id: this.collectionId } : {}) });
		void this.saved(this.collectionId);
	}

	/** An audio track picked: its language kept (a track without a tag changes nothing). */
	audioPicked(manifest: Manifest, index: number): Promise<void> | null {
		const lang = manifest.audio[index]?.lang;
		if (!lang) return null;
		this.#prefs = { ...this.#prefs, audio_language: lang };
		return this.#send();
	}

	/** A subtitle picked (`null`: off, kept as "off"). */
	subtitlePicked(manifest: Manifest, streamIdx: number | null): Promise<void> | null {
		const lang = streamIdx === null ? 'off' : (manifest.subtitles.find((s) => s.stream_idx === streamIdx)?.lang ?? null);
		if (!lang) return null;
		this.#prefs = { ...this.#prefs, subtitle_language: lang };
		return this.#send();
	}
}
