<script lang="ts">
	// My passkeys (FIDO « create, view and manage passkeys in account settings »), after
	// Maison's: one row per passkey (its name, « Synced » when it travels with my other
	// devices, when it was added and last used), renamed in place, removed after asking (the
	// focus then on the list's title). Passkeys are optional here: the password keeps working,
	// so the last one can go too. « Add a passkey » only where the browser can make one.
	import { createQuery } from '@tanstack/svelte-query';
	import { passkeys, register, supported, type PasskeyView } from '@iris/api/passkeys';
	import { loadable, queryClient } from '#lib/query.ts';
	import { ui } from '#lib/ui.svelte.ts';
	import { Gesture, pending } from '#lib/gesture.svelte.ts';
	import { refocus } from '#lib/focus.ts';
	import ConfirmDialog from '#lib/components/ConfirmDialog.svelte';
	import Group from '#lib/components/Group.svelte';
	import Icon from '#lib/components/Icon.svelte';
	import ListRow from '#lib/components/ListRow.svelte';
	import Loaded from '#lib/components/Loaded.svelte';
	import RenameField from '#lib/components/RenameField.svelte';
	import { count, onDay } from '#lib/history/words.ts';

	const keys = createQuery(
		() => ({ queryKey: ['passkeys'], queryFn: passkeys.list }),
		() => queryClient
	);
	const value = loadable(keys);
	const g = new Gesture();
	let renaming = $state<string | null>(null);
	let title = $state<HTMLElement>();
	const keyName = (k: PasskeyView) => k.name || 'Unnamed passkey';
	const facts = (k: PasskeyView) =>
		`Added ${onDay(k.created_at)} · ${k.last_used_at ? `Last used ${onDay(k.last_used_at)}` : 'Never used yet'}`;

	async function closeRename(k: PasskeyView) {
		renaming = null;
		await refocus(() => document.getElementById(`rename-${k.id}`));
	}

	const add = () =>
		g.run(
			() => register(),
			async (pk) => {
				ui.toast(`Passkey added as “${keyName(pk)}”.`);
				await keys.refetch();
			},
			'add'
		);

	const remove = (k: PasskeyView) =>
		g.run(
			() => passkeys.remove(k.id),
			async () => {
				ui.toast(`Passkey “${keyName(k)}” removed.`);
				await keys.refetch();
				await refocus(title);
			},
			`remove:${k.id}`
		);
</script>

<Group id="keys-title" title="Passkeys" fact={keys.data?.length ? count(keys.data.length, 'passkey') : undefined} bind:heading={title}>
	<p class="hint">
		Optional: a passkey signs you in with your fingerprint, face or screen lock instead of typing your password. Your password keeps working
		either way.
	</p>
	<Loaded {value} empty={keys.data?.length === 0} emptyText="No passkey yet." emptyHint="You sign in with your password.">
		<ul class="plain-list">
			{#each keys.data ?? [] as k (k.id)}
				{#snippet rename()}
					<RenameField
						label="Name of the passkey"
						hideLabel
						autofocus
						value={k.name}
						save={(name) => passkeys.rename(k.id, name).then(() => keys.refetch())}
						said={(name) => `Renamed to ${name}.`}
						ondone={() => closeRename(k)}
					/>
				{/snippet}
				<ListRow whole={renaming === k.id ? rename : undefined} second={facts(k)}>
					<span>{keyName(k)}</span>{#if k.backed_up}<span class="chip accent">Synced</span>{/if}
					{#snippet end()}
						<button class="btn ghost" id="rename-{k.id}" aria-label="Rename {keyName(k)}" onclick={() => (renaming = k.id)}>Rename</button>
						<ConfirmDialog
							ghost
							danger
							label="Remove"
							ariaLabel="Remove {keyName(k)}"
							title="Remove “{keyName(k)}”?"
							description={keys.data?.length === 1
								? 'It stops signing you in to Iris, on every device. You keep signing in with your password.'
								: 'It stops signing you in to Iris, on every device. Your other passkeys and your password keep working.'}
							action="Remove the passkey"
							busy={g.is(`remove:${k.id}`)}
							onconfirm={() => remove(k)}
						/>
					{/snippet}
				</ListRow>
			{/each}
		</ul>
	</Loaded>
	{#if supported()}
		<div>
			<!-- a stable button: its label says the wait, the focus stays on it -->
			<button class="btn" {...pending(g.is('add'))} onclick={add}>
				<Icon name="key" busy={g.is('add')} />{g.is('add') ? 'Waiting for your passkey…' : 'Add a passkey'}
			</button>
		</div>
	{:else}
		<p class="hint">This browser cannot make passkeys here. Open Iris at its usual https address to add one.</p>
	{/if}
</Group>
