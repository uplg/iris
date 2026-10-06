<script lang="ts">
	// The household's accounts (an admin's): each person, admin and « you » said in words, with
	// their watch history, a new display name in place, a new password (their sessions end), and
	// removal after asking (never oneself; their grabs stay in the library, re-attributed).
	import { createQuery } from '@tanstack/svelte-query';
	import { onDay, plural } from '@iris/api/format';
	import { admin, auth, type UserView } from '@iris/api/client';
	import { loadable, queryClient } from '#lib/query.ts';
	import { session } from '#lib/session.svelte.ts';
	import { ui } from '#lib/ui.svelte.ts';
	import { Gesture, pending } from '#lib/gesture.svelte.ts';
	import { refocus } from '#lib/focus.ts';
	import ConfirmDialog from '#lib/components/ConfirmDialog.svelte';
	import Group from '#lib/components/Group.svelte';
	import Icon from '#lib/components/Icon.svelte';
	import ListRow from '#lib/components/ListRow.svelte';
	import Loaded from '#lib/components/Loaded.svelte';
	import RenameField from '#lib/components/RenameField.svelte';
	import Sheet from '#lib/components/Sheet.svelte';
	import { invitationsQuery, usersQuery } from './queries.ts';

	const MIN = 8;
	const users = createQuery(usersQuery, () => queryClient);
	const value = loadable(users);
	const g = new Gesture();
	let title = $state<HTMLElement>();
	let filter = $state('');
	let renaming = $state<string | null>(null);
	let resetting = $state<UserView | null>(null);
	let password = $state('');
	let reveal = $state(false);
	let passwordField = $state<HTMLInputElement>();
	let invalid = $state('');

	const q = $derived(filter.trim().toLowerCase());
	const shown = $derived(
		(users.data ?? []).filter((u) => !q || u.display_name.toLowerCase().includes(q) || u.email.toLowerCase().includes(q))
	);
	const isMe = (u: UserView) => u.id === session.user?.id;

	async function rename(u: UserView, name: string) {
		await admin.setDisplayName(u.id, name);
		await users.refetch();
		// my own name: the header shows the server's
		if (isMe(u)) session.signedIn(await auth.me());
	}

	async function closeRename(u: UserView) {
		renaming = null;
		await refocus(() => document.getElementById(`rename-user-${u.id}`));
	}

	function closeReset() {
		const u = resetting;
		resetting = null;
		password = '';
		invalid = '';
		reveal = false;
		if (u) void refocus(() => document.getElementById(`reset-${u.id}`));
	}

	function reset(e: SubmitEvent) {
		e.preventDefault();
		const u = resetting;
		if (!u) return;
		invalid = '';
		if (password.length < MIN) {
			invalid = `Use at least ${MIN} characters.`;
			return passwordField?.focus();
		}
		return g.run(
			() => admin.resetPassword(u.id, password),
			() => {
				closeReset();
				ui.toast(`New password set for ${u.display_name}. Their devices were signed out; give them the new password.`);
			},
			'reset',
			{ field: () => passwordField }
		);
	}

	const remove = (u: UserView) =>
		g.run(
			() => admin.deleteUser(u.id),
			async () => {
				ui.toast(`${u.display_name}’s account deleted.`);
				void queryClient.invalidateQueries({ queryKey: invitationsQuery().queryKey });
				await users.refetch();
				await refocus(title);
			},
			`delete:${u.id}`
		);

	const problem = $derived(invalid || g.error);
</script>

<Group id="users-title" title="Users" fact={users.data ? plural(users.data.length, 'account') : undefined} bind:heading={title}>
	{#if (users.data?.length ?? 0) > 1}
		<div class="field find">
			<label for="users-filter">Find a person</label>
			<input id="users-filter" type="search" bind:value={filter} autocomplete="off" aria-describedby="users-count" />
			<p class="hint" id="users-count">
				{q ? `${plural(shown.length, 'match', 'matches')} of ${users.data?.length}` : 'By name or email.'}
			</p>
		</div>
	{/if}
	<Loaded {value} empty={users.data?.length === 0} emptyText="No accounts yet.">
		{#if q && shown.length === 0}
			<div class="empty"><p>No one matches “{filter.trim()}”.</p></div>
		{/if}
		<ul class="plain-list">
			{#each shown as u (u.id)}
				{#snippet renameField()}
					<RenameField
						label="Display name of {u.display_name}"
						hideLabel
						autofocus
						value={u.display_name}
						save={(name) => rename(u, name)}
						said={(name) => `Renamed to ${name}.`}
						ondone={() => closeRename(u)}
					/>
				{/snippet}
				<ListRow whole={renaming === u.id ? renameField : undefined} second="{u.email} · Joined {onDay(u.created_at)}">
					<span>{u.display_name}</span>
					{#if u.is_admin}<span class="chip accent"><Icon name="shield-check" size={12} />Admin</span>{/if}
					{#if isMe(u)}<span class="chip">You</span>{/if}
					{#snippet end()}
						<a class="btn ghost" href="/admin/users/{u.id}/history" aria-label="Watch history of {u.display_name}"
							><Icon name="history" />History</a
						>
						<button class="btn ghost" id="rename-user-{u.id}" aria-label="Rename {u.display_name}" onclick={() => (renaming = u.id)}
							>Rename</button
						>
						<button
							class="btn ghost"
							id="reset-{u.id}"
							aria-haspopup="dialog"
							aria-label="Set a new password for {u.display_name}"
							onclick={() => (resetting = u)}>New password</button
						>
						{#if !isMe(u)}
							<ConfirmDialog
								ghost
								danger
								label="Delete"
								ariaLabel="Delete the account of {u.display_name}"
								title="Delete {u.display_name}’s account?"
								description="Their sessions, watch history, follows and preferences are deleted for good. What they downloaded stays in the shared library, credited to you."
								action="Delete the account"
								busy={g.is(`delete:${u.id}`)}
								onconfirm={() => remove(u)}
							/>
						{/if}
					{/snippet}
				</ListRow>
			{/each}
		</ul>
	</Loaded>
</Group>

<Sheet
	open={resetting !== null}
	onclose={closeReset}
	title="New password for {resetting?.display_name ?? ''}"
	description="For {resetting?.email ?? ''}. Every device they use is signed out; they sign in again with this password."
>
	<form class="reset" onsubmit={reset} novalidate>
		<input type="email" name="username" autocomplete="off" value={resetting?.email ?? ''} readonly hidden />
		<div class="field">
			<label for="reset-password">New password</label>
			<div class="reveal-row">
				<input
					id="reset-password"
					type={reveal ? 'text' : 'password'}
					bind:this={passwordField}
					bind:value={password}
					autocomplete="new-password"
					aria-invalid={problem ? 'true' : undefined}
					aria-describedby="reset-password-hint reset-password-error"
				/>
				<button type="button" class="btn ghost" aria-pressed={reveal} aria-controls="reset-password" onclick={() => (reveal = !reveal)}
					>{reveal ? 'Hide' : 'Show'}</button
				>
			</div>
			<p class="hint" id="reset-password-hint">At least {MIN} characters.</p>
			<p class="form-error" id="reset-password-error">{problem}</p>
		</div>
		<div class="actions end">
			<button class="btn" type="button" onclick={closeReset}>Cancel</button>
			<button class="btn primary" {...pending(g.is('reset'))}><Icon name="key" busy={g.is('reset')} />Set the new password</button>
		</div>
	</form>
</Sheet>

<style>
	.find {
		max-width: 22rem;
	}
	.reset {
		display: grid;
		gap: var(--s-4);
	}
	.reveal-row {
		display: flex;
		gap: var(--s-2);
	}
	.reveal-row input {
		flex: 1;
		min-width: 0;
	}
	.reset .btn {
		min-height: var(--control-h);
	}
</style>
