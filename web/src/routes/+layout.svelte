<script lang="ts">
	import '@fontsource-variable/fraunces';
	import '../styles/app.css';
	import { QueryClientProvider } from '@tanstack/svelte-query';
	import { afterNavigate, beforeNavigate } from '$app/navigation';
	import { page, updated } from '$app/state';
	import { queryClient } from '#lib/query.ts';
	import { session } from '#lib/session.svelte.ts';
	import { ui } from '#lib/ui.svelte.ts';
	import { refocus } from '#lib/focus.ts';
	import Header from '#lib/components/Header.svelte';
	import AuthShell from '#lib/components/AuthShell.svelte';
	import SignIn from '#lib/components/SignIn.svelte';
	import Toasts from '#lib/components/Toasts.svelte';
	import Outdated from '#lib/components/Outdated.svelte';
	import { harmlessReload, outdated } from '#lib/update.svelte.ts';

	let { children } = $props();

	// Registering opens signed out: it is how one gets an account
	const door = $derived(page.url.pathname.startsWith('/register'));

	$effect(() => session.start());
	$effect(() => outdated.listen());

	// refused by the server (426): locked until a reload, the account page aside
	const locked = $derived(outdated.locked && !page.url.pathname.startsWith('/account'));
	$effect(() => {
		if (locked) void refocus('#outdated-title');
	});

	$effect(() => {
		const root = document.documentElement;
		if (ui.theme === 'system') delete root.dataset.theme;
		else root.dataset.theme = ui.theme;
	});

	// signed in: the page replaces the door under the focus, which goes to the page's title
	let was = session.state.status;
	$effect(() => {
		const now = session.state.status;
		if (now === 'signed_in' && was !== 'signed_in' && was !== 'loading') void refocus('main h1');
		was = now;
	});

	// on a change of view, focus goes to the new view's title
	let first = true;
	afterNavigate(() => {
		if (first) return void (first = false);
		void refocus('main h1');
	});

	// a new version deployed: taken at the next harmless moment (a navigation, or back on a page
	// where nothing plays), never under the fingers nor mid-playback
	beforeNavigate(({ willUnload, to }) => {
		if (updated.current && !willUnload && to?.url) location.href = to.url.href;
	});
	$effect(() => {
		const back = async () => {
			if (document.visibilityState !== 'visible') return;
			if ((updated.current || (await updated.check())) && harmlessReload(page.url.pathname)) location.reload();
		};
		document.addEventListener('visibilitychange', back);
		return () => document.removeEventListener('visibilitychange', back);
	});
</script>

<QueryClientProvider client={queryClient}>
	<nav class="skip" aria-label="Skip to content"><a href="#main">Skip to content</a></nav>
	{#if session.state.status === 'signed_in' && !locked}<Header />{/if}
	<main id="main" class:page={session.state.status === 'signed_in' && !locked} tabindex="-1">
		{#if locked}
			<Outdated />
		{:else if session.state.status === 'loading'}
			{#if session.state.retrying}
				<AuthShell>
					<section class="notice" aria-labelledby="unreachable-title">
						<h1 id="unreachable-title" tabindex="-1">Iris does not answer</h1>
						<p class="lead">It may be restarting. Iris tries again on its own; you can also reload.</p>
						<button class="btn primary big" onclick={() => location.reload()}>Reload</button>
					</section>
				</AuthShell>
			{:else}
				<p class="muted loading">Loading…</p>
			{/if}
		{:else if session.state.status === 'signed_out' && !door}
			<SignIn />
		{:else}
			<!-- another account: every view starts over, nothing kept from the last one -->
			{#key session.user?.id}{@render children()}{/key}
		{/if}
	</main>
	<Toasts />
	<!-- two live regions mounted once; only their text changes -->
	<div class="sr-only" role="status" aria-live="polite" aria-atomic="true">{ui.polite}</div>
	<div class="sr-only" role="alert">{ui.assertive}</div>
</QueryClientProvider>

<style>
	main {
		padding-bottom: calc(var(--bottom-h) + var(--s-6));
	}
	.loading {
		padding: var(--s-6) var(--page-margin);
	}
	.notice {
		display: grid;
		gap: var(--s-4);
	}
	/* the door fills the screen: no room kept for a bottom bar it does not have */
	main:not(.page) {
		padding-bottom: 0;
	}
</style>
