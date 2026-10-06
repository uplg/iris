<script lang="ts">
	// One channel, as a guide line: its logo on a plate that suits it, its number on free-to-air,
	// its name, what is on now (how far, until when) and next. A channel that will likely not
	// play says why, in words, and steps back.
	import Meter from '#lib/components/Meter.svelte';
	import type { LiveChannel, LiveNowNext } from '@iris/api/client';
	import { clockTime, timeLeft } from '@iris/api/format';
	import { knownTone, readTone, type LogoTone } from './logo-tone.ts';
	import { nextWords, programmeProgress } from './live.ts';
	import { channelNotice, dimmed } from './guide.ts';

	interface Props {
		channel: LiveChannel;
		country: string;
		nowNext?: LiveNowNext;
		/** The guide's read time (ms): « now » moves when it does, not with a timer. */
		at: number;
		/** The country has a guide: a channel without an entry says so. */
		guided?: boolean;
		/** The channel's country beside its name (search results span countries). */
		countryLabel?: string;
	}
	let { channel, country, nowNext, at, guided = false, countryLabel }: Props = $props();

	// the tone read from the logo once it shows (lazy, like the logo itself)
	let read = $state<LogoTone>();
	const tone = $derived(read ?? (channel.logo_url ? knownTone(channel.logo_url) : undefined) ?? 'neutral');
	let broken = $state(false);

	const now = $derived(nowNext?.now ?? null);
	const next = $derived(nowNext?.next ?? null);
	const progress = $derived(now ? programmeProgress(now.start, now.stop, at) : null);
	const left = $derived(now && progress !== null ? Math.max(0, (Date.parse(now.stop) - at) / 1000) : null);
	const notice = $derived(channelNotice(channel));
	const number = $derived(typeof channel.tnt_number === 'number' ? channel.tnt_number : null);
</script>

<a class="channel" class:dim={dimmed(channel)} href="/live/{encodeURIComponent(country)}/{encodeURIComponent(channel.id)}">
	<span class="well {tone}" aria-hidden="true">
		{#if channel.logo_url && !broken}
			{@const url = channel.logo_url}
			<img
				src={url}
				alt=""
				loading="lazy"
				referrerpolicy="no-referrer"
				onload={(e) => (read = readTone(url, e.currentTarget as HTMLImageElement))}
				onerror={() => (broken = true)}
			/>
		{:else}
			<span class="letter">{channel.name.charAt(0).toUpperCase()}</span>
		{/if}
	</span>
	<span class="body">
		<span class="title">
			{#if number !== null}<span class="number"><span class="sr-only">{'Channel '}</span>{number}</span>{/if}
			<span class="name">{channel.name}</span>
			{#if countryLabel}<span class="muted meta">{countryLabel}</span>{/if}
		</span>
		{#if now}
			<span class="now"><span class="sr-only">{'Now: '}</span>{now.title}</span>
			<!-- the next programme's time already says when this one ends -->
			<span class="meta muted"
				>{#if next && left !== null}{timeLeft(left)}{:else}Until {clockTime(now.stop)}{#if left !== null}, {timeLeft(left)}{/if}{/if}</span
			>
			{#if progress !== null}<Meter share={progress / 100} thin --meter-ground="var(--line)" />{/if}
			{#if next}<span class="meta muted next">{nextWords(next)}</span>{/if}
		{:else if guided && !notice}
			<span class="meta muted">No guide for this channel</span>
		{/if}
		{#if notice}<span class="meta notice">{notice}</span>{/if}
	</span>
</a>

<style>
	.channel {
		display: grid;
		grid-template-columns: auto minmax(0, 1fr);
		align-items: center;
		gap: var(--s-3);
		min-height: var(--row-min);
		height: 100%;
		padding: var(--s-3);
		border: 1px solid var(--line);
		border-radius: var(--radius-l);
		background: var(--surface);
		color: var(--ink);
		text-decoration: none;
	}
	.channel:hover {
		border-color: var(--accent-soft);
	}
	.channel:focus-visible {
		outline: var(--focus-ring);
		outline-offset: var(--focus-offset);
	}
	/* a channel that will likely not play steps back: no plate, a faded logo, quieter words */
	.channel.dim {
		background: transparent;
		border-style: dashed;
	}
	.channel.dim .well {
		filter: grayscale(1);
		opacity: 0.55;
	}
	.channel.dim .name {
		color: var(--ink-muted);
	}
	.well {
		display: grid;
		/* definite tracks: an auto one sizes to the image, and its max-height then resolves to
		   nothing, so a large square logo spilled out of the plate */
		grid-template: minmax(0, 1fr) / minmax(0, 1fr);
		place-items: center;
		width: 4.5rem;
		height: 3rem;
		padding: var(--s-1);
		overflow: hidden;
		border-radius: var(--radius-m);
	}
	.well.light {
		background: var(--raw-cloud);
	}
	.well.dark {
		background: var(--raw-night);
	}
	.well.neutral {
		background: color-mix(in srgb, var(--raw-muted-dark) 55%, transparent);
	}
	.well img {
		width: 100%;
		height: 100%;
		object-fit: contain;
	}
	.letter {
		display: grid;
		place-items: center;
		width: 2.25rem;
		height: 2.25rem;
		border-radius: var(--radius-m);
		background: var(--accent);
		color: var(--on-accent);
		font: var(--t-group);
	}
	.body {
		display: grid;
		gap: var(--s-1);
		min-width: 0;
	}
	.title {
		display: flex;
		align-items: baseline;
		flex-wrap: wrap;
		gap: 0 var(--s-2);
	}
	.number {
		min-width: 1.75rem;
		padding: 0 var(--s-1);
		border-radius: var(--radius-s);
		background: var(--ground-raised);
		font: var(--t-meta);
		font-family: var(--font-mono);
		font-variant-numeric: tabular-nums;
		text-align: center;
	}
	.name {
		font: var(--t-label);
		overflow-wrap: anywhere;
	}
	.now {
		font: var(--t-secondary);
		overflow-wrap: anywhere;
	}
	.meta {
		font: var(--t-meta);
		overflow-wrap: anywhere;
	}
	.next {
		padding-top: var(--s-half);
	}
	.notice {
		color: var(--ink-muted);
	}
</style>
