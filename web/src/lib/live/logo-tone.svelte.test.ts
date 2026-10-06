import { describe, expect, it, vi } from 'vitest';
import { render } from 'vitest-browser-svelte';
import type { LiveChannel } from '@iris/api/client';
import ChannelRow from './ChannelRow.svelte';
import { knownTone, readTone, toneFor } from './logo-tone.ts';

/** A plain square logo of one colour, as a data URL. */
function logo(colour: string): string {
	const c = document.createElement('canvas');
	c.width = c.height = 8;
	const ctx = c.getContext('2d')!;
	ctx.fillStyle = colour;
	ctx.fillRect(0, 0, 8, 8);
	return c.toDataURL('image/png');
}

const loaded = (src: string) =>
	new Promise<HTMLImageElement>((resolve) => {
		const img = new Image();
		img.onload = () => resolve(img);
		img.src = src;
	});

describe('a logo’s plate', () => {
	it('a dark logo gets a light well, a light one a dark well; read once per logo', async () => {
		const black = logo('#000');
		const white = logo('#fff');
		expect(knownTone(black)).toBeUndefined();
		expect(readTone(black, await loaded(black))).toBe('light');
		expect(readTone(white, await loaded(white))).toBe('dark');
		expect(knownTone(black)).toBe('light');
	});

	it('is read from the tile’s own lazy logo, never a second download', async () => {
		const spy = vi.spyOn(window, 'Image');
		const url = logo('#fff');
		const channel = { id: 'c1', name: 'Arte', logo_url: url } as LiveChannel;
		const { container } = await render(ChannelRow, { channel, country: 'fr', at: Date.now() });
		const img = container.querySelector('img')!;
		expect(img.getAttribute('loading')).toBe('lazy');
		await expect.poll(() => container.querySelector('.well')?.classList.contains('dark')).toBe(true);
		expect(spy).not.toHaveBeenCalled();
		spy.mockRestore();
	});
});

describe('the plate with the most contrast', () => {
	it('never a grey one: a red or orange logo on the light plate, a grey one on the dark', async () => {
		expect(readTone(logo('#e2001a'), await loaded(logo('#e2001a')))).toBe('light');
		expect(readTone(logo('#f39200'), await loaded(logo('#f39200')))).toBe('dark');
		expect(readTone(logo('#8a8a8a'), await loaded(logo('#8a8a8a')))).toBe('dark');
		expect(toneFor(0.01)).toBe('light');
		expect(toneFor(0.9)).toBe('dark');
	});

	it('a large square logo stays inside its plate', async () => {
		const c = document.createElement('canvas');
		c.width = c.height = 400;
		const ctx = c.getContext('2d')!;
		ctx.fillStyle = '#e2001a';
		ctx.fillRect(0, 0, 400, 400);
		const channel = { id: 'c2', name: 'FM104', logo_url: c.toDataURL('image/png') } as LiveChannel;
		const { container } = await render(ChannelRow, { channel, country: 'ie', at: Date.now() });
		const img = container.querySelector('img')!;
		await expect.poll(() => img.complete && img.naturalWidth > 0).toBe(true);
		const well = img.parentElement!.getBoundingClientRect();
		const box = img.getBoundingClientRect();
		expect(box.width).toBeLessThanOrEqual(well.width);
		expect(box.height).toBeLessThanOrEqual(well.height);
		expect(box.top).toBeGreaterThanOrEqual(well.top);
		expect(box.bottom).toBeLessThanOrEqual(well.bottom);
	});
});
