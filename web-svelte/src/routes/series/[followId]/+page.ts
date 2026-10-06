// The old series page: a follow's id is its collection's id, so it is a straight move (an id
// that resolves to nothing says so on the collection page).
import { redirect } from '@sveltejs/kit';
import type { PageLoad } from './$types';

export const load: PageLoad = ({ params }) => {
	redirect(308, `/collection/${encodeURIComponent(params.followId)}`);
};
