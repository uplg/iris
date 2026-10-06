<script lang="ts">
	// Invitations (an admin's), after Maison's: one press makes a link (it carries a code shown
	// once), which shows with the focus on it, ready to copy. The ones still waiting come first,
	// each with when it stops working and a revoke after asking; the used and expired ones are
	// folded behind their count, each saying who used it or when it ran out.
	import { createQuery } from '@tanstack/svelte-query';
	import { admin, type CreatedInvitation, type Invitation } from '@iris/api/client';
	import { ago, onDay, plural, until } from '@iris/api/format';
	import { loadable, queryClient } from '#lib/query.ts';
	import { ui } from '#lib/ui.svelte.ts';
	import { Gesture, pending } from '#lib/gesture.svelte.ts';
	import { refocus } from '#lib/focus.ts';
	import ConfirmDialog from '#lib/components/ConfirmDialog.svelte';
	import Disclosure from '#lib/components/Disclosure.svelte';
	import Group from '#lib/components/Group.svelte';
	import Icon from '#lib/components/Icon.svelte';
	import ListRow from '#lib/components/ListRow.svelte';
	import Loaded from '#lib/components/Loaded.svelte';
	import { isWaiting } from './model.ts';
	import { invitationsQuery, usersQuery } from './queries.ts';

	const invitations = createQuery(invitationsQuery, () => queryClient);
	const users = createQuery(usersQuery, () => queryClient);
	const value = loadable(invitations);
	const g = new Gesture();
	let title = $state<HTMLElement>();
	let created = $state.raw<(CreatedInvitation & { url: string }) | null>(null);

	const nameOf = (id: string | null | undefined) => users.data?.find((u) => u.id === id)?.display_name;
	const waiting = $derived(
		(invitations.data ?? []).filter((i) => isWaiting(i)).toSorted((a, b) => a.expires_at.localeCompare(b.expires_at))
	);
	const past = $derived(
		(invitations.data ?? [])
			.filter((i) => !isWaiting(i))
			.toSorted((a, b) => (b.consumed_at ?? b.expires_at).localeCompare(a.consumed_at ?? a.expires_at))
	);

	const made = (i: Invitation) => {
		const by = nameOf(i.created_by);
		return `Made ${ago(i.created_at)}${by ? ` by ${by}` : ''}`;
	};
	const outcome = (i: Invitation) => {
		if (!i.consumed_at) return `Expired ${ago(i.expires_at)}`;
		const who = nameOf(i.consumed_by);
		return `Used ${ago(i.consumed_at)}${who ? ` by ${who}` : ''}`;
	};

	async function create() {
		const r = await g.run(() => admin.createInvitation(), undefined, 'create');
		if (!r) return;
		created = { ...r, url: `${location.origin}/register?token=${encodeURIComponent(r.token)}` };
		void invitations.refetch();
		// the link is the outcome: said, and the focus on it, ready to copy
		ui.say('Invitation link ready to copy.');
		await refocus('#invite-url');
	}

	async function copy() {
		if (!created) return;
		try {
			await navigator.clipboard.writeText(created.url);
			ui.toast('Link copied.');
		} catch {
			// no clipboard here: the link selected, to copy by hand
			const field = document.getElementById('invite-url') as HTMLInputElement | null;
			field?.focus();
			field?.select();
		}
	}

	const revoke = (i: Invitation) =>
		g.run(
			() => admin.revokeInvitation(i.id),
			async () => {
				ui.toast('Invitation revoked.');
				if (created?.id === i.id) created = null;
				await invitations.refetch();
				await refocus(title);
			},
			`revoke:${i.id}`
		);
</script>

<Group id="invites-title" title="Invitations" fact={invitations.data ? `${waiting.length} waiting` : undefined} bind:heading={title}>
	<p class="hint">Iris is invitation-only: each link makes one account, once. A link is shown only when it is made.</p>
	<div>
		<button class="btn primary" {...pending(g.is('create'))} onclick={create}
			><Icon name="plus" busy={g.is('create')} />Create an invitation link</button
		>
	</div>
	{#if created}
		<div class="callout" role="group" aria-labelledby="created-title">
			<p id="created-title"><strong>Invitation link</strong> · Works once, until {onDay(created.expires_at).replace(/^on /, '')}</p>
			<div class="inline">
				<label class="sr-only" for="invite-url">Invitation link</label>
				<input id="invite-url" readonly value={created.url} onfocus={(e) => e.currentTarget.select()} />
				<button class="btn" onclick={copy}><Icon name="copy" />Copy the link</button>
			</div>
			<p class="hint">Or give them this code to type on the sign-up page: <code>{created.token}</code>. It is shown only now.</p>
		</div>
	{/if}
	<Loaded {value} empty={invitations.data?.length === 0} emptyText="No invitations yet.">
		{#if waiting.length}
			<ul class="plain-list">
				{#each waiting as i (i.id)}
					<ListRow second={made(i)}>
						<span class="chip accent"><Icon name="clock" size={12} />Waiting</span>
						<span>Stops working {until(i.expires_at)}</span>
						{#snippet end()}
							<ConfirmDialog
								ghost
								danger
								label="Revoke"
								ariaLabel="Revoke the invitation made {ago(i.created_at)}"
								title="Revoke this invitation?"
								description="Its link stops working at once. Whoever has it cannot make an account with it."
								action="Revoke the invitation"
								busy={g.is(`revoke:${i.id}`)}
								onconfirm={() => revoke(i)}
							/>
						{/snippet}
					</ListRow>
				{/each}
			</ul>
		{:else}
			<p class="hint">No invitation is waiting.</p>
		{/if}
		{#if past.length}
			<Disclosure label={plural(past.length, 'used or expired invitation', 'used or expired invitations')}>
				<ul class="plain-list">
					{#each past as i (i.id)}
						<ListRow second={made(i)}>
							<span class="chip"><Icon name={i.consumed_at ? 'circle-check' : 'ban'} size={12} />{i.consumed_at ? 'Used' : 'Expired'}</span>
							<span>{outcome(i)}</span>
						</ListRow>
					{/each}
				</ul>
			</Disclosure>
		{/if}
	</Loaded>
</Group>

<style>
	.inline {
		display: flex;
		gap: var(--s-2);
		flex-wrap: wrap;
	}
	.inline input {
		flex: 1 1 14rem;
		min-width: 0;
	}
	.inline .btn {
		min-height: var(--control-h);
	}
	code {
		font-family: var(--font-mono);
		overflow-wrap: anywhere;
	}
</style>
