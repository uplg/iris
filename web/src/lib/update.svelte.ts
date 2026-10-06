// A newer Iris on the server, two ways (memory: deploy update banner). A deploy (`updated`
// from `$app/state`) is taken at a harmless moment only: never under the fingers, never while
// something plays. A server that refuses this bundle (426, `MIN_WEB_VERSION`) locks the app
// until a reload pulls the new one.

import { CLIENT_OUTDATED_EVENT } from '@iris/api/client';

/** The routes that hold a player. */
const PLAYER = /^\/(watch|live\/[^/]+\/[^/]+)(\/|$)/;

/** Whether reloading the page now would interrupt nothing: not typing, not on a player's page,
 * nothing playing here or in a picture-in-picture window. */
export function harmlessReload(path: string, doc: Document = document): boolean {
	if (PLAYER.test(path)) return false;
	if (doc.activeElement?.closest('input, textarea, select, [contenteditable]')) return false;
	if ([...doc.querySelectorAll('video, audio')].some((m) => !(m as HTMLMediaElement).paused)) return false;
	if (doc.pictureInPictureElement) return false;
	const docPip = (doc.defaultView as { documentPictureInPicture?: { window: Window | null } } | null)?.documentPictureInPicture;
	return !docPip?.window;
}

class Outdated {
	/** The server answered 426 to this bundle. */
	locked = $state(false);

	listen() {
		const on = () => (this.locked = true);
		window.addEventListener(CLIENT_OUTDATED_EVENT, on);
		return () => window.removeEventListener(CLIENT_OUTDATED_EVENT, on);
	}
}

export const outdated = new Outdated();
