<script lang="ts">
	// A release, before grabbing it: what title and part it is, its main action (download and
	// play, or play from disk when it is already there), what the swarm and the technical sheet
	// say, the file to play, and the tracker's own notes. The details answer carries the title,
	// poster and copy on disk, so the page stands alone; a search hit (cache.ts) fills in first.
	import { goto } from '$app/navigation';
	import { createQuery } from '@tanstack/svelte-query';
	import { follows, searchDetails, tmdbImage, torrents } from '@iris/api/client';
	import { fileName, formatRelative, formatSize, kindWord, languageLabel, plural, prettySceneName } from '@iris/api/format';
	import { loadable, queryClient } from '#lib/query.ts';
	import { KEYS, read } from '#lib/queries.ts';
	import { ui } from '#lib/ui.svelte.ts';
	import { Gesture, pending, unavailable } from '#lib/gesture.svelte.ts';
	import Disclosure from '#lib/components/Disclosure.svelte';
	import Icon from '#lib/components/Icon.svelte';
	import Loaded from '#lib/components/Loaded.svelte';
	import PageHead from '#lib/components/PageHead.svelte';
	import Poster from '#lib/components/Poster.svelte';
	import StatusLine from '#lib/components/StatusLine.svelte';
	import StatusRow from '#lib/components/StatusRow.svelte';
	import { backToResults, findRelease } from '#lib/search/cache.ts';
	import { DEAD, isDead, ownedFile, partWords, sceneMark, seedersWords } from '#lib/search/release.ts';
	import Description from './Description.svelte';
	import { isSample, playWords, sortFiles, autoFile } from './files.ts';
	import { Grab, type GrabTarget } from './grab.svelte.ts';
	import GrabNotice from './GrabNotice.svelte';
	import { audioWords, subtitleWords, videoWords } from './mediainfo.ts';
	import { watchHref } from '#lib/paths.ts';

	let { provider, id }: { provider: string; id: string } = $props();
	const uid = $props.id();
	// svelte-ignore state_referenced_locally - the page is keyed on its release (+page.svelte)
	const hit = findRelease(provider, id);
	const back = backToResults();

	const preview = createQuery(
		() => ({ queryKey: ['torrent-preview', provider, id], queryFn: () => torrents.preview(provider, id), staleTime: 5 * 60_000 }),
		() => queryClient
	);
	// best-effort: some trackers have no details, and the release can still be grabbed
	const details = createQuery(
		() => ({
			queryKey: ['search-details', provider, id],
			queryFn: () => searchDetails.get(provider, id).catch(() => null),
			staleTime: 5 * 60_000
		}),
		() => queryClient
	);

	const p = $derived(preview.data ?? null);
	const d = $derived(details.data ?? null);
	const name = $derived(p?.name ?? d?.title ?? hit?.title ?? '');
	const mark = $derived(sceneMark(name));
	const matched = $derived(d?.title_match ?? hit?.title_match ?? null);
	const title = $derived(matched?.title ?? (name ? prettySceneName(name) : 'Release'));
	const tmdbId = $derived(matched?.tmdb_id ?? hit?.tmdb_id ?? null);
	const kind = $derived(matched?.kind ?? hit?.kind ?? (mark ? 'tv' : null));
	const year = $derived(matched?.year ?? hit?.year ?? null);
	const part = $derived(partWords(hit?.parsed_season ?? mark?.season, hit?.parsed_episode ?? mark?.episode, name));
	const heading = $derived([title, part ?? (kind === 'movie' ? year : null)].filter(Boolean).join(' · '));
	const poster = $derived(hit?.poster_url ?? d?.poster_url ?? tmdbImage(matched?.poster_path, 'w342'));

	const seeders = $derived(d?.seeders ?? hit?.seeders ?? null);
	const dead = $derived(isDead(seeders));
	// the server knows the copy on disk; a search hit can still name the file of a pack
	const owned = $derived(
		d?.library_infohash && typeof d.library_file_idx === 'number'
			? { infohash: d.library_infohash, idx: d.library_file_idx }
			: hit
				? ownedFile(hit)
				: null
	);
	const archive = $derived(p !== null && !p.streamable);
	const files = $derived(p ? sortFiles(p.files) : []);
	const videos = $derived(files.filter((f) => f.is_video && !isSample(f.path)));
	const others = $derived(files.filter((f) => !videos.includes(f)));
	let picked = $state<number | null>(null);
	const chosen = $derived(picked ?? (p ? autoFile(p.files) : null));
	const reason = $derived(dead ? `${uid}-dead` : archive ? `${uid}-archive` : undefined);

	const grab = new Grab((href) => goto(href));
	let grabButton = $state<HTMLElement>();
	const target = $derived<GrabTarget>({ provider, id, tmdbId, preview: p, fileIdx: chosen });

	const tagChips = $derived(
		[
			(d?.freeleech ?? hit?.freeleech) ? 'Freeleech' : null,
			d?.exclusive ? 'Exclusive' : null,
			provider,
			languageLabel(hit?.language_tag, 'short'),
			...(d?.tags ?? hit?.tags ?? []).slice(0, 6)
		].filter((c): c is string => !!c)
	);

	const leechers = $derived(d?.leechers ?? hit?.leechers);
	const swarm = $derived(
		[
			seedersWords(seeders),
			typeof leechers === 'number' ? plural(leechers, 'leecher') : null,
			typeof d?.times_completed === 'number' ? plural(d.times_completed, 'download') : null
		]
			.filter(Boolean)
			.join(' · ')
	);
	const uploaded = $derived.by(() => {
		const at = d?.uploaded_at ?? hit?.uploaded_at;
		const when = at ? formatRelative(at) : d?.age ? `${d.age} ago` : null;
		const who = d?.uploader ?? hit?.uploader;
		return [when, who ? `by ${who}` : null].filter(Boolean).join(' ');
	});
	const filesFact = $derived(p ? `${plural(p.files.length, 'file')} · ${formatSize(p.total_size_bytes)}` : '');

	const isTv = $derived(kind === 'tv');
	const followed = createQuery(
		() => ({ ...read.follows(), enabled: isTv }),
		() => queryClient
	);
	const following = $derived(
		(followed.data ?? []).some((f) => (tmdbId !== null && f.tmdb_id === tmdbId) || f.name.toLowerCase() === title.toLowerCase())
	);
	const follow = new Gesture();

	function startFollowing() {
		return follow.run(
			() => follows.add(title, tmdbId),
			async () => {
				await Promise.all([KEYS.follows, KEYS.watchlist, KEYS.summary].map((queryKey) => queryClient.invalidateQueries({ queryKey })));
				ui.toast(`You follow ${title}. New episodes show on your home page.`);
			}
		);
	}

	function play() {
		if (reason || !p) return;
		return grab.run(target);
	}
</script>

<a class="link-btn quiet back" href={back}><Icon name="arrow-left" />Back to results</a>

<article class="release" aria-labelledby="{uid}-h">
	<div class="art"><Poster src={poster} {title} eager /></div>

	<div class="main">
		<div id="{uid}-h"><PageHead title={heading} /></div>
		{#if kind}<p class="hint">{[kindWord(kind), year].filter(Boolean).join(' · ')}</p>{/if}
		{#if name}<p class="name">{name}</p>{/if}
		{#if tagChips.length}
			<ul class="chips plain-list" aria-label="About this release">
				{#each tagChips as c (c)}<li class={['chip', c === 'Freeleech' && 'accent']}>{c}</li>{/each}
			</ul>
		{/if}

		<!-- what is on disk plays whatever the torrent preview answers (a full leech slot fails it) -->
		{#if owned}
			<p class="callout">
				<span><Icon name="circle-check" /> This release is already in your library: it plays from disk, nothing to download.</span>
			</p>
			<div class="actions">
				<a class="btn primary big" href={watchHref(owned.infohash, owned.idx)}><Icon name="play" />Play from disk</a>
			</div>
		{/if}
		<Loaded value={loadable(preview)}>
			{#if dead}
				<p class="warn-text reason" id="{uid}-dead">
					<Icon name="triangle-alert" />{DEAD}: this release cannot be downloaded. Try another one.
				</p>
			{:else if archive}
				<p class="warn-text reason" id="{uid}-archive">
					<Icon name="triangle-alert" />Packed in RAR archives, which Iris cannot stream. Choose a release with a plain video file (.mkv or
					.mp4).
				</p>
			{/if}

			<div class="actions">
				{#if owned}
					<button bind:this={grabButton} class="btn big" {...pending(grab.busy)} {...unavailable(reason)} onclick={play}>
						<Icon name="download" busy={grab.busy} />Download anyway
					</button>
				{:else}
					<button bind:this={grabButton} class="btn primary big" {...pending(grab.busy)} {...unavailable(reason)} onclick={play}>
						<Icon name="download" busy={grab.busy} />{p ? playWords(p.files, chosen) : 'Download and play'}
					</button>
				{/if}
				{#if isTv && followed.isSuccess}
					{#if following}
						<StatusLine tone="ok" text="You follow this series" />
					{:else}
						<button class="btn big" {...pending(follow.is())} onclick={startFollowing}>
							<Icon name="bookmark" busy={follow.is()} />Follow the series
						</button>
					{/if}
				{/if}
			</div>
			{#if !owned && !reason}
				<p class="hint">Playback starts once the first minutes are on disk; the rest keeps downloading while you watch.</p>
			{/if}
			<GrabNotice {grab} {target} back={() => grabButton} />

			<dl class="facts">
				{#if swarm}<StatusRow label="Swarm" value={swarm} warn={dead} />{/if}
				{#if uploaded}<StatusRow label="Uploaded" value={uploaded} />{/if}
				{#if videoWords(d?.media_info)}<StatusRow label="Video" value={videoWords(d?.media_info) ?? ''} />{/if}
				{#if audioWords(d?.media_info)}<StatusRow label="Audio" value={audioWords(d?.media_info) ?? ''} />{/if}
				{#if subtitleWords(d?.media_info)}<StatusRow label="Subtitles" value={subtitleWords(d?.media_info) ?? ''} />{/if}
				{#if filesFact}<StatusRow label="Files" value={filesFact} />{/if}
			</dl>

			{#if videos.length > 1}
				<fieldset class="files">
					<legend>File to play</legend>
					{#each videos as f (f.index)}
						<label class="file">
							<input type="radio" name="{uid}-file" value={f.index} checked={chosen === f.index} onchange={() => (picked = f.index)} />
							<span class="file-text">
								<span class="path">{fileName(f.path)}</span>
								<span class="meta">{formatSize(f.size_bytes)}</span>
							</span>
						</label>
					{/each}
				</fieldset>
			{/if}
			{#if others.length}
				<Disclosure label="Other files ({others.length})">
					<ul class="plain-list others">
						{#each others as f (f.index)}
							<li>
								<span class="path">{f.path}</span>
								<span class="meta">{formatSize(f.size_bytes)}{f.extension ? ` · ${f.extension.toUpperCase()}` : ''}</span>
							</li>
						{/each}
					</ul>
				</Disclosure>
			{/if}
		</Loaded>

		{#if d?.description || d?.nfo}
			<section class="notes" aria-labelledby="{uid}-notes">
				<h2 id="{uid}-notes" class="group-title">Release notes from {provider}</h2>
				{#if d.description}<Description source={d.description} format={d.description_format} />{/if}
				{#if d.nfo}
					<Disclosure label="Technical sheet (NFO)">
						<!-- a scrolling region: reachable by keyboard, named -->
						<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
						<pre class="nfo" tabindex="0" aria-label="Technical sheet">{d.nfo}</pre>
					</Disclosure>
				{/if}
			</section>
		{/if}
	</div>
</article>

<style>
	.back {
		display: inline-flex;
		align-items: center;
		gap: var(--s-2);
		min-height: var(--control-h);
		margin-top: var(--s-3);
	}
	.release {
		display: grid;
		gap: var(--s-5);
		grid-template-columns: minmax(0, 1fr);
	}
	.art {
		width: min(12rem, 50vw);
	}
	@media (min-width: 840px) {
		.release {
			grid-template-columns: 14rem minmax(0, 1fr);
			align-items: start;
		}
		.art {
			width: 100%;
			padding-top: var(--s-5);
		}
	}
	.main {
		display: grid;
		gap: var(--s-4);
		min-width: 0;
		max-width: var(--measure-wide);
	}
	.main :global(.page-head) {
		padding-bottom: 0;
	}
	.name,
	.path {
		margin: 0;
		font: var(--t-secondary);
		font-family: var(--font-mono);
		overflow-wrap: anywhere;
	}
	.chips {
		display: flex;
		flex-wrap: wrap;
		gap: var(--s-1);
	}
	.reason {
		display: flex;
		gap: var(--s-2);
		align-items: flex-start;
	}
	.big {
		min-height: var(--control-h);
	}
	.files {
		border: 1px solid var(--line);
		border-radius: var(--radius-l);
		margin: 0;
		padding: var(--s-3) var(--s-4);
		display: grid;
		gap: var(--s-1);
		min-width: 0;
	}
	legend {
		font: var(--t-field-label);
		padding: 0 var(--s-1);
	}
	.file {
		display: flex;
		gap: var(--s-3);
		align-items: center;
		min-height: var(--control-h);
		cursor: pointer;
	}
	.file input {
		flex: none;
		width: var(--check-box);
		height: var(--check-box);
		accent-color: var(--accent);
	}
	.file-text {
		display: grid;
		min-width: 0;
	}
	.meta {
		font: var(--t-meta);
		color: var(--ink-muted);
	}
	.others {
		display: grid;
		gap: var(--s-1);
	}
	.notes {
		display: grid;
		gap: var(--s-3);
		margin-top: var(--s-4);
	}
	.nfo {
		margin: 0;
		max-height: 24rem;
		overflow: auto;
		padding: var(--s-3);
		border: 1px solid var(--line);
		border-radius: var(--radius-m);
		background: var(--surface);
		font: var(--t-tiny);
		font-family: var(--font-mono);
	}
</style>
