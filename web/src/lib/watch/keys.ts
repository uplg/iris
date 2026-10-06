// The watch page's own key, beside the player's (player/shortcuts.ts): T for theater mode. A
// single-character shortcut is active only while the focus is in the player (WCAG 2.1.4), so
// the page listens on the stage, not the window; a field or a control keeps its key, and a
// modal dialog over the page keeps them all.

import { ownedByControl } from '#lib/player/shortcuts.ts';

export const THEATER_KEY = 't';

export function isTheaterKey(e: KeyboardEvent): boolean {
	if (e.ctrlKey || e.metaKey || e.altKey || e.key.toLowerCase() !== THEATER_KEY) return false;
	if (ownedByControl(e.target, e.key)) return false;
	return !document.querySelector('[role="dialog"][aria-modal="true"], [role="alertdialog"][aria-modal="true"], dialog[open]');
}
