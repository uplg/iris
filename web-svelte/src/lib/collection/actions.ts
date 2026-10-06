// What every gesture on the page ends with: the server answered, so the collection is read
// again before anything is shown as changed (no optimistic UI), and the views that list the
// same releases are told to read again when next shown.

import { queryClient } from '#lib/query.ts';

export const collectionKey = (id: string) => ['collection', id] as const;
export const progressKey = (infohash: string) => ['torrent-progress', infohash] as const;

export async function refetchCollection(id: string): Promise<void> {
	for (const key of ['library', 'history', 'continue-watching', 'watchlist']) void queryClient.invalidateQueries({ queryKey: [key] });
	await queryClient.invalidateQueries({ queryKey: collectionKey(id) });
}

/** Something is still downloading: the page reads it again until it is done. */
export const POLL_MS = 3000;
