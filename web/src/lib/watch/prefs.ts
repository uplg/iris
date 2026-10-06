// The audio and subtitle LANGUAGE the person prefers, carried to the next episode and device:
// the title's own choice when the file belongs to one, else the account-wide default. Under a
// title, a pick saves only what was chosen for that title (a field it never chose goes as null
// and keeps inheriting the account's); account-wide, the whole state. The cache then reads it
// again, so the next episode starts in the language just picked.

import { me, type MediaKind, type PlaybackPrefs } from '@iris/api/client';
import { thisTitle } from '@iris/api/format';
import type { Manifest } from '@iris/core/manifest-client';
import { playbackPrefsSaved } from '#lib/queries.ts';
import { OFF } from '#lib/language.ts';

type Save = typeof me.savePlaybackPreferences;
type Langs = { audio_language: string | null; subtitle_language: string | null };

/** Said in the audio/subtitles panel. */
export function keptForText(collectionId: string | null | undefined, kind: MediaKind | null | undefined): string {
	if (!collectionId) return 'Kept as your default';
	return kind === 'movie' ? `Kept for ${thisTitle(kind)}` : 'Kept for the whole series';
}

/** What a title chose itself (null: inherits the account's). Account-wide, everything. */
export function ownChoices(p: PlaybackPrefs | undefined, collectionId: string | null): Langs {
	if (!p) return { audio_language: null, subtitle_language: null };
	if (!collectionId) return { audio_language: p.audio_language ?? null, subtitle_language: p.subtitle_language ?? null };
	return {
		audio_language: p.audio_for_collection ? (p.audio_language ?? null) : null,
		subtitle_language: p.subtitle_for_collection ? (p.subtitle_language ?? null) : null
	};
}

export class PlaybackChoices {
	#own: Langs = { audio_language: null, subtitle_language: null };

	constructor(
		readonly collectionId: string | null,
		readonly save: Save = (b) => me.savePlaybackPreferences(b),
		readonly saved: (collectionId: string | null) => unknown = playbackPrefsSaved
	) {}

	/** The preferences as read from the server. */
	adopt(p: PlaybackPrefs | undefined) {
		if (p) this.#own = ownChoices(p, this.collectionId);
	}

	async #send() {
		await this.save({ ...this.#own, ...(this.collectionId ? { collection_id: this.collectionId } : {}) });
		void this.saved(this.collectionId);
	}

	/** An audio track picked: its language kept (a track without a tag changes nothing). */
	audioPicked(manifest: Manifest, index: number): Promise<void> | null {
		const lang = manifest.audio[index]?.lang;
		if (!lang) return null;
		this.#own = { ...this.#own, audio_language: lang };
		return this.#send();
	}

	/** A subtitle picked (`null`: off, kept as "off"). */
	subtitlePicked(manifest: Manifest, streamIdx: number | null): Promise<void> | null {
		const lang = streamIdx === null ? OFF : (manifest.subtitles.find((s) => s.stream_idx === streamIdx)?.lang ?? null);
		if (!lang) return null;
		this.#own = { ...this.#own, subtitle_language: lang };
		return this.#send();
	}
}
