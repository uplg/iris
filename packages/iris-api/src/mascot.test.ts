import { describe, expect, it } from 'vitest';
import { MASCOT_ANTENNAS, MASCOT_EYES, MASCOT_FEET, MASCOT_SCREENS, mascot, mascotHash } from './mascot';

/** The Android TV app's `MascotTest` holds these same values: one name, one mascot. */
const parts = (name: string) => {
	const m = mascot(name);
	return [
		m.tile,
		MASCOT_ANTENNAS.indexOf(m.antenna as never),
		MASCOT_EYES.indexOf(m.eyes as never),
		MASCOT_SCREENS.indexOf(m.screen as never),
		MASCOT_FEET.indexOf(m.foot as never)
	];
};

describe('mascot', () => {
	it('a name always draws the same one, whatever its case or spaces', () => {
		expect(mascot(' Théo ')).toEqual(mascot('théo'));
	});

	it('the vectors shared with the TV', () => {
		expect(mascotHash('Vincent')).toBe(4095504808);
		expect(parts('Vincent')).toEqual([0, 1, 1, 1, 2]);
		expect(parts('Muriel')).toEqual([3, 1, 1, 2, 1]);
		expect(parts('Uplg')).toEqual([1, 3, 3, 1, 2]);
		expect(parts('Théo')).toEqual([6, 0, 3, 2, 2]);
		expect(parts('')).toEqual([5, 0, 4, 0, 0]);
	});
});
