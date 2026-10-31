<script lang="ts">
	// The people sharing this account: each has their own history, watchlist and preferences.
	// Removing one asks first; what it grabbed stays in the library.
	import { createQuery } from '@tanstack/svelte-query';
	import { MAX_NAME, profiles, type ProfileView } from '@iris/api/client';
	import { plural } from '@iris/api/format';
	import { KEYS, read } from '#lib/queries.ts';
	import { loadable, queryClient } from '#lib/query.ts';
	import { Gesture, pending } from '#lib/gesture.svelte.ts';
	import { ui } from '#lib/ui.svelte.ts';
	import { enter } from '#lib/profiles/enter.ts';
	import ConfirmDialog from '#lib/components/ConfirmDialog.svelte';
	import Group from '#lib/components/Group.svelte';
	import Icon from '#lib/components/Icon.svelte';
	import ListRow from '#lib/components/ListRow.svelte';
	import Loaded from '#lib/components/Loaded.svelte';
	import Mascot from '#lib/components/Mascot.svelte';

	const list = createQuery(
		() => read.profiles(),
		() => queryClient
	);
	const value = loadable(list);
	const g = new Gesture();
	let name = $state('');
	let field = $state<HTMLInputElement>();

	function add(e: SubmitEvent) {
		e.preventDefault();
		const wanted = name.trim();
		if (!wanted) return field?.focus();
		return g.run(
			() => profiles.create(wanted),
			(next) => {
				queryClient.setQueryData(KEYS.profiles, next);
				name = '';
				ui.toast(`Profile ${wanted} added.`);
			},
			'add',
			{ field: () => field }
		);
	}

	const remove = (p: ProfileView) =>
		g.run(
			() => profiles.remove(p.id),
			async () => {
				ui.toast(`Profile ${p.name} removed.`);
				await list.refetch();
			},
			`remove:${p.id}`
		);

	const standing = (p: ProfileView) => [p.main && 'Main profile', p.current && 'In use'].filter(Boolean).join(' · ');
</script>

<Group id="profiles-title" title="Profiles" fact={list.data?.length ? plural(list.data.length, 'profile') : undefined}>
	<p class="hint">For people sharing this account: each profile keeps its own history, watchlist and preferences.</p>
	<Loaded {value}>
		<ul class="plain-list">
			{#each list.data ?? [] as p (p.id)}
				<ListRow second={standing(p) || undefined}>
					<Mascot name={p.name} /><span>{p.name}</span>
					{#snippet end()}
						{#if !p.current}
							<button class="btn" {...pending(g.is(`enter:${p.id}`))} onclick={() => g.run(() => enter(p), undefined, `enter:${p.id}`)}
								>Switch</button
							>
						{/if}
						{#if !p.main && !p.current}
							<ConfirmDialog
								label="Remove"
								ariaLabel="Remove the profile {p.name}"
								ghost
								danger
								title="Remove {p.name}?"
								description="Its history, watchlist and preferences go. What it added stays in the library."
								action="Remove the profile"
								busy={g.is(`remove:${p.id}`)}
								onconfirm={() => remove(p)}
							/>
						{/if}
					{/snippet}
				</ListRow>
			{/each}
		</ul>
	</Loaded>
	<form class="add" onsubmit={add} novalidate>
		<div class="field">
			<label for="profile-name">New profile</label>
			<input
				id="profile-name"
				bind:this={field}
				bind:value={name}
				autocomplete="off"
				maxlength={MAX_NAME}
				aria-describedby="profile-error"
			/>
			<p class="form-error" id="profile-error">{g.error}</p>
		</div>
		<button class="btn" {...pending(g.is('add'))}><Icon name="plus" busy={g.is('add')} />Add</button>
	</form>
</Group>

<style>
	.add {
		display: flex;
		flex-wrap: wrap;
		align-items: start;
		gap: var(--s-3);
	}
	.add .field {
		flex: 1 1 12rem;
	}
	.add .btn {
		margin-top: var(--s-5);
	}
</style>
