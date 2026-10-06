// The session, as runes: `@iris/api/session` decides, this mirrors it for the views.

import { Session, type SessionState } from '@iris/api/session';
import { Gesture } from '#lib/gesture.svelte.ts';
import { queryClient } from '#lib/query.ts';
import { rememberSearch } from '#lib/search/cache.ts';

const whose = (s: SessionState) => (s.status === 'signed_in' ? s.user.id : null);

/** Whether what the cache holds may belong to someone else now: the account left (signed out,
 * expired) or another one came in. A first answer after loading is the same person. */
export const accountChanged = (was: SessionState, now: SessionState) => whose(was) !== null && whose(was) !== whose(now);

class SessionView {
	#core = new Session();
	state = $state<SessionState>(this.#core.state);
	user = $derived(this.state.status === 'signed_in' ? this.state.user : null);
	/** Signing out, as a gesture: busy while it travels, a failure said. */
	readonly out = new Gesture();

	constructor() {
		this.#core.subscribe((s) => {
			// a household browser: the next person must never read, or save over, the last one's answers
			if (accountChanged(this.state, s)) forgetAccount();
			this.state = s;
		});
	}

	start() {
		this.#core.start();
		return () => this.#core.stop();
	}

	signedIn = (...a: Parameters<Session['signedIn']>) => this.#core.signedIn(...a);
	login = (...a: Parameters<Session['login']>) => this.#core.login(...a);
	register = (...a: Parameters<Session['register']>) => this.#core.register(...a);
	logout = () => this.out.run(() => this.#core.logout(), undefined, 'logout');
}

function forgetAccount() {
	queryClient.clear();
	rememberSearch('/search');
}

export const session = new SessionView();
