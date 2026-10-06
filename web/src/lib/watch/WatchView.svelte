<script lang="ts">
	// One file, watched: the stage (getting ready, then the player), what the torrent is doing,
	// and the season beside it. Rebuilt for each file (the route keys it), so nothing of one
	// file's state (position, picks, demotions, prompts) leaks into the next.
	//
	// The engine tier comes from the manifest (`pickTier`), demotes on genuine engine errors
	// (B/C/D/E → E or F), never on a backend outage (a deploy): that holds the tier and remounts it
	// once the server answers again.
	import { createQuery, useQueryClient } from '@tanstack/svelte-query';
	import { STORAGE } from '#lib/storage.ts';
	import { Dialog } from 'bits-ui';
	import { untrack } from 'svelte';
	import { afterNavigate, goto } from '$app/navigation';
	import { follows, library, progress as progressApi, torrents } from '@iris/api/client';
	import {
		duration as lengthWords,
		episodeCode,
		fileName,
		formatSize,
		isVideo,
		percent,
		plural,
		prettySceneName,
		speed
	} from '@iris/api/format';
	import { hevcMseNeedsIdrStart } from '@iris/core/caps';
	import { irisFetch } from '@iris/core/stream-fetch';
	import { fetchManifest, ManifestNotReadyError, pickTier, postSeekHint, rawStreamUrl, type DecodeTier } from '@iris/core/manifest-client';
	import Icon from '#lib/components/Icon.svelte';
	import Progress from '#lib/components/Progress.svelte';
	import { Gesture, pending } from '#lib/gesture.svelte.ts';
	import { errorText, isGone } from '#lib/errors.ts';
	import { stored, text as words } from '#lib/stored.ts';
	import { pageTitle } from '#lib/title.ts';
	import { KEYS, read, refreshLibrary } from '#lib/queries.ts';
	import { stateWord } from '#lib/torrent.ts';
	import { ui } from '#lib/ui.svelte.ts';
	import IrisPlayer from '#lib/player/IrisPlayer.svelte';
	import StageTopBar from '#lib/player/StageTopBar.svelte';
	import { readStoredVolume, writeStoredVolume } from '#lib/player/browser.ts';
	import EpisodesPanel from './EpisodesPanel.svelte';
	import GettingReady from './GettingReady.svelte';
	import { isTheaterKey } from './keys.ts';
	import { fetchAgain } from '#lib/regrab.ts';
	import { listsEpisodes, retrySearchQuery, sideRows, type SideRow } from './episodes.ts';
	import { factsLine } from './facts.ts';
	import { keptForText, PlaybackChoices } from './prefs.ts';
	import { ProgressSaver } from './progress.ts';
	import { readiness } from './ready.ts';
	import {
		afterDemotions,
		Demotions,
		errorOutcome,
		forcedTier,
		isNearEnd,
		nextDemotionTarget,
		notOnDisk,
		playSource,
		playStatusInterval,
		resumeFrom,
		sourceReady,
		subtitleVersion
	} from './tier.ts';

	let { infohash, fileIdx }: { infohash: string; fileIdx: number } = $props();
	const qc = useQueryClient();

	// theater: the stage spans the page, the episodes go under it (a device's choice, like volume)
	const keptTheater = stored<'1' | '0'>(STORAGE.theater, '0', words(['1', '0']));
	let theater = $state(keptTheater.get() === '1');
	function toggleTheater() {
		theater = !theater;
		keptTheater.set(theater ? '1' : '0');
	}
	// T, while the focus is in the player only (keys.ts)
	let screen = $state<HTMLElement>();
	$effect(() => {
		const el = screen;
		if (!el) return;
		const onKey = (e: KeyboardEvent) => {
			if (!isTheaterKey(e)) return;
			e.preventDefault();
			toggleTheater();
		};
		el.addEventListener('keydown', onKey);
		return () => el.removeEventListener('keydown', onKey);
	});

	const torrentQ = createQuery(() => read.torrent(infohash));
	const data = $derived(torrentQ.data);
	const file = $derived(data?.files.find((f) => f.index === fileIdx));
	const videoFiles = $derived((data?.files ?? []).filter((f) => isVideo(f.path)));
	const name = $derived(fileName(file?.path) ?? data?.name ?? 'Iris');
	const collectionId = $derived(data?.collection_id ?? null);
	const isTv = $derived(!!collectionId && data?.kind === 'tv');

	const playStatusQ = createQuery(() => ({
		queryKey: KEYS.playStatus(infohash, fileIdx),
		queryFn: () => torrents.playStatus(infohash, fileIdx),
		refetchInterval: (q) => playStatusInterval(q.state.data),
		retry: 8,
		retryDelay: 2000
	}));

	// fired at once, not after the download: the backend prefetches the head and answers « not
	// yet on disk » until it is there; polled on that answer past the retry budget, so a slow
	// swarm self-heals instead of needing a reload
	const probeQ = createQuery(() => ({
		queryKey: KEYS.probe(infohash, fileIdx),
		queryFn: () => torrents.probe(infohash, fileIdx),
		retry: (count: number, e: Error) => notOnDisk(e) && count < 30,
		retryDelay: 2000,
		refetchInterval: (q) => (q.state.data ? false : notOnDisk(q.state.error) ? 2000 : false),
		// a file's streams never change under it (a regrab invalidates them)
		staleTime: Infinity
	}));
	const manifestQ = createQuery(() => ({
		queryKey: ['manifest', infohash, fileIdx],
		queryFn: () => fetchManifest(infohash, fileIdx),
		retry: (count: number, e: Error) => e instanceof ManifestNotReadyError && count < 30,
		retryDelay: 2000,
		refetchInterval: (q) => (q.state.data ? false : q.state.error instanceof ManifestNotReadyError ? 2000 : false),
		staleTime: Infinity
	}));
	const manifest = $derived(manifestQ.data);

	// a fresh read on every visit: a cached position would replay as if nothing was watched since
	const progressQ = createQuery(() => ({
		queryKey: ['progress', infohash, fileIdx],
		queryFn: () => progressApi.get(infohash, fileIdx),
		staleTime: 0,
		gcTime: 0
	}));
	const prefsQ = createQuery(() => ({ ...read.playbackPrefs(collectionId ?? null), enabled: !!data }));
	const torrentProgressQ = createQuery(() => ({ ...read.progress(infohash), refetchInterval: 10_000 }));
	const collectionQ = createQuery(() => ({ ...read.collection(collectionId ?? ''), enabled: isTv }));
	const episodeContextQ = createQuery(() => read.episodeContext(infohash, fileIdx));

	// tier: picked from the manifest, `?tier=` pins it; a demoted tier never comes back
	const demotions = new Demotions();
	let tier = $state<DecodeTier | null>(null);
	$effect(() => {
		const m = manifest;
		if (!m) return;
		const forced = forcedTier(location.search);
		if (forced) {
			tier = forced;
			console.log('[iris-core] tier', forced, '(forced via ?tier=)');
			return;
		}
		let cancelled = false;
		void (async () => {
			const t = await pickTier(m);
			if (cancelled) return;
			const final = afterDemotions(t, demotions.demoted);
			tier = final;
			console.log('[iris-core] tier', final, {
				container: m.container,
				video: m.video.map((v) => v.codec_string ?? v.codec),
				audio: m.audio.map((a) => `${a.codec}${a.browser_native ? '(native)' : ''}`)
			});
		})();
		return () => {
			cancelled = true;
		};
	});

	let playerError = $state<string | null>(null);
	let outage = $state(false);
	let nonce = $state(0);

	function demote(from: DecodeTier, reason: string) {
		const m = manifest;
		const to = demotions.demote(from, (f) =>
			nextDemotionTarget(f, m, { userAgent: navigator.userAgent, hevcNeedsIdrStart: hevcMseNeedsIdrStart(), demoted: demotions.demoted })
		);
		if (!to) return;
		console.warn(`[iris-core] tier ${from} → ${to} (${reason})`);
		playerError = null;
		tier = to;
		if (m) {
			void torrents
				.reportPlaybackError(m.infohash, m.file_idx, { tier: from, reason, codec: m.video[0]?.codec ?? null, browser: navigator.userAgent })
				.catch(() => undefined);
		}
	}

	/** Through the same proxy as everything: a deploy shows as 502/503/504 or no answer; any
	 * status under 500 means the server is up. */
	async function backendReachable(): Promise<boolean> {
		try {
			const res = await irisFetch(rawStreamUrl(infohash, fileIdx), { method: 'HEAD', credentials: 'include' });
			return res.status < 500;
		} catch {
			return false;
		}
	}

	async function engineError(from: DecodeTier, msg: string) {
		if (outage) return;
		if (!(await backendReachable())) {
			console.warn(`[iris-core] tier ${from}: backend unreachable (${msg}) — holding tier, reconnecting`);
			outage = true;
			return;
		}
		if (errorOutcome(from) === 'say') playerError = msg;
		else demote(from, msg);
	}

	// while the server is away, ask every 2 s; once it answers, remount on the same tier
	const recoveryQ = createQuery(() => ({
		queryKey: ['backend-recovery', infohash, fileIdx, nonce],
		queryFn: backendReachable,
		enabled: outage,
		refetchInterval: (q) => (q.state.data === true ? false : 2000),
		gcTime: 0
	}));
	$effect(() => {
		if (outage && recoveryQ.data === true) {
			outage = false;
			nonce += 1;
		}
	});

	// progress: heartbeats, pause, end, and a beacon when the page goes away
	// the route keys this view per file: its identity never changes under it
	const saver = untrack(() => new ProgressSaver(infohash, fileIdx));
	$effect(() => {
		if (!progressQ.isPending) saver.restore(progressQ.data);
	});
	$effect(() => {
		const flush = () => saver.flush();
		window.addEventListener('pagehide', flush);
		window.addEventListener('beforeunload', flush);
		return () => {
			window.removeEventListener('pagehide', flush);
			window.removeEventListener('beforeunload', flush);
			flush();
		};
	});

	const choices = $derived(new PlaybackChoices(collectionId));
	$effect(() => choices.adopt(prefsQ.data));
	function kept(p: Promise<void> | null) {
		p?.catch(() => ui.toast('Iris could not keep this choice for next time.', { warn: true }));
	}

	// the next episode: offered in the control bar near the end when it is on disk; when it is
	// only available and the series is followed, a one-off « prepare it? » at the end
	let nearEnd = $state(false);
	let nextDialog = $state(false);
	let nextDismissed = false;
	let nextPrompted = false;
	const nextEp = $derived(episodeContextQ.data?.next ?? null);
	const nextOnDisk = $derived(
		nextEp?.status === 'downloaded' && nextEp.infohash && typeof nextEp.file_idx === 'number'
			? { infohash: nextEp.infohash, fileIdx: nextEp.file_idx, season: nextEp.season, episode: nextEp.episode }
			: null
	);
	// the next episode may have come on disk since the page opened (prepared here, or elsewhere)
	$effect(() => {
		if (nearEnd) void untrack(() => qc.invalidateQueries({ queryKey: KEYS.episodeContext(infohash, fileIdx) }));
	});
	function maybePromptNext() {
		const ctx = episodeContextQ.data;
		if (!ctx?.followed || ctx.next?.status !== 'available' || !ctx.next.follow_id || nextDismissed || nextPrompted) return;
		nextPrompted = true;
		nextDialog = true;
	}
	const prepare = new Gesture();

	const g = new Gesture();
	let regrabbed = $state(false);
	function regrab() {
		return g.run(
			() => fetchAgain(infohash),
			() => {
				regrabbed = true;
				void qc.invalidateQueries({ queryKey: KEYS.torrent(infohash) });
				void qc.invalidateQueries({ queryKey: KEYS.playStatus(infohash, fileIdx) });
				void qc.invalidateQueries({ queryKey: KEYS.probe(infohash, fileIdx) });
			},
			'regrab',
			{ inline: true }
		);
	}

	const currentEpisode = $derived(collectionQ.data?.episodes.find((e) => e.infohash === infohash && e.file_idx === fileIdx));
	const rows = $derived(
		sideRows({
			infohash,
			fileIdx,
			isTvCollection: isTv,
			episodes: collectionQ.data?.episodes ?? [],
			available: collectionQ.data?.available_episodes ?? [],
			videoFiles,
			progressByFile: new Map((torrentProgressQ.data ?? []).map((p) => [p.file_idx, p]))
		})
	);
	const panelTitle = $derived(
		listsEpisodes({
			isTvCollection: isTv,
			episodes: collectionQ.data?.episodes ?? [],
			available: collectionQ.data?.available_episodes ?? []
		})
			? 'Episodes'
			: 'Other files'
	);
	const grab = new Gesture();
	const grabKey = (r: SideRow) => `grab:${r.grab?.season}:${r.grab?.episode}`;
	function grabRow(row: SideRow) {
		if (!row.grab || !collectionId) return;
		const { season, episode, language } = row.grab;
		void grab.run(
			() => library.grabCollectionEpisode(collectionId, season, episode, language),
			(res) => {
				void qc.invalidateQueries({ queryKey: KEYS.collection(collectionId) });
				return goto(`/watch/${res.infohash}/${res.file_idx}`);
			},
			grabKey(row)
		);
	}

	const replace = new Gesture();
	function replaceDead() {
		const q = retrySearchQuery(collectionQ.data?.display_title, currentEpisode, data?.name);
		return replace.run(
			() => torrents.remove(infohash),
			() => {
				void refreshLibrary();
				if (collectionId) void qc.invalidateQueries({ queryKey: KEYS.collection(collectionId) });
				return goto(`/search?${new URLSearchParams({ q })}`);
			},
			'replace',
			{ inline: true }
		);
	}

	const heading = $derived(collectionQ.data?.display_title ?? prettySceneName(name));
	const episodeTitle = $derived(episodeContextQ.data?.current?.name ?? null);
	const subheading = $derived(
		currentEpisode
			? [episodeCode(currentEpisode.season, currentEpisode.episode), episodeTitle].filter(Boolean).join(' · ')
			: isTv
				? null
				: name !== heading
					? name
					: null
	);
	// a series goes back to its page; a film steps back in the history (a film page holding one
	// copy replaced itself with this player, so a link to it would open the player again), or
	// to the library when it was opened from outside Iris
	let inApp = $state(false);
	afterNavigate(({ from }) => {
		if (from) inApp = true;
	});
	const back = $derived(
		isTv && collectionId
			? { href: `/collection/${collectionId}`, label: `Back to ${collectionQ.data?.display_title ?? 'the series'}` }
			: inApp
				? { href: '/library', label: 'Back', onclick: stepBack }
				: { href: '/library', label: 'Back to the library' }
	);
	function stepBack(e: MouseEvent) {
		if (e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return;
		e.preventDefault();
		history.back();
	}
	const facts = $derived(factsLine(data, manifest, tier));

	const source = $derived(tier ? playSource(tier, infohash, fileIdx, nonce) : null);
	const ready = $derived(
		!!tier &&
			!!manifest &&
			!progressQ.isPending &&
			!prefsQ.isLoading &&
			sourceReady(tier, playStatusQ.data?.ready === true, probeQ.data !== undefined, manifest !== undefined)
	);
	const steps = $derived(
		data
			? readiness({
					torrent: data,
					serverPrep: tier === 'F',
					hasProbe: probeQ.data !== undefined,
					hasManifest: manifest !== undefined,
					probeFetching: probeQ.isFetching,
					// during the retries the error is the failure reason: « not on disk yet » shows at once
					probeError: probeQ.error ?? probeQ.failureReason,
					progressPending: progressQ.isPending,
					playStatus: playStatusQ.data ?? null,
					playError: playStatusQ.error
				})
			: null
	);
	const notice = $derived(outage ? 'Iris is not answering. Reconnecting…' : playerError ? `The player stopped: ${playerError}` : null);
	const gone = $derived(isGone(torrentQ.error));
</script>

<svelte:head><title>{pageTitle(data ? heading : 'Watch')}</title></svelte:head>

{#snippet top()}
	<StageTopBar {back} title={heading} sub={subheading} {facts} />
{/snippet}

{#if torrentQ.isPending}
	<p class="hint pad">Loading…</p>
{:else if torrentQ.isError && !data}
	<section class="empty gone" aria-labelledby="gone-title">
		{#if gone}
			<h1 id="gone-title" tabindex="-1" class="group-title">This file is no longer on disk</h1>
			<p class="hint">
				{g.error
					? 'Iris cannot grab this release again by itself. Find it again from the library or a search.'
					: 'It was probably reclaimed to free up space. Grab it again and playback picks up where it left off.'}
			</p>
			<div class="actions">
				{#if !g.error}
					<button class="btn primary" type="button" {...pending(g.is('regrab') || regrabbed)} onclick={() => !regrabbed && regrab()}>
						<Icon name="download" busy={g.is('regrab') || regrabbed} />{g.is('regrab')
							? 'Grabbing…'
							: regrabbed
								? 'Starting…'
								: 'Grab it again'}
					</button>
				{/if}
				<a class="btn" href="/library">Open the library</a>
			</div>
		{:else}
			<h1 id="gone-title" tabindex="-1" class="group-title">This stream could not be loaded</h1>
			<p class="hint">{errorText(torrentQ.error)} Iris keeps trying.</p>
		{/if}
	</section>
{:else if data}
	<div class="watch" class:theater class:with-side={rows.length > 1 && !theater}>
		<div class="main">
			<div class="screen" bind:this={screen}>
				{#if ready && manifest && tier && source}
					<IrisPlayer
						{tier}
						src={source.src}
						title={heading}
						{manifest}
						startPosition={resumeFrom(progressQ.data)}
						initialAudioIndex={progressQ.data?.audio_track_idx ?? undefined}
						initialSubtitleStreamIdx={progressQ.data?.subtitle_track_idx ?? undefined}
						preferredAudioLang={prefsQ.data?.audio_language ?? null}
						preferredSubtitleLang={prefsQ.data?.subtitle_language ?? null}
						initialVolume={readStoredVolume()}
						onVolumeChange={(v) => writeStoredVolume(v)}
						subtitleVersion={subtitleVersion(data)}
						nextEpisode={nearEnd && nextOnDisk
							? {
									label: `Next: ${episodeCode(nextOnDisk.season, nextOnDisk.episode)}`,
									onPlay: () => goto(`/watch/${nextOnDisk.infohash}/${nextOnDisk.fileIdx}`)
								}
							: null}
						{top}
						keptFor={keptForText(collectionId, data?.kind)}
						{notice}
						onAudioTrackChange={(i) => {
							saver.audioIdx = i;
							kept(choices.audioPicked(manifest, i));
						}}
						onActiveSubtitleChange={(s) => {
							saver.subtitleIdx = s;
							kept(choices.subtitlePicked(manifest, s));
						}}
						onTimeUpdate={(t) => {
							saver.timeUpdate(t);
							nearEnd = isNearEnd(t, saver.duration);
							// with the end as a second chance: a short episode can skip the 95 % sample
							if (nearEnd) maybePromptNext();
						}}
						onDurationChange={(d) => saver.durationChange(d)}
						onSeeking={(t) => {
							saver.seekPending = true;
							postSeekHint(manifest, t);
						}}
						onPause={(t) => saver.pause(t)}
						onBusyChange={(b) => saver.busy(b)}
						onEnded={() => {
							nearEnd = true;
							saver.ended();
							maybePromptNext();
						}}
						onError={(msg) => void engineError(tier!, msg)}
					/>
				{:else}
					<div class="getting-ready">
						{@render top()}
						{#if steps}
							<GettingReady ready={steps} onreplace={replaceDead} replacing={replace.is('replace')} replaceError={replace.error} />
						{/if}
					</div>
				{/if}
			</div>

			<div class="under">
				<div class="actions">
					<button class="btn theater-btn" type="button" aria-pressed={theater} aria-keyshortcuts="t" onclick={toggleTheater}
						>Theater mode <kbd>T</kbd></button
					>
					<a class="btn" href={torrents.downloadUrl(infohash, fileIdx)} download={name}><Icon name="download" />Download</a>
					<a class="btn ghost" href="/library"><Icon name="library" />Library</a>
				</div>

				<section class="torrent" aria-labelledby="torrent-title">
					<h2 id="torrent-title" class="sr-only">The torrent</h2>
					<p class="state">
						<strong>{stateWord(data)}</strong>
						<span>{formatSize(data.progress_bytes)} of {formatSize(data.total_size_bytes)}</span>
					</p>
					<Progress
						label="Downloaded"
						value={data.progress_pct}
						max={100}
						valueText={percent(Math.min(100, Math.max(0, data.progress_pct)))}
					/>
					<p class="hint line">
						<span>Down {speed(data.download_speed_bps)}</span>
						<span>Up {speed(data.upload_speed_bps)}</span>
						<span>{plural(data.peers, 'peer')}</span>
						{#if probeQ.data?.video[0]}
							{@const v = probeQ.data.video[0]}
							<span>{v.codec.toUpperCase()}{v.width && v.height ? ` ${v.width}×${v.height}` : ''}</span>
						{/if}
						{#if probeQ.data?.duration_seconds}<span>{lengthWords(probeQ.data.duration_seconds)}</span>{/if}
						{#if data.source_provider}<span>From {data.source_provider}</span>{/if}
					</p>
					{#if data.error}<p class="warn-text">{data.error}</p>{/if}
					<p class="hint name">{data.name}</p>
				</section>

				{#if probeQ.data && probeQ.data.subtitle.length > 0}
					<section class="subs" aria-labelledby="subs-title">
						<h2 id="subs-title" class="label">Subtitles in this file ({probeQ.data.subtitle.length})</h2>
						<ul class="plain-list">
							{#each probeQ.data.subtitle as s (s.index)}
								<li>
									{s.title ?? s.language?.toUpperCase() ?? `Subtitles ${s.index + 1}`}
									<span class="hint">· {s.codec}{s.forced ? ' · forced' : ''}{s.default ? ' · default' : ''}</span>
								</li>
							{/each}
						</ul>
						<p class="hint">Switch subtitles and audio from the player, with the <kbd>C</kbd> key or the captions button.</p>
					</section>
				{/if}
			</div>
		</div>

		{#if rows.length > 1}
			<aside class="side">
				<EpisodesPanel title={panelTitle} {rows} ongrab={grabRow} grabbing={(r) => grab.is(grabKey(r))} />
			</aside>
		{/if}
	</div>

	{#if nextEp}
		<Dialog.Root
			bind:open={
				() => nextDialog,
				(o) => {
					nextDialog = o;
					if (!o) nextDismissed = true;
				}
			}
		>
			<Dialog.Portal>
				<Dialog.Overlay class="overlay" />
				<Dialog.Content class="modal">
					<div class="dlg-body">
						<Dialog.Title level={2} class="group-title">Next episode available</Dialog.Title>
						<Dialog.Description class="hint">
							{episodeCode(nextEp.season, nextEp.episode)} is ready to grab. Prepare it for next time?
						</Dialog.Description>
						<div class="actions end">
							<Dialog.Close class="btn">Later</Dialog.Close>
							<button
								class="btn primary"
								type="button"
								{...pending(prepare.is())}
								onclick={() => {
									const ep = nextEp;
									const followId = ep?.follow_id;
									if (!ep || !followId) return;
									void prepare.run(
										() => follows.grabEpisode(followId, ep.season, ep.episode),
										() => {
											nextDismissed = true;
											nextDialog = false;
											ui.say(`${episodeCode(ep.season, ep.episode)} is being prepared.`);
											void qc.invalidateQueries({ queryKey: KEYS.episodeContext(infohash, fileIdx) });
										}
									);
								}}><Icon name="download" busy={prepare.is()} />Prepare</button
							>
						</div>
					</div>
				</Dialog.Content>
			</Dialog.Portal>
		</Dialog.Root>
	{/if}
{/if}

<style>
	.pad {
		padding-block: var(--s-6);
	}
	.gone {
		display: grid;
		gap: var(--s-3);
		justify-items: center;
		margin-top: var(--s-6);
	}
	.watch {
		display: grid;
		gap: var(--s-5);
		padding-top: var(--s-4);
	}
	@media (min-width: 1200px) {
		.watch.with-side {
			grid-template-columns: minmax(0, 1fr) var(--pane);
			align-items: start;
		}
		.watch.with-side .side {
			position: sticky;
			top: calc(var(--header-h) + var(--s-4));
			max-height: calc(100svh - var(--header-h) - var(--s-6));
			overflow: auto;
		}
	}
	.main {
		display: grid;
		gap: var(--s-4);
		min-width: 0;
	}
	/* the stage keeps 16:9 and narrows until it fits the window with the title line under it in
	   sight; in theater it spans the page and fills the window's height (the picture letterboxes
	   inside) */
	.screen {
		color-scheme: dark;
		position: relative;
		justify-self: center;
		width: min(100%, calc((100svh - var(--header-h) - 9rem) * 16 / 9));
		aspect-ratio: 16 / 9;
		overflow: hidden;
		border-radius: var(--radius-xl);
		background: var(--stage);
	}
	.theater .screen {
		width: 100%;
		max-height: calc(100svh - var(--header-h) - var(--s-4) * 2);
	}
	/* a phone on its side: the title line can't fit beside a usable picture, the stage wins */
	@media (max-height: 540px) {
		.screen {
			width: 100%;
		}
	}
	.getting-ready {
		position: absolute;
		inset: 0;
		display: flex;
		flex-direction: column;
	}
	.getting-ready > :global(:first-child) {
		background: var(--stage-scrim);
		padding: var(--s-2) var(--s-3);
	}
	.getting-ready > :global(.ready) {
		flex: 1;
		min-height: 0;
	}
	.under {
		display: grid;
		gap: var(--s-4);
	}
	/* below 1200 px the page is one column already: nothing to widen */
	.theater-btn {
		display: none;
	}
	@media (min-width: 1200px) {
		.theater-btn {
			display: inline-flex;
		}
	}
	.torrent,
	.subs {
		display: grid;
		gap: var(--s-2);
		padding: var(--s-4);
		border: 1px solid var(--line);
		border-radius: var(--radius-l);
		background: var(--surface);
		min-width: 0;
	}
	.state {
		margin: 0;
		display: flex;
		flex-wrap: wrap;
		justify-content: space-between;
		gap: var(--s-2);
	}
	.line {
		display: flex;
		flex-wrap: wrap;
		gap: var(--s-1) var(--s-4);
	}
	.name {
		overflow-wrap: anywhere;
		font-family: var(--font-mono);
	}
	.subs li {
		font: var(--t-secondary);
	}
</style>
