<script lang="ts">
	// The household's accounts (an admin's), the most recently active first: each person's name
	// (it opens what they watched), admin and « you » said in words, when they last played. Their
	// actions sit behind one menu: a new display name in place, a new password (their sessions
	// end), removal after asking (never oneself; their grabs stay in the library, re-attributed).
	// Past ten accounts, a search; a long list shows its first ten until asked for all.
	import { createQuery } from '@tanstack/svelte-query';
	import { DropdownMenu } from 'bits-ui';
	import { ago, plural } from '@iris/api/format';
	import { admin, auth, type UserView } from '@iris/api/client';
	import { loadable, queryClient } from '#lib/query.ts';
	import { session } from '#lib/session.svelte.ts';
	import { ui } from '#lib/ui.svelte.ts';
	import { Gesture, pending } from '#lib/gesture.svelte.ts';
	import { refocus } from '#lib/focus.ts';
	import { personHref } from '#lib/paths.ts';
	import ConfirmDialog from '#lib/components/ConfirmDialog.svelte';
	import Group from '#lib/components/Group.svelte';
	import Icon from '#lib/components/Icon.svelte';
	import ListRow from '#lib/components/ListRow.svelte';
	import Loaded from '#lib/components/Loaded.svelte';
	import RenameField from '#lib/components/RenameField.svelte';
	import Sheet from '#lib/components/Sheet.svelte';
	import ShowMore from './ShowMore.svelte';
	import { invitationsQuery, usersQuery } from './queries.ts';

	const MIN = 8;
	/** Accounts shown before « Show all », and past which a search helps. */
	const FIRST = 10;
	const users = createQuery(usersQuery, () => queryClient);
	const value = loadable(users);
	const g = new Gesture();
	let title = $state<HTMLElement>();
	let filter = $state('');
	let all = $state(false);
	let renaming = $state<string | null>(null);
	let resetting = $state<UserView | null>(null);
	let deleting = $state<UserView | null>(null);
	let confirming = $state(false);
	let password = $state('');
	let reveal = $state(false);
	let passwordField = $state<HTMLInputElement>();
	let invalid = $state('');

	const total = $derived(users.data?.length ?? 0);
	const q = $derived(filter.trim().toLowerCase());
	const latest = (u: UserView) => (u.last_played_at ? new Date(u.last_played_at).getTime() : 0);
	const sorted = $derived((users.data ?? []).toSorted((a, b) => latest(b) - latest(a) || a.display_name.localeCompare(b.display_name)));
	const matches = $derived(sorted.filter((u) => !q || u.display_name.toLowerCase().includes(q) || u.email.toLowerCase().includes(q)));
	const shown = $derived(q || all ? matches : matches.slice(0, FIRST));
	const isMe = (u: UserView) => u.id === session.user?.id;
	const played = (u: UserView) =>
		u.last_played_at ? `Last played ${ago(u.last_played_at)} · ${plural(u.plays ?? 0, 'play')}` : 'Never played anything';

	async function rename(u: UserView, name: string) {
		await admin.setDisplayName(u.id, name);
		await users.refetch();
		// my own name: the header shows the server's
		if (isMe(u)) session.signedIn(await auth.me());
	}

	async function closeRename(u: UserView) {
		renaming = null;
		await refocus(() => document.getElementById(`user-menu-${u.id}`));
	}

	function closeReset() {
		const u = resetting;
		resetting = null;
		password = '';
		invalid = '';
		reveal = false;
		if (u) void refocus(() => document.getElementById(`user-menu-${u.id}`));
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

<Group id="users-title" title="Accounts" fact={users.data ? plural(total, 'account') : undefined} bind:heading={title}>
	{#if total > FIRST}
		<div class="field find">
			<label for="users-filter">Find a person</label>
			<input id="users-filter" type="search" bind:value={filter} autocomplete="off" aria-describedby="users-count" />
			<p class="hint" id="users-count">
				{q ? `${plural(matches.length, 'match', 'matches')} of ${total}` : 'By name or email. The most recently active first.'}
			</p>
		</div>
	{/if}
	<Loaded {value} empty={total === 0} emptyText="No accounts yet.">
		{#if q && matches.length === 0}
			<div class="empty"><p>No one matches “{filter.trim()}”.</p></div>
		{/if}
		<ul class="plain-list people">
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
				<ListRow whole={renaming === u.id ? renameField : undefined} second="{u.email} · {played(u)}">
					<a class="name" href={personHref(u.id)}>{u.display_name}</a>
					{#if u.is_admin}<span class="chip accent"><Icon name="shield-check" size={12} />Admin</span>{/if}
					{#if isMe(u)}<span class="chip">You</span>{/if}
					{#snippet end()}
						<DropdownMenu.Root>
							<DropdownMenu.Trigger
								class="icon-btn row-menu"
								id="user-menu-{u.id}"
								aria-label="Actions for {u.display_name}"
								{...pending(g.is(`delete:${u.id}`))}
							>
								<Icon name="ellipsis" busy={g.is(`delete:${u.id}`)} />
							</DropdownMenu.Trigger>
							<DropdownMenu.Portal>
								<DropdownMenu.Content class="menu" align="end" sideOffset={4}>
									<DropdownMenu.Item class="menu-item" onSelect={() => (renaming = u.id)}><Icon name="pen" />Rename</DropdownMenu.Item>
									<DropdownMenu.Item class="menu-item" onSelect={() => (resetting = u)}
										><Icon name="key" />Set a new password</DropdownMenu.Item
									>
									{#if !isMe(u)}
										<DropdownMenu.Separator class="menu-separator" />
										<DropdownMenu.Item
											class="menu-item danger"
											onSelect={() => {
												deleting = u;
												confirming = true;
											}}><Icon name="trash-2" />Delete the account</DropdownMenu.Item
										>
									{/if}
								</DropdownMenu.Content>
							</DropdownMenu.Portal>
						</DropdownMenu.Root>
					{/snippet}
				</ListRow>
			{/each}
		</ul>
		{#if !q}
			<ShowMore more={!all && matches.length > FIRST} label="Show all {plural(total, 'person', 'people')}" onmore={() => (all = true)} />
		{/if}
	</Loaded>
</Group>

<ConfirmDialog
	bind:open={
		() => confirming,
		(o) => {
			confirming = o;
			if (!o && deleting) void refocus(document.getElementById(`user-menu-${deleting.id}`));
		}
	}
	title="Delete {deleting?.display_name ?? ''}’s account?"
	description="Their sessions, watch history, follows and preferences are deleted for good. What they downloaded stays in the shared library, credited to you."
	action="Delete the account"
	onconfirm={() => deleting && remove(deleting)}
/>

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
	.name {
		color: var(--ink);
		font-weight: 600;
		text-decoration: none;
		min-height: var(--control-h-xs);
		display: inline-flex;
		align-items: center;
		overflow-wrap: anywhere;
	}
	.name:hover {
		text-decoration: underline;
	}
	.people :global(.second) {
		overflow-wrap: anywhere;
	}
	:global(.row-menu) {
		width: var(--control-h);
		height: var(--control-h);
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
