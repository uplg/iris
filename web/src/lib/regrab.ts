// A reclaimed release fetched again, the one way (history, a title's page, the player): from
// the provenance the server recorded, so the same release comes back with the same files and
// the saved positions apply again. What lists releases is read again.

import { torrents } from '@iris/api/client';
import { refreshLibrary } from '#lib/queries.ts';

export async function fetchAgain(infohash: string) {
	const res = await torrents.regrab(infohash);
	void refreshLibrary();
	return res;
}
