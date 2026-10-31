// Going onto a profile, the one way: the server opens its session, then the session view forgets
// what the last profile read (`accountChanged`) and every view starts over.

import { profiles, type ProfileView } from '@iris/api/client';
import { chosen } from './chosen.svelte.ts';
import { session } from '#lib/session.svelte.ts';

export async function enter(profile: ProfileView) {
	const user = profile.current ? undefined : await profiles.switchTo(profile.id);
	chosen.set(true);
	if (user) session.signedIn(user);
}
