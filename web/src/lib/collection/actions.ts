// What every gesture on the page ends with: the server answered, so the collection is read
// again before anything is shown as changed (no optimistic UI), and the views that list the
// same releases are told to read again when next shown.

import { queryClient } from '#lib/query.ts';
import { KEYS, refreshLibrary } from '#lib/queries.ts';

export async function refetchCollection(id: string): Promise<void> {
	void refreshLibrary();
	void queryClient.invalidateQueries({ queryKey: KEYS.progressAll });
	await queryClient.invalidateQueries({ queryKey: KEYS.collection(id) });
}
