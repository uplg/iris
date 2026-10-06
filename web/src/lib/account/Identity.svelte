<script lang="ts">
	// Who I am here: my display name (what others see, « added by »), changed in place; my
	// email, private; admin said in words. A name left as it came can take the email's first
	// word in one press. The session learns the new name from the server, not from the field.
	import { auth } from '@iris/api/client';
	import { session } from '#lib/session.svelte.ts';
	import { ui } from '#lib/ui.svelte.ts';
	import { Gesture, pending } from '#lib/gesture.svelte.ts';
	import Group from '#lib/components/Group.svelte';
	import RenameField from '#lib/components/RenameField.svelte';
	import StatusRow from '#lib/components/StatusRow.svelte';

	const g = new Gesture();
	const user = $derived(session.user);
	const fromEmail = $derived(user?.email.split('@')[0]?.split('.')[0] ?? '');

	/** Saved, then read back: the header and every « added by » show the server's name. */
	async function rename(name: string) {
		await auth.changeDisplayName(name);
		session.signedIn(await auth.me());
	}

	const useEmail = () =>
		g.run(
			() => rename(fromEmail),
			() => ui.say(`Display name changed to ${fromEmail}.`),
			'email'
		);
</script>

{#if user}
	<Group id="you-title" title="You">
		<RenameField label="Display name" value={user.display_name} save={rename} said={(name) => `Display name changed to ${name}.`} />
		<p class="hint">Shown to the household, as in “added by”. Your email stays private.</p>
		{#if fromEmail && fromEmail !== user.display_name}
			<div>
				<button class="btn ghost" {...pending(g.is('email'))} onclick={useEmail}>Use “{fromEmail}” from your email</button>
			</div>
		{/if}
		<dl class="facts">
			<StatusRow label="Email" value={user.email} />
			<StatusRow label="Role" value={user.is_admin ? 'Admin' : 'Member'} />
		</dl>
	</Group>
{/if}
