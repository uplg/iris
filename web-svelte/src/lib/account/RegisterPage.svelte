<script lang="ts">
	// Creating an account, at the door (signed out): the invitation code (filled from the
	// invite link's `?token`), an email, a password of 8 characters or more (shown on demand,
	// pasted and saved by password managers), one big action. A refusal is said under the
	// field it is about. Once in, Home; signed in already, straight there.
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { ApiError, auth } from '@iris/api/client';
	import { session } from '#lib/session.svelte.ts';
	import { pageTitle } from '#lib/title.ts';
	import { errorText } from '#lib/errors.ts';
	import { Gesture, pending } from '#lib/gesture.svelte.ts';
	import AuthShell from '#lib/components/AuthShell.svelte';
	import Icon from '#lib/components/Icon.svelte';

	const MIN = 8;
	const g = new Gesture();
	let token = $state(page.url.searchParams.get('token') ?? '');
	let email = $state('');
	let password = $state('');
	let reveal = $state(false);
	/** What is wrong, by field: said under it, the focus brought to it. */
	let problem = $state<{ field: 'token' | 'email' | 'password' | 'form'; text: string } | null>(null);
	const fields: Record<string, HTMLInputElement | undefined> = $state({});

	$effect(() => {
		if (session.state.status === 'signed_in') void goto('/', { replace: true });
	});

	function say(field: 'token' | 'email' | 'password' | 'form', text: string) {
		problem = { field, text };
		if (field !== 'form') fields[field]?.focus();
	}

	/** Which field a refusal is about, from the server's words. */
	function refusal(e: unknown): boolean {
		if (!(e instanceof ApiError)) return false;
		const text = errorText(e);
		if (/invitation/i.test(e.message)) say('token', 'This invitation code is not valid any more. Ask for a new invitation link.');
		else if (/already registered/i.test(e.message)) say('email', 'An account already uses this email. Sign in instead.');
		else if (/email/i.test(e.message)) say('email', 'This does not look like an email address.');
		else if (/password/i.test(e.message)) say('password', `Use at least ${MIN} characters.`);
		else say('form', text);
		return true;
	}

	function submit(e: SubmitEvent) {
		e.preventDefault();
		problem = null;
		if (!token.trim()) return say('token', 'Enter the invitation code from your invite link.');
		if (!email.trim()) return say('email', 'Enter your email.');
		if (password.length < MIN) return say('password', `Use at least ${MIN} characters.`);
		return g.run(
			() => auth.register(token.trim(), email.trim(), password),
			(user) => session.signedIn(user),
			'register',
			{ refused: refusal, inline: true }
		);
	}

	const error = (f: 'token' | 'email' | 'password') => (problem?.field === f ? problem.text : '');
</script>

<svelte:head><title>{pageTitle('Create your account')}</title></svelte:head>

<AuthShell>
	<section class="door-step" aria-labelledby="register-title">
		<div class="intro">
			<h1 id="register-title" tabindex="-1">Create your account</h1>
			<p class="lead">Iris is invitation-only. Your invite link fills in the code for you.</p>
		</div>
		<form class="stack" onsubmit={submit} novalidate>
			{#snippet message(f: 'token' | 'email' | 'password', id: string)}
				<p class="form-error" {id}>{error(f)}</p>
			{/snippet}
			<div class="field">
				<label for="invite-token">Invitation code</label>
				<input
					id="invite-token"
					name="invite"
					bind:this={fields.token}
					bind:value={token}
					autocomplete="off"
					spellcheck="false"
					autocapitalize="none"
					aria-invalid={error('token') ? 'true' : undefined}
					aria-describedby="invite-token-error"
				/>
				{@render message('token', 'invite-token-error')}
			</div>
			<div class="field">
				<label for="register-email">Email</label>
				<input
					id="register-email"
					type="email"
					name="email"
					bind:this={fields.email}
					bind:value={email}
					autocomplete="username"
					aria-invalid={error('email') ? 'true' : undefined}
					aria-describedby="register-email-error"
				/>
				{@render message('email', 'register-email-error')}
			</div>
			<div class="field">
				<label for="register-password">Password</label>
				<div class="reveal-row">
					<input
						id="register-password"
						type={reveal ? 'text' : 'password'}
						name="password"
						bind:this={fields.password}
						bind:value={password}
						autocomplete="new-password"
						minlength={MIN}
						aria-invalid={error('password') ? 'true' : undefined}
						aria-describedby="register-password-hint register-password-error"
					/>
					<button type="button" class="btn ghost" aria-pressed={reveal} aria-controls="register-password" onclick={() => (reveal = !reveal)}
						>{reveal ? 'Hide' : 'Show'}</button
					>
				</div>
				<p class="hint" id="register-password-hint">At least {MIN} characters. You can add a passkey later in Account.</p>
				{@render message('password', 'register-password-error')}
			</div>
			<button class="btn primary big" {...pending(g.is('register'))}>
				<Icon name="arrow-right" busy={g.is('register')} />Create my account
			</button>
			<p class="form-error" role="alert">{problem?.field === 'form' ? problem.text : g.error}</p>
		</form>
		<p class="hint aside">Already have an account? <a href="/">Sign in</a></p>
	</section>
</AuthShell>

<style>
	.door-step,
	.stack {
		display: grid;
		gap: var(--s-4);
	}
	.intro {
		display: grid;
		gap: var(--s-2);
	}
	.door-step p {
		margin: 0;
	}
	.reveal-row {
		display: flex;
		gap: var(--s-2);
	}
	.reveal-row input {
		flex: 1;
		min-width: 0;
	}
	.reveal-row .btn {
		min-height: var(--control-h);
	}
</style>
