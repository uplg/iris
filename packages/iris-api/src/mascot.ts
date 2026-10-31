// A person's mascot, picked from their name: the Iris mark's grammar (tile, antenna, screen,
// blush, foot, and no mouth), on its 48-unit grid. The same name draws the same mascot everywhere:
// the Android TV app holds this table and this hash too (`ui/components/Mascot.kt`), and the test
// vectors in both must stay equal.

export const MASCOT_TILES = 8;

export const MASCOT_ANTENNAS = ['M19 8.5l5 6 5-6', 'M24 14.5V9.5M24 6.5v.01', 'M18 14.5l-2-6M30 14.5l2-6', 'M24 14.5l5-6 4 2'] as const;

export const MASCOT_EYES = [
	'M16.4 25.4a2.3 2.3 0 0 1 4.4 0M27.2 25.4a2.3 2.3 0 0 1 4.4 0',
	'M16.4 24a2.3 2.3 0 0 0 4.4 0M27.2 24a2.3 2.3 0 0 0 4.4 0',
	'M18.6 23.2v2.8M29.4 23.2v2.8',
	'M16.4 25.4a2.3 2.3 0 0 1 4.4 0M29.4 23.2v2.8',
	'M16.2 24.8h4.8M27 24.8h4.8',
	'M16.6 23.4l4 1.6M31.4 23.4l-4 1.6'
] as const;

export const MASCOT_SCREENS = [
	'M15.5 15h17a6 6 0 0 1 6 6v10a6 6 0 0 1-6 6h-17a6 6 0 0 1-6-6v-10a6 6 0 0 1 6-6z',
	'M12.5 15h23a3 3 0 0 1 3 3v16a3 3 0 0 1-3 3h-23a3 3 0 0 1-3-3v-16a3 3 0 0 1 3-3z',
	'M19.5 15h9a10 10 0 0 1 10 10v2a10 10 0 0 1-10 10h-9a10 10 0 0 1-10-10v-2a10 10 0 0 1 10-10z'
] as const;

export const MASCOT_FEET = ['M19 41h10', 'M15 38v3M33 38v3', 'M14 41h20'] as const;

export const MASCOT_BLUSH = 'M13.2 29.6a2 1.2 0 1 0 4 0a2 1.2 0 1 0-4 0zM30.8 29.6a2 1.2 0 1 0 4 0a2 1.2 0 1 0-4 0z';

export interface Mascot {
	tile: number;
	antenna: string;
	eyes: string;
	screen: string;
	foot: string;
}

/** FNV-1a over the UTF-8 of the name, trimmed and in lower case: « Théo » and « théo » are one. */
export function mascotHash(name: string): number {
	let h = 0x811c9dc5;
	for (const byte of new TextEncoder().encode(name.trim().toLowerCase())) h = Math.imul(h ^ byte, 0x01000193);
	return h >>> 0;
}

export function mascot(name: string): Mascot {
	let h = mascotHash(name);
	const take = <T>(n: number, from: (i: number) => T): T => {
		const i = h % n;
		h = Math.floor(h / n);
		return from(i);
	};
	return {
		tile: take(MASCOT_TILES, (i) => i),
		antenna: take(MASCOT_ANTENNAS.length, (i) => MASCOT_ANTENNAS[i]),
		eyes: take(MASCOT_EYES.length, (i) => MASCOT_EYES[i]),
		screen: take(MASCOT_SCREENS.length, (i) => MASCOT_SCREENS[i]),
		foot: take(MASCOT_FEET.length, (i) => MASCOT_FEET[i])
	};
}
