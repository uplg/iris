<script lang="ts">
	// Who changed or deleted what (an admin's), kept by the server, the latest 50, read again
	// every 30 s: who, what in words (an action not listed here keeps its own name), when.
	import { createQuery } from '@tanstack/svelte-query';
	import { plural } from '@iris/api/format';
	import type { AuditLogEntry } from '@iris/api/client';
	import { loadable, queryClient } from '#lib/query.ts';
	import Group from '#lib/components/Group.svelte';
	import ListRow from '#lib/components/ListRow.svelte';
	import Loaded from '#lib/components/Loaded.svelte';
	import { onDay } from '#lib/history/words.ts';
	import { auditQuery } from './queries.ts';

	const ACTIONS: Record<string, string> = {
		'torrent.delete': 'deleted a release',
		'user.password_reset': 'set a new password',
		'user.display_name_update': 'changed a display name',
		'user.delete': 'deleted an account',
		'gc.evict': 'freed disk space',
		'remux.wipe': 'deleted a prepared copy'
	};
	const log = createQuery(auditQuery, () => queryClient);
	const value = loadable(log);
	const facts = (e: AuditLogEntry) => [e.details, onDay(e.created_at)].filter(Boolean).join(' · ');
</script>

<Group id="audit-title" title="Audit log" fact={log.data?.length ? plural(log.data.length, 'entry', 'entries') : undefined}>
	<p class="hint">Deletions, password changes and clean-ups, and who made them.</p>
	<Loaded {value} empty={log.data?.length === 0} emptyText="Nothing recorded yet.">
		<ul class="plain-list log">
			{#each log.data ?? [] as e (e.id)}
				<ListRow second={facts(e)}>
					<span><strong>{e.actor_display_name}</strong> {ACTIONS[e.action] ?? e.action}</span>
				</ListRow>
			{/each}
		</ul>
	</Loaded>
</Group>

<style>
	.log :global(.list-row) {
		content-visibility: auto;
		contain-intrinsic-size: auto 4rem;
	}
	.log :global(.second) {
		overflow-wrap: anywhere;
	}
</style>
