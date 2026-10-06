// What the interface remembers about itself: the theme, the toasts, and the two live regions
// that say each outcome once (the layout mounts them, only their text changes).
//
// No timers (CLAUDE.md web rule): a toast leaves when its CSS countdown ends (`animationend`,
// paused while hovered or focused: WCAG 2.2.1), and a live region repeats a message by
// alternating an invisible suffix instead of clearing itself first.

import { haptic, FAILURE } from '#lib/haptics.ts';
import { errorText } from '#lib/errors.ts';
import { stored, text as words } from '#lib/stored.ts';
import { PUBLIC_THEME_KEY } from '$app/env/public';

export type Theme = 'system' | 'light' | 'dark';

/** A toast's one action ("Undo"): its words, what it does, and where the focus goes when the
 * toast leaves while holding it (the row it was about is gone). */
export interface ToastAction {
	label: string;
	run: () => unknown;
	back?: () => HTMLElement | null | undefined;
}

export interface ToastOptions {
	/** A failure: assertive, and it stays until dismissed. */
	warn?: boolean;
	action?: ToastAction;
}

export type Toast = { id: number; text: string; warn: boolean; action?: ToastAction };

// "system" is nothing kept: the page follows the system
const keptTheme = stored<Theme>(PUBLIC_THEME_KEY, 'system', words<Theme>(['light', 'dark']));
const AGAIN = '​';

class Ui {
	theme = $state<Theme>(keptTheme.get());
	toasts = $state<Toast[]>([]);
	polite = $state('');
	assertive = $state('');
	#next = 0;

	setTheme(next: Theme) {
		this.theme = next;
		keptTheme.set(next === 'system' ? undefined : next);
	}

	/** Say something politely, once (a screen reader hears it; nothing moves on screen). */
	say(text: string) {
		this.polite = this.polite === text ? text + AGAIN : text;
	}

	/** A short message under the fingers, read once. Failures stay until dismissed. Returns
	 * its id (its action button is `toast-action-<id>`). */
	toast(text: string, { warn = false, action }: ToastOptions = {}): number {
		// the same words again replace the toast already saying them
		for (const t of this.toasts) if (t.text === text && t.warn === warn) this.dismiss(t.id);
		const id = this.#next++;
		this.toasts.push({ id, text, warn, action });
		if (warn) this.assertive = this.assertive === text ? text + AGAIN : text;
		else this.say(text);
		return id;
	}

	/** A gesture failed: felt, said in plain words (errors.ts), and shown until read. */
	fail(error: unknown) {
		haptic(FAILURE);
		this.toast(errorText(error), { warn: true });
	}

	dismiss(id: number) {
		this.toasts = this.toasts.filter((t) => t.id !== id);
	}
}

export const ui = new Ui();
