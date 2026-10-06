<script lang="ts">
	// Who is watching what right now (an admin's), the first thing of the admin: the presence
	// registry the playback heartbeat feeds, read every 10 s (the query's refetchInterval). One
	// card a person: what plays as people name it, where they are in it as clocks (« 1:02:14 of
	// 2:10:00 ») with its bar and what is left, its state in words (playing, paused, buffering,
	// or no news for a while), the app and its version (an old one said so) and since when; the
	// file itself on demand.
	import { createQuery } from '@tanstack/svelte-query';
	import { IRIS_WEB_VERSION, type ActiveSession } from '@iris/api/client';
	import { clock, clockTime, duration, fileName, plural, timeLeft } from '@iris/api/format';
	import { loadable, queryClient } from '#lib/query.ts';
	import Disclosure from '#lib/components/Disclosure.svelte';
	import Group from '#lib/components/Group.svelte';
	import Icon, { type IconName } from '#lib/components/Icon.svelte';
	import Loaded from '#lib/components/Loaded.svelte';
	import Meter from '#lib/components/Meter.svelte';
	import TitlePoster from '#lib/history/TitlePoster.svelte';
	import { playName } from '#lib/history/words.ts';
	import { watchedShare } from '#lib/watched.ts';
	import { personHref, watchHref } from '#lib/paths.ts';
	import { liveState, olderThan, type LiveState } from './model.ts';
	import { sessionsQuery } from './queries.ts';

	const sessions = createQuery(sessionsQuery, () => queryClient);
	const value = loadable(sessions);
	const now = $derived(sessions.dataUpdatedAt || Date.now());

	const STATES: Record<LiveState, { word: string; icon: IconName }> = {
		playing: { word: 'Playing', icon: 'play' },
		paused: { word: 'Paused', icon: 'pause' },
		buffering: { word: 'Buffering', icon: 'loader-circle' },
		stalled: { word: 'No news', icon: 'triangle-alert' }
	};
	/** How long since its last heartbeat, once that says something (stalled). */
	const silence = (s: ActiveSession) => duration((now - new Date(s.last_seen_at).getTime()) / 1000);
	const app = (s: ActiveSession) => {
		const name = s.client === 'tv' ? 'Android TV' : s.client === 'web' ? 'Web' : 'An Iris app';
		return [s.client_version ? `${name} ${s.client_version}` : name, s.browser].filter(Boolean).join(' · ');
	};
	const position = (s: ActiveSession) =>
		s.duration_seconds ? `${clock(s.position_seconds)} of ${clock(s.duration_seconds)}` : `At ${clock(s.position_seconds)}`;
	const left = (s: ActiveSession) =>
		s.duration_seconds && s.duration_seconds > s.position_seconds ? timeLeft(s.duration_seconds - s.position_seconds) : null;
	const file = (s: ActiveSession) => fileName(s.file_path) ?? s.torrent_name;
</script>

<Group id="watching-title" title="Now watching" fact={sessions.data?.length ? plural(sessions.data.length, 'person', 'people') : undefined}>
	<Loaded
		{value}
		empty={sessions.data?.length === 0}
		emptyText="Nobody is watching right now."
		emptyHint="A play shows here within seconds."
	>
		<ul class="plain-list cards">
			{#each sessions.data ?? [] as s (s.user_id)}
				{@const state = liveState(s, now)}
				{@const name = playName(s)}
				{@const old = olderThan(s.client_version, IRIS_WEB_VERSION)}
				<li class="card whole-card">
					<TitlePoster posterPath={s.poster_path} title={name.title} />
					<div class="text">
						<p class="who">
							<a href={personHref(s.user_id)}>{s.display_name}</a>
							<span class={['chip', state === 'playing' && 'accent', state === 'stalled' && 'warn']}>
								<Icon name={STATES[state].icon} size={12} />{STATES[state].word}{#if state === 'stalled'}&nbsp;for {silence(s)}{/if}
							</span>
						</p>
						<a class="card-link title" href={s.collection_id ? `/collection/${s.collection_id}` : watchHref(s.infohash, s.file_idx)}
							>{name.title}</a
						>
						{#if name.detail}<p class="detail">{name.detail}</p>{/if}
						<div class="progress">
							<p class="how">
								<span class="at">{position(s)}</span>
								{#if left(s)}<span class="muted">{left(s)}</span>{/if}
							</p>
							<Meter share={watchedShare(s.position_seconds, s.duration_seconds) ?? 0} />
						</div>
						<p class="quiet">{app(s)}</p>
						{#if old}<p class="old"><Icon name="circle-alert" size={14} />Older than {IRIS_WEB_VERSION}, the current release</p>{/if}
						<p class="quiet">Since {clockTime(s.started_at)}, {duration((now - new Date(s.started_at).getTime()) / 1000)} ago</p>
						{#if file(s)}
							<Disclosure label="The file">
								<p class="file">{file(s)}</p>
							</Disclosure>
						{/if}
					</div>
				</li>
			{/each}
		</ul>
	</Loaded>
</Group>

<style>
	.cards {
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(min(21rem, 100%), 1fr));
		gap: var(--s-4);
	}
	.card {
		display: flex;
		gap: var(--s-4);
		align-items: flex-start;
		padding: var(--s-4);
		border: 1px solid var(--line);
		border-radius: var(--radius-l);
		background: var(--surface);
		min-width: 0;
	}
	.text {
		flex: 1;
		min-width: 0;
		display: grid;
		gap: var(--s-1);
	}
	.text p {
		margin: 0;
	}
	.who {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: var(--s-1) var(--s-2);
		font: var(--t-label);
	}
	.who a {
		color: var(--ink);
		min-height: var(--control-h-xs);
		display: inline-flex;
		align-items: center;
		overflow-wrap: anywhere;
	}
	.title {
		color: var(--ink);
		font: var(--t-group);
		text-decoration: none;
		overflow-wrap: anywhere;
	}
	.detail {
		font: var(--t-secondary);
		color: var(--ink-muted);
	}
	.progress {
		display: grid;
		gap: var(--s-1);
		margin-top: var(--s-2);
	}
	.how {
		display: flex;
		flex-wrap: wrap;
		justify-content: space-between;
		gap: 0 var(--s-3);
		font: var(--t-secondary);
		font-variant-numeric: tabular-nums;
	}
	.at {
		font-weight: 600;
	}
	.quiet {
		font: var(--t-meta);
		color: var(--ink-muted);
	}
	.progress + .quiet {
		margin-top: var(--s-1);
	}
	.old {
		display: flex;
		align-items: center;
		gap: var(--s-1);
		font: var(--t-meta);
		color: var(--warn-text);
	}
	.old :global(.icon) {
		flex: none;
	}
	.file {
		font: var(--t-tiny);
		font-family: var(--font-mono);
		color: var(--ink-muted);
		overflow-wrap: anywhere;
	}
</style>
