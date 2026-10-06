<script lang="ts">
	// The admin at a glance, one quiet line under the title: who is watching, the disk, the
	// trackers, the invitations waiting. Each fact leads to the view that says more (and to
	// its section); what needs a hand says so in words, its icon only repeats it. A fact not
	// read yet is left out rather than guessed.
	import { createQuery } from '@tanstack/svelte-query';
	import { formatSize, plural } from '@iris/api/format';
	import { queryClient } from '#lib/query.ts';
	import Icon, { type IconName } from '#lib/components/Icon.svelte';
	import { isWaiting, type View } from './model.ts';
	import { invitationsQuery, providersQuery, sessionsQuery, storageQuery } from './queries.ts';

	let { onopen }: { onopen: (view: View, section: string) => void } = $props();

	const sessions = createQuery(sessionsQuery, () => queryClient);
	const storage = createQuery(storageQuery, () => queryClient);
	const trackers = createQuery(providersQuery, () => queryClient);
	const invitations = createQuery(invitationsQuery, () => queryClient);

	interface Fact {
		text: string;
		view: View;
		section: string;
		icon: IconName;
		warn?: boolean;
	}

	/** « Alex is watching », « Alex and Sam are watching », « 4 people are watching ». */
	function watching(names: string[]): string {
		if (names.length === 0) return 'Nobody is watching';
		if (names.length === 1) return `${names[0]} is watching`;
		// a long list of names would fill the line: their count says it shorter
		if (names.length <= 3 && names.join(', ').length <= 36) return `${names.slice(0, -1).join(', ')} and ${names.at(-1)} are watching`;
		return `${names.length} people are watching`;
	}

	const facts = $derived.by(() => {
		const out: Fact[] = [];
		if (sessions.data)
			out.push({ text: watching(sessions.data.map((s) => s.display_name)), view: 'activity', section: '#watching-title', icon: 'play' });
		const s = storage.data;
		if (s) {
			const over = s.used_bytes >= s.threshold_bytes;
			const free = Math.max(0, s.max_storage_bytes - s.used_bytes);
			out.push({
				text: over ? `Disk past the clean-up line, ${formatSize(free)} free` : `${formatSize(free)} free on disk`,
				view: 'system',
				section: '#storage-title',
				icon: 'hard-drive',
				warn: over
			});
		}
		const t = trackers.data;
		if (t?.length) {
			const on = t.filter((p) => p.enabled);
			const failing = on.filter((p) => p.last_search?.error).length;
			out.push({
				text: `${on.length} of ${plural(t.length, 'tracker')} on${failing ? `, ${failing} failing` : ''}`,
				view: 'system',
				section: '#trackers-title',
				icon: 'search',
				warn: failing > 0
			});
		}
		const i = invitations.data;
		if (i) {
			const waiting = i.filter((x) => isWaiting(x)).length;
			out.push({
				text: waiting ? `${plural(waiting, 'invitation')} waiting` : 'No invitation waiting',
				view: 'people',
				section: '#invites-title',
				icon: 'users'
			});
		}
		return out;
	});
</script>

<nav class="glance" aria-label="At a glance">
	<ul class="plain-list">
		{#each facts as f (f.section)}
			<li>
				<a
					href="?view={f.view}"
					class:warn={f.warn}
					onclick={(e) => {
						e.preventDefault();
						onopen(f.view, f.section);
					}}><Icon name={f.warn ? 'triangle-alert' : f.icon} size={16} />{f.text}</a
				>
			</li>
		{/each}
	</ul>
</nav>

<style>
	.glance {
		margin: calc(-1 * var(--s-3)) 0 var(--s-4);
		min-height: var(--control-h);
	}
	ul {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		column-gap: var(--s-5);
	}
	li {
		display: inline-flex;
		align-items: center;
		gap: var(--s-2);
	}
	a {
		display: inline-flex;
		align-items: center;
		gap: var(--s-1);
		min-height: var(--control-h);
		color: var(--ink-muted);
		font: var(--t-secondary);
		text-decoration: underline;
		text-decoration-color: var(--line);
		text-underline-offset: var(--s-1);
	}
	a:hover {
		color: var(--ink);
		text-decoration-color: currentColor;
	}
	a.warn {
		color: var(--warn-text);
		font-weight: 600;
	}
</style>
