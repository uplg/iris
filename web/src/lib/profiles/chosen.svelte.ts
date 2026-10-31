// Whether this tab was asked who is watching: kept for the tab (a reload never asks again), and
// asked afresh by a new visit. sessionStorage can refuse: the choice then lasts the page.

import { STORAGE } from '#lib/storage.ts';

function read(): boolean {
	try {
		return sessionStorage.getItem(STORAGE.profileChosen) === '1';
	} catch {
		return false;
	}
}

class Chosen {
	done = $state(read());

	set(done: boolean) {
		this.done = done;
		try {
			if (done) sessionStorage.setItem(STORAGE.profileChosen, '1');
			else sessionStorage.removeItem(STORAGE.profileChosen);
		} catch {
			// kept for the page only
		}
	}
}

export const chosen = new Chosen();
