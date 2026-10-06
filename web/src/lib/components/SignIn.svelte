<script lang="ts">
	// Sign in: email and password, or a passkey (optional, never required). Saved passkeys show
	// in the email field's autofill (conditional mediation); a button asks for one modally.
	// After a password sign-in, a browser that can make passkeys offers one, once per account
	// and device, with "Not now" as easy as "Make a passkey" (CNIL: refusing as simple as
	// accepting). Paste and password managers always work (WCAG 3.3.8).
	import { onMount } from 'svelte';
	import { auth, type User } from '@iris/api/client';
	import { conditionalSupported, register, signIn, supported } from '@iris/api/passkeys';
	import { PUBLIC_PASSKEY_OFFERED_KEY } from '$app/env/public';
	import { json, stored } from '#lib/stored.ts';
	import { pageTitle } from '#lib/title.ts';
	import { session } from '#lib/session.svelte.ts';
	import { Gesture, pending } from '#lib/gesture.svelte.ts';
	import AuthShell from './AuthShell.svelte';
	import Icon from './Icon.svelte';

	type Step = { name: 'signin' } | { name: 'passkey' } | { name: 'offer'; user: User } | { name: 'done'; user: User; label: string };

	const offered = stored<string[]>(
		PUBLIC_PASSKEY_OFFERED_KEY,
		[],
		json((v): v is string[] => Array.isArray(v))
	);
	const g = new Gesture();
	let step = $state<Step>({ name: 'signin' });
	// why the session ended here, when the server ended it (a password change)
	const notice = $derived(session.state.status === 'signed_out' ? session.state.notice : undefined);
	let email = $state('');
	let password = $state('');
	let reveal = $state(false);
	// the autofill wait, given up when the user signs in another way
	let autofill: AbortController | undefined;

	onMount(() => {
		void startAutofill();
		return () => autofill?.abort();
	});

	async function startAutofill() {
		if (!(await conditionalSupported())) return;
		autofill = new AbortController();
		try {
			done(await signIn({ conditional: true, signal: autofill.signal }));
		} catch {
			// no passkey picked, or the wait given up: the form stays
		}
	}

	/** Signed in: offer a passkey once, else straight in. */
	function arrived(user: User) {
		const ids = offered.get();
		if (supported() && !ids.includes(user.id)) {
			offered.set([...ids, user.id]);
			step = { name: 'offer', user };
		} else done(user);
	}

	function done(user: User) {
		session.signedIn(user);
	}

	function withPassword(e: SubmitEvent) {
		e.preventDefault();
		autofill?.abort();
		return g.run(() => auth.login(email.trim(), password), arrived, 'password', { inline: true });
	}

	function withPasskey() {
		autofill?.abort();
		step = { name: 'passkey' };
		return g.run(() => signIn(), done, 'passkey', { inline: true, refused: () => void (step = { name: 'signin' }) });
	}

	function makePasskey(user: User) {
		return g.run(
			() => register(),
			(pk) => (step = { name: 'done', user, label: pk.name || 'this device' }),
			'make',
			{ inline: true }
		);
	}
</script>

<svelte:head><title>{pageTitle('Sign in')}</title></svelte:head>

<AuthShell>
	{#if step.name === 'signin'}
		<section class="door-step" aria-labelledby="signin-title">
			<div class="intro">
				<h1 id="signin-title" tabindex="-1">Sign in</h1>
				<p class="lead">Use your email and password, or a passkey if you made one.</p>
			</div>
			{#if notice}<p class="callout">{notice}</p>{/if}
			<form class="stack" onsubmit={withPassword}>
				<div class="field">
					<label for="email">Email</label>
					<input
						id="email"
						type="email"
						name="email"
						autocomplete="username webauthn"
						required
						bind:value={email}
						aria-describedby="email-hint"
					/>
					<p id="email-hint" class="hint">Your saved passkeys for Iris also appear in this field.</p>
				</div>
				<div class="field">
					<label for="password">Password</label>
					<div class="reveal-row">
						<input
							id="password"
							type={reveal ? 'text' : 'password'}
							name="password"
							autocomplete="current-password"
							required
							bind:value={password}
						/>
						<button type="button" class="btn ghost" aria-pressed={reveal} aria-controls="password" onclick={() => (reveal = !reveal)}>
							{reveal ? 'Hide' : 'Show'}
						</button>
					</div>
					<p class="hint">Forgot it? The person who runs this server can reset it.</p>
				</div>
				<button class="btn primary big" {...pending(g.is('password'))}><Icon name="arrow-right" busy={g.is('password')} />Sign in</button>
				<p class="form-error" role="alert">{g.is('passkey') ? '' : g.error}</p>
			</form>
			{#if supported()}
				<div class="or" aria-hidden="true"><span>or</span></div>
				<button class="btn big" onclick={withPasskey}><Icon name="key" />Sign in with a passkey</button>
			{/if}
			<div class="notes">
				<p class="hint">No account yet? Iris is invitation-only: open the invite link you were sent.</p>
				<p class="hint">Setting up a TV? Sign in here, then enter the code the TV shows in Account.</p>
			</div>
		</section>
	{:else if step.name === 'passkey'}
		<section class="door-step" aria-labelledby="passkey-title">
			<div class="intro">
				<h1 id="passkey-title" tabindex="-1">Use your passkey</h1>
				<p class="lead">Your browser or phone asks you to confirm with your fingerprint, face or screen lock.</p>
			</div>
			<p class="banner" role="status"><Icon name="loader-circle" busy />Waiting for your passkey</p>
			<p class="form-error" role="alert">{g.error}</p>
			<p class="hint">
				Nothing appeared? This device may not hold a passkey for Iris yet. Sign in with your password and make one afterwards.
			</p>
			<button class="btn big" onclick={() => (step = { name: 'signin' })}>Use my password instead</button>
		</section>
	{:else if step.name === 'offer'}
		{@const user = step.user}
		<section class="door-step" aria-labelledby="offer-title">
			<div class="intro">
				<p class="hint">Signed in as {user.display_name}</p>
				<h1 id="offer-title" tabindex="-1">Sign in faster next time?</h1>
				<p class="lead">
					A passkey replaces typing your password on this device. It uses your fingerprint, face or screen lock and only works on this site,
					so it can't be phished. Your password keeps working.
				</p>
			</div>
			<div class="pair">
				<button class="btn primary big" {...pending(g.is('make'))} onclick={() => makePasskey(user)}>
					<Icon name="key" busy={g.is('make')} />Make a passkey
				</button>
				<button class="btn big" onclick={() => done(user)}>Not now</button>
			</div>
			<p class="form-error" role="alert">{g.error}</p>
			<p class="hint">You can add or remove passkeys anytime in Account. Iris asks once per device.</p>
		</section>
	{:else}
		{@const user = step.user}
		<section class="door-step" aria-labelledby="done-title">
			<div class="intro">
				<h1 id="done-title" tabindex="-1">Passkey ready</h1>
				<p class="lead">Next time, pick Iris in your passkey list. It's saved as "{step.label}", which you can rename in Account.</p>
			</div>
			<button class="btn primary big" onclick={() => done(user)}><Icon name="arrow-right" />Continue to Iris</button>
		</section>
	{/if}
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
	.or {
		display: flex;
		align-items: center;
		gap: var(--s-3);
		color: var(--ink-muted);
		font: var(--t-secondary);
	}
	.or::before,
	.or::after {
		content: '';
		flex: 1;
		height: 1px;
		background: var(--line);
	}
	.pair {
		display: grid;
		grid-template-columns: 1fr 1fr;
		gap: var(--s-3);
	}
	.notes {
		display: grid;
		gap: var(--s-2);
		padding-top: var(--s-4);
		border-top: 1px solid var(--line);
	}
	.banner {
		display: flex;
		align-items: center;
		gap: var(--s-2);
	}
</style>
