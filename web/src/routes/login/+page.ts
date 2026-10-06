// The old sign-in address: the shell asks to sign in by itself, so this goes on to where the
// person was headed (a path on this site only).
import { redirect } from '@sveltejs/kit';
import type { PageLoad } from './$types';

export const load: PageLoad = ({ url }) => {
	const to = url.searchParams.get('redirect');
	redirect(308, to?.startsWith('/') && !to.startsWith('//') ? to : '/');
};
