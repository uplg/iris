<script lang="ts">
	// From 600 px a sticky 56 px header whose inner row is the page container; below, the
	// destinations move to a bottom bar (at most five). Session, display settings and the
	// less frequent places (history, admin) live in one panel, from the person's name.
	import { Popover } from 'bits-ui';
	import { page } from '$app/state';
	import { session } from '#lib/session.svelte.ts';
	import { ui, type Theme } from '#lib/ui.svelte.ts';
	import Brand from './Brand.svelte';
	import Icon, { type IconName } from './Icon.svelte';
	import ToggleGroup from './ToggleGroup.svelte';

	const views: { href: string; label: string; icon: IconName; match: (p: string) => boolean }[] = [
		{ href: '/', label: 'Home', icon: 'house', match: (p) => p === '/' },
		{ href: '/search', label: 'Search', icon: 'search', match: (p) => p.startsWith('/search') },
		{ href: '/discover', label: 'Discover', icon: 'compass', match: (p) => p.startsWith('/discover') },
		{ href: '/library', label: 'Library', icon: 'library', match: (p) => p.startsWith('/library') || p.startsWith('/collection') },
		{ href: '/live', label: 'Live TV', icon: 'tv', match: (p) => p.startsWith('/live') }
	];
	const THEMES: { value: Theme; label: string }[] = [
		{ value: 'system', label: 'System' },
		{ value: 'light', label: 'Light' },
		{ value: 'dark', label: 'Dark' }
	];
	const name = $derived(session.user?.display_name ?? '');
	let open = $state(false);
</script>

{#snippet nav(cls: string)}
	<nav class={cls} aria-label="Main">
		{#each views as v (v.href)}
			<a href={v.href} aria-current={v.match(page.url.pathname) ? 'page' : undefined}>
				<Icon name={v.icon} size={20} />
				<span>{v.label}</span>
			</a>
		{/each}
	</nav>
{/snippet}

<header class="top">
	<div class="page bar">
		<Brand />
		{@render nav('views')}
		<div class="grow"></div>
		<Popover.Root bind:open>
			<Popover.Trigger class="btn ghost who" aria-label="{name}: account, theme, sign out">
				<span class="initial" aria-hidden="true">{name.slice(0, 1).toUpperCase()}</span>
				<span class="who-name">{name}</span>
			</Popover.Trigger>
			<Popover.Portal>
				<Popover.Content class="panel" sideOffset={6} align="end" role="dialog" aria-labelledby="session-title">
					<h2 id="session-title" class="who-title">{name}</h2>
					<ToggleGroup type="single" label="Theme" options={THEMES} value={ui.theme} onchange={(t) => ui.setTheme(t)} />
					<div class="row">
						<a class="btn" href="/history" onclick={() => (open = false)}><Icon name="history" />History</a>
						{#if session.user?.is_admin}
							<a class="btn" href="/admin" onclick={() => (open = false)}><Icon name="shield-check" />Admin</a>
						{/if}
					</div>
					<div class="row">
						<a class="btn" href="/account" onclick={() => (open = false)}><Icon name="key" />Account</a>
						<button class="btn" onclick={() => session.logout()}>
							<Icon name="log-out" />Sign out
						</button>
					</div>
				</Popover.Content>
			</Popover.Portal>
		</Popover.Root>
	</div>
</header>
{@render nav('bottom')}

<style>
	.top {
		position: sticky;
		top: 0;
		z-index: 20;
		border-bottom: 1px solid var(--line);
		background: var(--ground);
	}
	.bar {
		display: flex;
		align-items: center;
		gap: var(--s-3) var(--s-5);
		min-height: var(--header-h);
	}
	.grow {
		flex: 1;
	}
	.views {
		display: flex;
		gap: var(--s-1);
	}
	.views a,
	.bottom a {
		display: inline-flex;
		align-items: center;
		gap: var(--s-2);
		color: var(--ink-muted);
		text-decoration: none;
		font: var(--t-label);
		border-radius: var(--radius);
	}
	.views a {
		min-height: var(--control-h-s);
		padding: 0 var(--s-3);
	}
	.views a:hover {
		background: var(--ground-raised);
		color: var(--ink);
	}
	.views a[aria-current='page'] {
		color: var(--accent);
		background: var(--accent-wash);
	}
	:global(.who) {
		gap: var(--s-2);
	}
	.initial {
		display: inline-grid;
		place-items: center;
		width: var(--avatar);
		height: var(--avatar);
		border-radius: 50%;
		background: var(--accent-wash);
		color: var(--accent);
		font: var(--t-meta);
	}
	:global(.panel .who-title) {
		font: var(--t-group);
		margin: 0;
	}

	/* the bottom bar: navigation only, always visible, under 600 px */
	.bottom {
		display: none;
	}
	@media (max-width: 599px) {
		.views,
		.who-name {
			display: none;
		}
		.bottom {
			position: fixed;
			inset: auto 0 0 0;
			z-index: 20;
			display: grid;
			grid-auto-flow: column;
			grid-auto-columns: 1fr;
			height: var(--bottom-h);
			padding-bottom: env(safe-area-inset-bottom, 0px);
			border-top: 1px solid var(--line);
			background: var(--ground);
		}
		.bottom a {
			flex-direction: column;
			justify-content: center;
			gap: var(--s-1);
			font: var(--t-meta);
		}
		.bottom a[aria-current='page'] {
			color: var(--accent);
		}
	}
</style>
