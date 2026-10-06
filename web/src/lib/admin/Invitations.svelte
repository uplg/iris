<script lang="ts">
	// Invitations (an admin's), after Maison's: one press makes a link (it carries a code shown
	// once), which shows with the focus on it, ready to copy; every invitation says where it is
	// in words (waiting, used and by whom, expired); a waiting one is revoked after asking.
	import { createQuery } from '@tanstack/svelte-query';
	import { admin, type CreatedInvitation, type Invitation } from '@iris/api/client';
	import { loadable, queryClient } from '#lib/query.ts';
	import { ui } from '#lib/ui.svelte.ts';
	import { Gesture, pending } from '#lib/gesture.svelte.ts';
	import { refocus } from '#lib/focus.ts';
	import ConfirmDialog from '#lib/components/ConfirmDialog.svelte';
	import Group from '#lib/components/Group.svelte';
	import Icon from '#lib/components/Icon.svelte';
	import ListRow from '#lib/components/ListRow.svelte';
	import Loaded from '#lib/components/Loaded.svelte';
	import { onDay } from '@iris/api/format';
	import { invitationsQuery, usersQuery } from './queries.ts';

	const invitations = createQuery(invitationsQuery, () => queryClient);
	const users = createQuery(usersQuery, () => queryClient);
	const value = loadable(invitations);
	const g = new Gesture();
	let title = $state<HTMLElement>();
	let created = $state.raw<(CreatedInvitation & { url: string }) | null>(null);

	const nameOf = (id: string | null | undefined) => users.data?.find((u) => u.id === id)?.display_name;
	const stage = (i: Invitation): 'used' | 'expired' | 'waiting' =>
		i.consumed_at ? 'used' : new Date(i.expires_at).getTime() < Date.now() ? 'expired' : 'waiting';
	const waiting = $derived((invitations.data ?? []).filter((i) => stage(i) === 'waiting').length);

	const made = (i: Invitation) => {
		const by = nameOf(i.created_by);
		return `Made ${onDay(i.created_at)}${by ? ` by ${by}` : ''}`;
	};
	function facts(i: Invitation): string {
		switch (stage(i)) {
			case 'used': {
				const who = nameOf(i.consumed_by);
				return `Used ${onDay(i.consumed_at!)}${who ? ` by ${who}` : ''}`;
			}
			case 'expired':
				return `Expired ${onDay(i.expires_at)}`;
			default:
				return `Works until ${onDay(i.expires_at).replace(/^on /, '')}`;
		}
	}
	const WORDS = { used: 'Used', expired: 'Expired', waiting: 'Waiting' } as const;

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

<Group id="invites-title" title="Invitations" fact={invitations.data ? `${waiting} waiting` : undefined} bind:heading={title}>
	<p class="hint">Iris is invitation-only: each link makes one account, once.</p>
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
		<ul class="plain-list">
			{#each invitations.data ?? [] as i (i.id)}
				{@const s = stage(i)}
				<ListRow second={facts(i)}>
					<span class={['chip', s === 'waiting' && 'accent']}>
						<Icon name={s === 'waiting' ? 'clock' : s === 'used' ? 'circle-check' : 'ban'} size={12} />{WORDS[s]}
					</span>
					<span>{made(i)}</span>
					{#snippet end()}
						{#if s === 'waiting'}
							<ConfirmDialog
								ghost
								danger
								label="Revoke"
								ariaLabel="Revoke the invitation made {onDay(i.created_at)}"
								title="Revoke this invitation?"
								description="Its link stops working at once. Whoever has it cannot make an account with it."
								action="Revoke the invitation"
								busy={g.is(`revoke:${i.id}`)}
								onconfirm={() => revoke(i)}
							/>
						{/if}
					{/snippet}
				</ListRow>
			{/each}
		</ul>
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
