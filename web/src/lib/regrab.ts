// A reclaimed release fetched again, the one way (history, a title's page, the player): from
// the provenance the server recorded, so the same release comes back with the same files and
// the saved positions apply again. What lists releases is read again. A refusal by the server
// (the tracker turned off, no seeders, the tracker lost the release) lands the person on Search
// with the title already asked and the refusal in words, instead of on a dead end.

import { goto } from '$app/navigation';
import { ApiError, torrents, type IngestResponse } from '@iris/api/client';
import { prettySceneName } from '@iris/api/format';
import { isNoSeeders } from '@iris/api/refusals';
import { errorText } from '#lib/errors.ts';
import { refreshLibrary } from '#lib/queries.ts';
import { searchHref } from '#lib/search/params.ts';
import { ui } from '#lib/ui.svelte.ts';

export async function fetchAgain(infohash: string) {
	const res = await torrents.regrab(infohash);
	void refreshLibrary();
	return res;
}

/** What to search when a release cannot come back: the series and the episode, else the
 * title, else the cleaned-up release name (its SCENE name would find the same corpse). */
export function retrySearchQuery(
	title: string | null | undefined,
	current: { season: number; episode: number } | null | undefined,
	releaseName: string | null | undefined
): string {
	const pad = (n: number) => String(n).padStart(2, '0');
	if (title && current) return `${title} S${pad(current.season)}E${pad(current.episode)}`;
	if (title) return title;
	return prettySceneName(releaseName ?? '');
}

export interface Reclaimed {
	infohash: string;
	/** The collection's title, when the release has one. */
	title?: string | null;
	episode?: { season: number; episode: number } | null;
	/** The release's own name, the query when there is no title. */
	name?: string | null;
}

/** The server said no (a 4xx); not a session or version gate, which the app answers itself. */
const isRefusal = (e: unknown): e is ApiError =>
	e instanceof ApiError && e.status >= 400 && e.status < 500 && e.status !== 401 && e.status !== 426;

function refusalWords(e: ApiError): string {
	if (e.code === 'provider_off') return 'This tracker is turned off in Admin.';
	if (isNoSeeders(e)) return 'Nobody shares this release any more.';
	// the tracker no longer knows the release (`provider: …` is its own words, not for people)
	if (e.status === 404 || /^provider:/i.test(e.message)) return 'Its tracker no longer has this release.';
	return errorText(e);
}

/**
 * Fetches the release again; the answer, or undefined when the server refused it and the
 * person was taken to Search instead (other releases of the same title, the refusal said).
 * Anything else (no answer at all, a server failure) is thrown, for the gesture to say.
 */
export async function fetchAgainOrSearch(r: Reclaimed): Promise<IngestResponse | undefined> {
	try {
		return await fetchAgain(r.infohash);
	} catch (e) {
		if (!isRefusal(e)) throw e;
		const q = retrySearchQuery(r.title, r.episode, r.name);
		await goto(searchHref({ q }));
		ui.toast(`${refusalWords(e)} Here are other releases of ${q}.`);
		return undefined;
	}
}
