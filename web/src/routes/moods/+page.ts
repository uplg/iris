// The old moods page: Discover holds the board, the mood and the kind keep their address.
import { redirect } from '@sveltejs/kit';
import type { PageLoad } from './$types';

export const load: PageLoad = ({ url }) => {
	const to = new URLSearchParams();
	for (const key of ['mood', 'kind']) {
		const v = url.searchParams.get(key);
		if (v) to.set(key, v);
	}
	redirect(308, `/discover${to.size ? `?${to}` : ''}`);
};
