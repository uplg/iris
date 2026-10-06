// The player's keyboard, framework-free: YouTube / Netflix conventions, a key to an action, and
// when a key belongs to the control under the focus instead.

export type PlayerAction =
	| { kind: 'toggle-play' }
	| { kind: 'seek-by'; seconds: number }
	| { kind: 'seek-to-fraction'; fraction: number }
	| { kind: 'volume-by'; delta: number }
	| { kind: 'toggle-mute' }
	| { kind: 'fullscreen' }
	| { kind: 'tracks' }
	| { kind: 'pip' }
	| { kind: 'shortcuts' }
	| { kind: 'speed-by'; step: 1 | -1 }
	| { kind: 'escape' };

export const SEEK_STEP = 10;
export const VOLUME_STEP = 0.1;

interface KeyLike {
	key: string;
	ctrlKey?: boolean;
	metaKey?: boolean;
	altKey?: boolean;
}

/** The action for a key, or null (a key the player leaves to the page or the browser). Live:
 * no timeline, so no seeking and no speed. */
export function actionFor(e: KeyLike, live = false): PlayerAction | null {
	if (e.ctrlKey || e.metaKey || e.altKey) return null;
	const k = e.key.length === 1 ? e.key.toLowerCase() : e.key;
	switch (k) {
		case ' ':
		case 'k':
			return { kind: 'toggle-play' };
		case 'ArrowLeft':
		case 'j':
			return live ? null : { kind: 'seek-by', seconds: -SEEK_STEP };
		case 'ArrowRight':
		case 'l':
			return live ? null : { kind: 'seek-by', seconds: SEEK_STEP };
		case 'ArrowUp':
			return { kind: 'volume-by', delta: VOLUME_STEP };
		case 'ArrowDown':
			return { kind: 'volume-by', delta: -VOLUME_STEP };
		case 'm':
			return { kind: 'toggle-mute' };
		case 'f':
			return { kind: 'fullscreen' };
		case 'c':
			return { kind: 'tracks' };
		case 'p':
			return { kind: 'pip' };
		case '?':
			return { kind: 'shortcuts' };
		case '<':
			return live ? null : { kind: 'speed-by', step: -1 };
		case '>':
			return live ? null : { kind: 'speed-by', step: 1 };
		case 'Escape':
			return { kind: 'escape' };
		default:
			if (!live && /^[0-9]$/.test(k)) return { kind: 'seek-to-fraction', fraction: Number(k) / 10 };
			return null;
	}
}

/**
 * Whether a key belongs to the focused control rather than to the player: text fields take
 * everything, a slider its arrows and Home/End, a radio group its arrows, a button or a link
 * its Space and Enter (else one press would both press it and toggle playback).
 */
export function ownedByControl(target: EventTarget | null, key: string): boolean {
	if (!(target instanceof Element)) return false;
	if (target.closest('input:not([type="radio"]), textarea, select, [contenteditable="true"]')) return true;
	const arrows = key.startsWith('Arrow') || key === 'Home' || key === 'End' || key === 'PageUp' || key === 'PageDown';
	if (arrows && target.closest('[role="slider"], input[type="radio"], [role="radio"]')) return true;
	if ((key === ' ' || key === 'Enter') && target.closest('button, a[href], input[type="radio"], [role="slider"]')) return true;
	return false;
}

/** The list the « Keyboard shortcuts » panel shows. */
export function shortcutList(live: boolean): { keys: string[]; does: string }[] {
	const all = [
		{ keys: ['Space', 'K'], does: 'Play or pause' },
		...(live
			? []
			: [
					{ keys: ['J', '←'], does: `Back ${SEEK_STEP} seconds` },
					{ keys: ['L', '→'], does: `Forward ${SEEK_STEP} seconds` },
					{ keys: ['0', '…', '9'], does: 'Go to 0% … 90% of the video' }
				]),
		{ keys: ['↑', '↓'], does: 'Volume up or down' },
		{ keys: ['M'], does: 'Mute or unmute' },
		{ keys: ['C'], does: 'Audio and subtitles' },
		{ keys: ['F'], does: 'Full screen' },
		{ keys: ['P'], does: 'Picture in picture' },
		...(live ? [] : [{ keys: ['<', '>'], does: 'Slower or faster' }]),
		{ keys: ['?'], does: 'This list' },
		{ keys: ['Esc'], does: 'Close a panel' }
	];
	return all;
}

export const SPEEDS = [0.5, 0.75, 1, 1.25, 1.5, 1.75, 2] as const;

/** The next playback speed from `current`, one step up or down, clamped to the list. */
export function nextSpeed(current: number, step: 1 | -1): number {
	let i = SPEEDS.findIndex((s) => Math.abs(s - current) < 0.01);
	if (i < 0) i = SPEEDS.indexOf(1);
	return SPEEDS[Math.min(SPEEDS.length - 1, Math.max(0, i + step))];
}
