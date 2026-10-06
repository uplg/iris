// The old sign-in address: the shell asks to sign in by itself, so this goes on to where the
// person was headed (a path on this site only).
import { redirect } from '@sveltejs/kit';
import { sameSite } from '#lib/paths.ts';
import type { PageLoad } from './$types';

export const load: PageLoad = ({ url }) => {
	redirect(308, sameSite(url.searchParams.get('redirect'), url.origin));
};
