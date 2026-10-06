// What is typed, a moment later: the TMDB suggestions follow the field without asking TMDB on
// every key. The tracker search never goes through here (it runs on submit).

const DELAY_MS = 250;

export class Settled {
	value = $state('');
	#timer: ReturnType<typeof setTimeout> | undefined;

	constructor(initial = '') {
		this.value = initial;
	}

	set(next: string) {
		clearTimeout(this.#timer);
		// approved web timer: TMDB typeahead debounce
		this.#timer = setTimeout(() => (this.value = next), DELAY_MS);
	}

	/** Now, without waiting (a suggestion picked, the field cleared). */
	now(next: string) {
		clearTimeout(this.#timer);
		this.value = next;
	}

	stop() {
		clearTimeout(this.#timer);
	}
}
