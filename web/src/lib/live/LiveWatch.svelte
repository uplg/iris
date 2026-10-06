<script lang="ts">
	// One channel, live: the player, what is on now (how far, what it is about) and next, and an
	// escape hatch when it plays badly (garbled sound, artifacts) that the player cannot notice
	// by itself: report this feed and start again on the next one.
	import Meter from '#lib/components/Meter.svelte';
	import { createQuery } from '@tanstack/svelte-query';
	import { livetv } from '@iris/api/client';
	import { clockTime, timeLeft } from '@iris/api/format';
	import Icon from '#lib/components/Icon.svelte';
	import { pageTitle } from '#lib/title.ts';
	import { read } from '#lib/queries.ts';
	import { ui } from '#lib/ui.svelte.ts';
	import StageTopBar from '#lib/player/StageTopBar.svelte';
	import LivePlayer from './LivePlayer.svelte';
	import { nextWords, nowWords, programmeProgress } from './live.ts';
	import { channelNotice } from './guide.ts';

	let { country, channelId }: { country: string; channelId: string } = $props();

	/** Faster than the grid: the page shows a live progress bar. */
	const EPG_REFETCH_MS = 30_000;

	const channelsQ = createQuery(() => read.liveChannels(country));
	const epgQ = createQuery(() => ({ ...read.liveEpg(country), refetchInterval: EPG_REFETCH_MS }));

	const channel = $derived(channelsQ.data?.channels.find((c) => c.id === channelId));
	const name = $derived(channel?.name ?? channelId);
	const nowNext = $derived(epgQ.data?.entries.find((e) => e.channel_id === channelId));
	const now = $derived(nowNext?.now ?? null);
	const next = $derived(nowNext?.next ?? null);
	const at = $derived(epgQ.dataUpdatedAt || Date.now());
	const progress = $derived(now ? programmeProgress(now.start, now.stop, at) : null);

	// a fresh player section (a fresh rotation budget, a fresh probe)
	let epoch = $state(0);
	function anotherSource() {
		void livetv.reportPlaybackError(country, channelId).catch(() => undefined);
		epoch += 1;
		ui.say('Trying another source.');
	}
</script>

<svelte:head><title>{pageTitle(name)}</title></svelte:head>

{#snippet top()}
	<StageTopBar
		back={{ href: `/live?country=${encodeURIComponent(country)}`, label: 'Back to channels' }}
		title={name}
		sub={now ? nowWords(now) : null}
		facts={channel ? channelNotice(channel) : null}
	/>
{/snippet}

<div class="live-watch">
	{#key epoch}
		<LivePlayer {country} {channelId} channelName={name} {top} />
	{/key}

	<div class="actions">
		<button class="btn" type="button" onclick={anotherSource}><Icon name="refresh-cw" />Try another source</button>
	</div>

	{#if now || next}
		<section class="guide" aria-labelledby="guide-title">
			<h2 id="guide-title" class="sr-only">On this channel</h2>
			{#if now}
				<div class="now">
					<p class="title-row">
						<strong>{now.title}</strong>
						<span class="hint"
							>{clockTime(now.start)} to {clockTime(now.stop)}{#if progress != null}, {timeLeft(
									Math.max(0, (Date.parse(now.stop) - at) / 1000)
								)}{/if}</span
						>
					</p>
					{#if progress != null}<Meter share={progress / 100} thin --meter-ground="var(--line)" />{/if}
					{#if now.description}<p class="hint desc">{now.description}</p>{/if}
				</div>
			{/if}
			{#if next}<p class="hint">{nextWords(next)}</p>{/if}
		</section>
	{/if}
</div>

<style>
	.live-watch {
		display: grid;
		gap: var(--s-4);
		padding-top: var(--s-4);
	}
	.guide {
		display: grid;
		gap: var(--s-2);
		padding: var(--s-4);
		border: 1px solid var(--line);
		border-radius: var(--radius-l);
		background: var(--surface);
	}
	.now {
		display: grid;
		gap: var(--s-2);
	}
	.title-row {
		margin: 0;
		display: flex;
		flex-wrap: wrap;
		justify-content: space-between;
		gap: var(--s-2);
	}
	.desc {
		display: -webkit-box;
		-webkit-line-clamp: 3;
		line-clamp: 3;
		-webkit-box-orient: vertical;
		overflow: hidden;
	}
</style>
