<script lang="ts">
	// One person, for an admin: who they are (email, role, since when, how much they watch),
	// then what they watched, the same rows as the household's history, read only (nothing is
	// downloaded again from here).
	import { createQuery } from '@tanstack/svelte-query';
	import { ago, plural } from '@iris/api/format';
	import { queryClient } from '#lib/query.ts';
	import Icon from '#lib/components/Icon.svelte';
	import PageHead from '#lib/components/PageHead.svelte';
	import PlayHistory from './PlayHistory.svelte';
	import { playName } from '#lib/history/words.ts';
	import { sessionsQuery, usersQuery } from './queries.ts';

	let { userId }: { userId: string } = $props();

	const users = createQuery(usersQuery, () => queryClient);
	const who = $derived(users.data?.find((u) => u.id === userId));
	const gone = $derived(!!users.data && !who);
	const sessions = createQuery(sessionsQuery, () => queryClient);
	const live = $derived(sessions.data?.find((s) => s.user_id === userId));
	const playing = $derived(live ? Object.values(playName(live)).filter(Boolean).join(', ') : null);
	const facts = $derived(
		who
			? [
					who.email,
					who.is_admin ? 'Admin' : null,
					`Joined ${ago(who.created_at)}`,
					who.last_played_at ? `Last played ${ago(who.last_played_at)}, ${plural(who.plays ?? 0, 'play')} in all` : 'Never played anything'
				]
					.filter(Boolean)
					.join(' · ')
			: null
	);
</script>

<a class="back link-btn quiet" href="/admin"><Icon name="arrow-left" />Admin</a>
<PageHead title={who?.display_name ?? 'A person'}>
	{#snippet sub()}{facts ?? (gone ? 'This account no longer exists.' : 'Loading…')}{/snippet}
</PageHead>
{#if playing && live}
	<p class="live">
		<span class="chip accent"
			><Icon name={live.state === 'playing' ? 'play' : 'pause'} size={12} />{live.state === 'playing' ? 'Playing now' : 'Paused now'}</span
		>
		<span>{playing}</span>
	</p>
{/if}
{#if !gone}
	<PlayHistory {userId} />
{/if}

<style>
	.live {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: var(--s-2);
		margin: calc(-1 * var(--s-2)) 0 var(--s-5);
	}
	.back {
		display: inline-flex;
		align-items: center;
		gap: var(--s-2);
		min-height: var(--control-h);
		margin-top: var(--s-3);
	}
</style>
