<script lang="ts">
	// Friends, profile by profile: found by name, asked, answered. Declining tells the other
	// person nothing. What one adds is visible to friends; what one watches only once shared,
	// never a title hidden from them.
	import { createQuery } from '@tanstack/svelte-query';
	import { friends, type HiddenTitleView, type PersonView } from '@iris/api/client';
	import { plural } from '@iris/api/format';
	import { KEYS, read } from '#lib/queries.ts';
	import { loadable, queryClient } from '#lib/query.ts';
	import { Gesture, pending } from '#lib/gesture.svelte.ts';
	import { ui } from '#lib/ui.svelte.ts';
	import ConfirmDialog from '#lib/components/ConfirmDialog.svelte';
	import Group from '#lib/components/Group.svelte';
	import Icon from '#lib/components/Icon.svelte';
	import ListRow from '#lib/components/ListRow.svelte';
	import Loaded from '#lib/components/Loaded.svelte';
	import ToggleGroup from '#lib/components/ToggleGroup.svelte';

	const SHARING = [
		{ value: 'off', label: 'Keep it private' },
		{ value: 'on', label: 'Share with friends' }
	] as const;

	const list = createQuery(
		() => read.friends(),
		() => queryClient
	);
	const value = loadable(list);
	const g = new Gesture();
	let name = $state('');
	let found = $state<PersonView[] | null>(null);
	let field = $state<HTMLInputElement>();

	/** Answered, then read again: this list and the header's count. */
	const act = (key: string, send: () => Promise<unknown>, said: string) =>
		g.run(
			send,
			async () => {
				ui.toast(said);
				await Promise.all([list.refetch(), queryClient.invalidateQueries({ queryKey: KEYS.summary })]);
			},
			key
		);

	function find(e: SubmitEvent) {
		e.preventDefault();
		const wanted = name.trim();
		if (wanted.length < 2) return field?.focus();
		return g.run(
			() => friends.search(wanted),
			(people) => (found = people),
			'find',
			{ field: () => field }
		);
	}

	const ask = async (p: PersonView) => {
		await act(`ask:${p.id}`, () => friends.ask(p.id), `Request sent to ${p.name}.`);
		found = found?.filter((f) => f.id !== p.id) ?? null;
	};

	const share = (next: 'on' | 'off') =>
		act(
			'share',
			() => friends.setSharing(next === 'on'),
			next === 'on' ? 'Friends can see what you watch.' : 'What you watch stays private.'
		);
	const showAgain = (h: HiddenTitleView) => act(`show:${h.title}`, () => friends.unhide(h), `${h.title} is visible to friends again.`);
</script>

<Group id="friends-title" title="Friends" fact={list.data?.friends.length ? plural(list.data.friends.length, 'friend') : undefined}>
	<p class="hint">Friends see what you add to the library. What you watch stays private until you share it below.</p>
	<Loaded {value}>
		{#if list.data?.incoming.length}
			<h3 class="eyebrow">Asked you</h3>
			<ul class="plain-list">
				{#each list.data.incoming as p (p.id)}
					<ListRow>
						{p.name}
						{#snippet end()}
							<button
								class="btn primary"
								{...pending(g.is(`accept:${p.id}`))}
								onclick={() => act(`accept:${p.id}`, () => friends.accept(p.id), `You and ${p.name} are friends.`)}>Accept</button
							>
							<button
								class="btn ghost"
								aria-label="Decline {p.name}"
								{...pending(g.is(`decline:${p.id}`))}
								onclick={() => act(`decline:${p.id}`, () => friends.decline(p.id), `Request from ${p.name} declined.`)}>Decline</button
							>
						{/snippet}
					</ListRow>
				{/each}
			</ul>
		{/if}
		{#if list.data?.friends.length}
			<ul class="plain-list">
				{#each list.data.friends as p (p.id)}
					<ListRow>
						{p.name}
						{#snippet end()}
							<ConfirmDialog
								ghost
								danger
								label="Remove"
								ariaLabel="Remove {p.name}"
								title="Remove {p.name}?"
								description="You stop seeing each other’s activity. They are not told."
								action="Remove the friend"
								busy={g.is(`remove:${p.id}`)}
								onconfirm={() => act(`remove:${p.id}`, () => friends.remove(p.id), `${p.name} removed.`)}
							/>
						{/snippet}
					</ListRow>
				{/each}
			</ul>
		{/if}
		{#if list.data?.outgoing.length}
			<h3 class="eyebrow">Waiting for an answer</h3>
			<ul class="plain-list">
				{#each list.data.outgoing as p (p.id)}
					<ListRow>
						{p.name}
						{#snippet end()}
							<button
								class="btn ghost"
								aria-label="Take back the request to {p.name}"
								{...pending(g.is(`back:${p.id}`))}
								onclick={() => act(`back:${p.id}`, () => friends.remove(p.id), `Request to ${p.name} taken back.`)}>Take back</button
							>
						{/snippet}
					</ListRow>
				{/each}
			</ul>
		{/if}
	</Loaded>

	<form class="find" onsubmit={find} novalidate>
		<div class="field">
			<label for="friend-name">Find someone by name</label>
			<input id="friend-name" bind:this={field} bind:value={name} autocomplete="off" aria-describedby="friend-error" />
			<p class="form-error" id="friend-error">{g.error}</p>
		</div>
		<button class="btn" {...pending(g.is('find'))}><Icon name="search" busy={g.is('find')} />Find</button>
	</form>
	{#if found}
		{#if found.length}
			<ul class="plain-list">
				{#each found as p (p.id)}
					<ListRow>
						{p.name}
						{#snippet end()}
							<button class="btn" aria-label="Ask {p.name}" {...pending(g.is(`ask:${p.id}`))} onclick={() => ask(p)}>Ask</button>
						{/snippet}
					</ListRow>
				{/each}
			</ul>
		{:else}
			<p class="hint">Nobody new by that name.</p>
		{/if}
	{/if}

	{#if list.data}
		<ToggleGroup
			type="single"
			label="What you watch"
			options={SHARING}
			value={list.data.share_watching ? 'on' : 'off'}
			busy={g.is('share')}
			onchange={(v) => share(v)}
		/>
		{#if list.data.hidden.length}
			<h3 class="eyebrow">Hidden from friends</h3>
			<ul class="plain-list">
				{#each list.data.hidden as h (h.collection_id ?? h.infohash)}
					<ListRow>
						{h.title}
						{#snippet end()}
							<button
								class="btn ghost"
								aria-label="Show {h.title} to friends again"
								{...pending(g.is(`show:${h.title}`))}
								onclick={() => showAgain(h)}>Show again</button
							>
						{/snippet}
					</ListRow>
				{/each}
			</ul>
		{/if}
	{/if}
</Group>

<style>
	h3 {
		margin: 0;
	}
	.find {
		display: flex;
		flex-wrap: wrap;
		align-items: start;
		gap: var(--s-3);
	}
	.find .field {
		flex: 1 1 12rem;
	}
	.find .btn {
		margin-top: var(--s-5);
	}
</style>
