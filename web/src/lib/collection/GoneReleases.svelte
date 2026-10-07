<script lang="ts">
	// What was on disk and the episode list cannot show in place (a movie, a pack never split):
	// how far it was watched, its name; « Download again » brings the same release back (same
	// infohash: the saved position resumes), « Hide » takes it off this page for this person only.
	import { me, type GoneReleaseEntry } from '@iris/api/client';
	import { fetchAgainOrSearch } from '#lib/regrab.ts';
	import { ago, formatSize, plural } from '@iris/api/format';
	import { progressWords } from '#lib/history/words.ts';
	import { Gesture, pending } from '#lib/gesture.svelte.ts';
	import { refocus } from '#lib/focus.ts';
	import { ui } from '#lib/ui.svelte.ts';
	import Icon from '#lib/components/Icon.svelte';
	import StatusLine from '#lib/components/StatusLine.svelte';
	import { refetchCollection } from './actions.ts';

	let { collectionId, title, releases }: { collectionId: string; title: string; releases: GoneReleaseEntry[] } = $props();
	const id = $props.id();
	const g = new Gesture();
	let heading = $state<HTMLElement>();
	let list = $state<HTMLElement>();

	function watchLine(r: GoneReleaseEntry): string | null {
		if (r.watched) return r.last_watched_at ? `Watched ${ago(r.last_watched_at)}` : 'Watched';
		const pos = r.position_seconds ?? 0;
		if (pos <= 0) return null;
		const how = progressWords(pos, r.duration_seconds);
		return r.last_watched_at ? `${how}, ${ago(r.last_watched_at)}` : how;
	}

	/** The row leaves the list: the focus goes to the next one, else the title. */
	function then(r: GoneReleaseEntry, said: string) {
		const row = list?.querySelector(`[data-infohash="${r.infohash}"]`);
		const next = row?.nextElementSibling ?? row?.previousElementSibling;
		return async () => {
			await refetchCollection(collectionId);
			ui.say(said);
			await refocus(() => next?.querySelector<HTMLElement>('button'), heading, 'main h1');
		};
	}
</script>

<section class="gone" aria-labelledby="{id}-title">
	<div class="head">
		<h2 id="{id}-title" class="group-title" tabindex="-1" bind:this={heading}>Previously on disk</h2>
		<span class="hint">{plural(releases.length, 'release')}</span>
	</div>
	<ul class="plain-list" bind:this={list}>
		{#each releases as r (r.infohash)}
			{@const line = watchLine(r)}
			<li data-infohash={r.infohash}>
				<div class="text">
					{#if line}<StatusLine tone={r.watched ? 'ok' : 'info'} text={line} />{/if}
					<p class="release">{r.name}</p>
					<p class="hint">
						{[formatSize(r.total_size_bytes), `via ${r.source_provider}`, r.deleted_at ? `removed ${ago(r.deleted_at)}` : null]
							.filter(Boolean)
							.join(' · ')}
					</p>
				</div>
				<div class="actions">
					<button
						class="btn"
						aria-label="Download again: {r.name}"
						{...pending(g.is(`again:${r.infohash}`))}
						onclick={() => {
							const done = then(r, `${r.name} is downloading again. Your watch position is kept.`);
							// a refusal went to Search: nothing left here to read again
							g.run(
								() => fetchAgainOrSearch({ infohash: r.infohash, title, name: r.name }),
								(res) => res && done(),
								`again:${r.infohash}`
							);
						}}
					>
						<Icon name="rotate-ccw" size={16} busy={g.is(`again:${r.infohash}`)} />Download again
					</button>
					<button
						class="btn ghost"
						aria-label="Hide: {r.name}"
						{...pending(g.is(`hide:${r.infohash}`))}
						onclick={() =>
							g.run(
								() => me.dismissGone({ infohash: r.infohash }),
								then(r, `${r.name} is hidden. Your history is kept.`),
								`hide:${r.infohash}`
							)}
					>
						<Icon name="x" size={16} busy={g.is(`hide:${r.infohash}`)} />Hide
					</button>
				</div>
			</li>
		{/each}
	</ul>
	<p class="hint">Hiding a release takes it off this page for you only. Your history is kept.</p>
</section>

<style>
	.gone {
		display: grid;
		gap: var(--s-3);
		max-width: var(--measure-wide);
	}
	.head {
		display: flex;
		align-items: baseline;
		gap: var(--s-3);
	}
	li {
		display: flex;
		flex-wrap: wrap;
		gap: var(--s-2) var(--s-3);
		align-items: center;
		padding: var(--s-3) 0;
		border-bottom: 1px solid var(--line);
	}
	.text {
		flex: 1 1 16rem;
		min-width: 0;
		display: grid;
		gap: var(--s-1);
	}
	.release {
		margin: 0;
		font: var(--t-secondary);
		font-family: var(--font-mono);
		overflow-wrap: anywhere;
	}
	.actions .btn {
		min-height: var(--control-h);
	}
</style>
