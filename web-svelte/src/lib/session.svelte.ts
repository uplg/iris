// The session, as runes: `@iris/api/session` decides, this mirrors it for the views.

import { Session, type SessionState } from '@iris/api/session';

class SessionView {
	#core = new Session();
	state = $state<SessionState>(this.#core.state);
	user = $derived(this.state.status === 'signed_in' ? this.state.user : null);

	constructor() {
		this.#core.subscribe((s) => (this.state = s));
	}

	start() {
		this.#core.start();
		return () => this.#core.stop();
	}

	signedIn = (...a: Parameters<Session['signedIn']>) => this.#core.signedIn(...a);
	login = (...a: Parameters<Session['login']>) => this.#core.login(...a);
	register = (...a: Parameters<Session['register']>) => this.#core.register(...a);
	logout = () => this.#core.logout();
}

export const session = new SessionView();
