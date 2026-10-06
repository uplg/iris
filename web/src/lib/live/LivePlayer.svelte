<script lang="ts">
	// The live player: one GET on the master playlist says which source the backend elected
	// (`x-iris-live-upstream`, and warms the election and the tuner's remux server-side), which
	// picks the engine; a failure retries the same source once, then reports it (the backend
	// cools it down and elects the next) and rotates, within the channel's source count.
	import type { Snippet } from 'svelte';
	import { createQuery, useQueryClient } from '@tanstack/svelte-query';
	import { livetv } from '@iris/api/client';
	import IrisPlayer from '#lib/player/IrisPlayer.svelte';
	import { readStoredVolume, writeStoredVolume } from '#lib/player/browser.ts';
	import { KEYS } from '#lib/queries.ts';
	import { forcedTier } from '#lib/watch/tier.ts';
	import { liveManifest, liveTier, LiveRotation, sourceCount } from './live.ts';
	import { ENCRYPTED, isEncryptedRefusal } from './guide.ts';

	interface Props {
		country: string;
		channelId: string;
		channelName: string;
		top?: Snippet;
		/** The channel list already says every feed is DRM-locked: nothing is asked. */
		encrypted?: boolean;
	}
	let { country, channelId, channelName, top, encrypted = false }: Props = $props();
	const queryClient = useQueryClient();

	let attempt = $state(0);
	let failed = $state(false);
	const rotation = new LiveRotation();
	const manifest = $derived(liveManifest(channelName));
	const masterUrl = $derived(livetv.masterUrl(country, channelId));

	const probeQ = createQuery(() => ({
		queryKey: ['livetv', 'probe', country, channelId, attempt],
		// through the client: an expired access cookie refreshes instead of failing the channel
		queryFn: async () => {
			const headers = await livetv.masterHeaders(country, channelId);
			const forced = forcedTier(location.search);
			if (forced) console.log('[iris-core] live tier', forced, '(forced via ?tier=)');
			return {
				tier: liveTier(headers.get('x-iris-live-upstream'), typeof globalThis.VideoDecoder !== 'undefined', forced),
				sources: sourceCount(headers.get('x-iris-live-sources'))
			};
		},
		enabled: !encrypted,
		staleTime: 0,
		gcTime: 0,
		retry: (count, error) => count < 1 && !isEncryptedRefusal(error),
		// a return to the tab must not ask again: it would re-warm the election mid-watch
		refetchOnWindowFocus: false
	}));

	function rotate(reason: string) {
		const step = rotation.next(probeQ.data?.sources ?? 0);
		if (step === 'retry') {
			console.warn(`[live] remounting same source after: ${reason}`);
			attempt += 1;
			return;
		}
		console.warn(`[live] rotating source: ${reason}`);
		void livetv.reportPlaybackError(country, channelId).catch(() => undefined);
		if (step === 'rotate') attempt += 1;
		else failed = true;
	}

	// `?r=` makes the player remount on a rotation (the master route ignores it)
	const src = $derived(attempt > 0 ? `${masterUrl}?r=${attempt}` : masterUrl);
	const showFailed = $derived(failed || probeQ.isError);
	const locked = $derived(encrypted || isEncryptedRefusal(probeQ.error));
	// learnt on the zap: the channel list says it too from now on
	$effect(() => {
		if (locked && !encrypted) void queryClient.invalidateQueries({ queryKey: KEYS.liveChannels(country) });
	});
</script>

<div class="screen">
	{#if probeQ.data && !failed && !locked}
		<IrisPlayer
			live
			tier={probeQ.data.tier}
			{src}
			title={channelName}
			{manifest}
			startPosition={0}
			initialVolume={readStoredVolume()}
			onVolumeChange={(v) => writeStoredVolume(v)}
			{top}
			onEnded={() => rotate('stream ended')}
			onError={(m) => rotate(m)}
		/>
	{:else}
		<div class="waiting">
			{@render top?.()}
			<div class="say">
				{#if locked}
					<div class="failed" role="alert">
						<h2 class="group-title">This channel can't be played</h2>
						<p class="hint">{ENCRYPTED}</p>
					</div>
				{:else if showFailed}
					<div class="failed" role="alert">
						<h2 class="group-title">This channel is not playing</h2>
						<p class="hint">
							{channelName} cannot be reached right now. It may be blocked in your country, off the air, or its source may be down.
						</p>
						<button
							class="btn"
							type="button"
							onclick={() => {
								rotation.reset();
								failed = false;
								attempt += 1;
							}}>Try again</button
						>
					</div>
				{:else}
					<p role="status">Tuning in…</p>
				{/if}
			</div>
		</div>
	{/if}
</div>

<style>
	.screen {
		color-scheme: dark;
		position: relative;
		width: 100%;
		aspect-ratio: 16 / 9;
		overflow: hidden;
		border-radius: var(--radius-xl);
		background: var(--stage);
		color: var(--stage-ink);
	}
	.waiting {
		position: absolute;
		inset: 0;
		display: flex;
		flex-direction: column;
	}
	.waiting > :global(:first-child:not(.say)) {
		background: var(--stage-scrim);
		padding: var(--s-2) var(--s-3);
	}
	.say {
		flex: 1;
		display: grid;
		place-items: center;
		padding: var(--s-4);
		text-align: center;
	}
	.failed {
		display: grid;
		gap: var(--s-3);
		justify-items: center;
		max-width: 28rem;
	}
	.failed .hint {
		color: var(--stage-muted);
	}
	.failed .btn {
		min-height: var(--control-h);
	}
</style>
