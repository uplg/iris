<script lang="ts">
	// Changing my password: the current one, then the new one (8 characters or more), each
	// shown on demand, pasted and filled by password managers. A wrong current password is said
	// under it, a short new one under it; the server then signs every device out, this one
	// included (its cookies cleared), so this one goes to the door, which says why.
	import { ApiError, auth } from '@iris/api/client';
	import { errorText } from '#lib/errors.ts';
	import { session } from '#lib/session.svelte.ts';
	import { ui } from '#lib/ui.svelte.ts';
	import { Gesture, pending } from '#lib/gesture.svelte.ts';
	import Group from '#lib/components/Group.svelte';
	import Icon from '#lib/components/Icon.svelte';

	const MIN = 8;
	const g = new Gesture();
	let current = $state('');
	let next = $state('');
	let show = $state({ current: false, next: false });
	let errors = $state({ current: '', next: '' });
	const inputs: { current?: HTMLInputElement; next?: HTMLInputElement } = $state({});

	const CHANGED = 'Your password is changed. Sign in again with the new one.';

	/** The field a refusal is about, from the server's code. */
	function refusedField(err: unknown): 'current' | 'next' | undefined {
		if (!(err instanceof ApiError)) return undefined;
		if (err.code === 'wrong_password') return 'current';
		if (err.code === 'password_too_short') return 'next';
		return undefined;
	}

	function fail(field: 'current' | 'next', text: string) {
		errors[field] = text;
		inputs[field]?.focus();
	}

	function submit(e: SubmitEvent) {
		e.preventDefault();
		errors = { current: '', next: '' };
		if (!current) return fail('current', 'Enter your current password.');
		if (next.length < MIN) return fail('next', `Use at least ${MIN} characters.`);
		return g.run(
			() => auth.changePassword(current, next),
			() => {
				current = next = '';
				ui.say(CHANGED);
				session.revoked(CHANGED);
			},
			'change',
			{
				refused: (err) => {
					const field = refusedField(err);
					if (!field) return false;
					fail(field, errorText(err));
					return true;
				},
				field: () => inputs.current
			}
		);
	}
</script>

{#snippet secret(key: 'current' | 'next', label: string, autocomplete: 'current-password' | 'new-password', hint?: string)}
	{@const id = `password-${key}`}
	<div class="field">
		<label for={id}>{label}</label>
		<div class="reveal-row">
			<input
				{id}
				type={show[key] ? 'text' : 'password'}
				bind:this={inputs[key]}
				bind:value={() => (key === 'current' ? current : next), (v) => (key === 'current' ? (current = v) : (next = v))}
				{autocomplete}
				aria-invalid={errors[key] || (key === 'current' && g.error) ? 'true' : undefined}
				aria-describedby="{hint ? `${id}-hint ` : ''}{id}-error"
			/>
			<button
				type="button"
				class="btn ghost"
				aria-pressed={show[key]}
				aria-controls={id}
				aria-label="{show[key] ? 'Hide' : 'Show'} {label.toLowerCase()}"
				onclick={() => (show[key] = !show[key])}>{show[key] ? 'Hide' : 'Show'}</button
			>
		</div>
		{#if hint}<p class="hint" id="{id}-hint">{hint}</p>{/if}
		<p class="form-error" id="{id}-error">{errors[key] || (key === 'current' ? g.error : '')}</p>
	</div>
{/snippet}

<Group id="password-title" title="Password">
	<p class="hint">Changing it signs every device out; each signs in again with the new one.</p>
	<form class="stack" onsubmit={submit} novalidate>
		<!-- the account's email, for password managers to file the new password under -->
		<input type="email" name="username" autocomplete="username" value={session.user?.email ?? ''} readonly hidden />
		{@render secret('current', 'Current password', 'current-password')}
		{@render secret('next', 'New password', 'new-password', `At least ${MIN} characters.`)}
		<div>
			<button class="btn primary" {...pending(g.is('change'))}><Icon name="key" busy={g.is('change')} />Change my password</button>
		</div>
	</form>
</Group>

<style>
	.stack {
		display: grid;
		gap: var(--s-4);
		max-width: 26rem;
	}
	.reveal-row {
		display: flex;
		gap: var(--s-2);
	}
	.reveal-row input {
		flex: 1;
		min-width: 0;
	}
	.btn {
		min-height: var(--control-h);
	}
</style>
