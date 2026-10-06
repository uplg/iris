<script lang="ts">
	// Who is watching what right now (an admin's): the presence registry the playback heartbeat
	// feeds, read every 10 s (the query's refetchInterval). Each person: what plays, playing or
	// paused in words and by its icon, how far, on which app, for how long.
	import Meter from '#lib/components/Meter.svelte';
	import { createQuery } from '@tanstack/svelte-query';
	import { fileName, plural, since } from '@iris/api/format';
	import type { ActiveSession } from '@iris/api/client';
	import { loadable, queryClient } from '#lib/query.ts';
	import Group from '#lib/components/Group.svelte';
	import Icon from '#lib/components/Icon.svelte';
	import Loaded from '#lib/components/Loaded.svelte';
	import TitlePoster from '#lib/history/TitlePoster.svelte';
	import { progressWords, watchedShare } from '#lib/history/words.ts';
	import { sessionsQuery } from './queries.ts';

	const sessions = createQuery(sessionsQuery, () => queryClient);
	const value = loadable(sessions);

	const what = (s: ActiveSession) => fileName(s.file_path) ?? s.torrent_name ?? 'Something unnamed';
	const app = (s: ActiveSession) => {
		const where = s.client === 'tv' ? 'On the TV app' : s.client === 'web' ? 'In a browser' : 'On an Iris app';
		return s.client_version ? `${where} ${s.client_version}` : where;
	};
</script>

<Group id="watching-title" title="Now watching" fact={sessions.data ? plural(sessions.data.length, 'person', 'people') : undefined}>
	<Loaded {value} empty={sessions.data?.length === 0} emptyText="Nobody is watching right now.">
		<ul class="plain-list rows">
			{#each sessions.data ?? [] as s (s.user_id)}
				{@const playing = s.state === 'playing'}
				<li class="row">
					<TitlePoster posterPath={s.poster_path} title={what(s)} />
					<div class="text">
						<span class="name">
							<strong>{s.display_name}</strong>
							<span class={['chip', playing && 'accent']}
								><Icon name={playing ? 'play' : 'pause'} size={12} />{playing ? 'Playing' : 'Paused'}</span
							>
						</span>
						<a class="what" href="/watch/{s.infohash}/{s.file_idx}">{what(s)}</a>
						<span class="meta">{progressWords(s.position_seconds, s.duration_seconds)} · {app(s)} · {since(s.started_at)}</span>
						<Meter share={watchedShare(s.position_seconds, s.duration_seconds)} --meter-max="16rem" />
					</div>
				</li>
			{/each}
		</ul>
	</Loaded>
</Group>

<style>
	.rows {
		display: grid;
	}
	.row {
		display: flex;
		gap: var(--s-3);
		align-items: flex-start;
		padding-block: var(--s-3);
		border-bottom: 1px solid var(--line);
	}
	.text {
		flex: 1;
		min-width: 0;
		display: grid;
		gap: var(--s-1);
	}
	.name {
		display: flex;
		flex-wrap: wrap;
		gap: var(--s-2);
		align-items: center;
	}
	.what {
		color: var(--ink);
		overflow-wrap: anywhere;
	}
	.meta {
		font: var(--t-meta);
		color: var(--ink-muted);
	}
</style>
