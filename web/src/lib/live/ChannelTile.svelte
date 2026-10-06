<script lang="ts">
	// One channel: its logo on a plate that suits it, its name, and what is on now and next, in
	// words (a bar shows how far the programme is; the words say it too).
	import Meter from '#lib/components/Meter.svelte';
	import type { LiveChannel, LiveNowNext } from '@iris/api/client';
	import { timeLeft } from '@iris/api/format';
	import { knownTone, readTone, type LogoTone } from './logo-tone.ts';
	import { nextWords, nowWords, programmeProgress } from './live.ts';

	interface Props {
		channel: LiveChannel;
		country: string;
		nowNext?: LiveNowNext;
		/** The guide's read time (ms): « now » moves when it does, not with a timer. */
		at: number;
		showNumber?: boolean;
		/** The channel's country beside its name (search results span countries). */
		countryLabel?: string;
	}
	let { channel, country, nowNext, at, showNumber = false, countryLabel }: Props = $props();

	// the tone read from the logo once it shows (lazy, like the logo itself)
	let read = $state<LogoTone>();
	const tone = $derived(read ?? (channel.logo_url ? knownTone(channel.logo_url) : undefined) ?? 'neutral');
	let broken = $state(false);

	const now = $derived(nowNext?.now ?? null);
	const next = $derived(nowNext?.next ?? null);
	const progress = $derived(now ? programmeProgress(now.start, now.stop, at) : null);
	const left = $derived(now ? Math.max(0, (Date.parse(now.stop) - at) / 1000) : null);
</script>

<a class="tile-link" href="/live/{encodeURIComponent(country)}/{encodeURIComponent(channel.id)}">
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
		{#if showNumber && channel.tnt_number != null}<span class="badge number">{channel.tnt_number}</span>{/if}
		{#if channel.quality != null}<span class="badge quality">{channel.quality}p</span>{/if}
	</span>
	<span class="name">
		{#if showNumber && channel.tnt_number != null}<span class="sr-only"
				>Channel {channel.tnt_number},
			</span>{/if}{channel.name}{#if countryLabel}<span class="muted"> · {countryLabel}</span>{/if}
	</span>
	{#if now}
		<span class="line"
			>{nowWords(now)}{#if left != null && progress != null}<span class="muted">, {timeLeft(left)}</span>{/if}</span
		>
		{#if progress != null}<Meter share={progress / 100} thin --meter-ground="var(--line)" />{/if}
		{#if next}<span class="line muted">{nextWords(next)}</span>{/if}
	{:else}
		<span class="line muted"
			>{channel.geo_blocked
				? 'May be blocked in your country'
				: channel.not_24_7
					? 'Not on air all day'
					: 'No guide for this channel'}</span
		>
	{/if}
</a>

<style>
	.tile-link {
		display: grid;
		gap: var(--s-1);
		align-content: start;
		padding: var(--s-3);
		border: 1px solid var(--line);
		border-radius: var(--radius-l);
		background: var(--surface);
		color: var(--ink);
		text-decoration: none;
		min-width: 0;
		height: 100%;
	}
	.tile-link:hover {
		border-color: var(--accent-soft);
	}
	.well {
		position: relative;
		display: grid;
		place-items: center;
		height: 4rem;
		padding: var(--s-2);
		border-radius: var(--radius-m);
		margin-bottom: var(--s-1);
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
		max-height: 3rem;
		max-width: 100%;
		object-fit: contain;
	}
	.letter {
		display: grid;
		place-items: center;
		width: 3rem;
		height: 3rem;
		border-radius: var(--radius-m);
		background: var(--accent);
		color: var(--on-accent);
		font: var(--t-group);
	}
	.badge {
		position: absolute;
		top: var(--s-1);
		padding: 0 var(--s-1);
		border-radius: var(--radius-s);
		background: var(--stage-scrim);
		color: var(--stage-ink);
		font: var(--t-tiny);
	}
	.number {
		left: var(--s-1);
		font-family: var(--font-mono);
	}
	.quality {
		right: var(--s-1);
	}
	.name {
		font: var(--t-label);
		overflow-wrap: anywhere;
	}
	.line {
		font: var(--t-meta);
		overflow-wrap: anywhere;
	}
</style>
