<script lang="ts">
	// My account, after Maison's: who I am, my passkeys (optional), my password, my paired
	// devices, the languages playback starts in, what « For You » is tuned by, and the theme
	// (also in the header's panel). Each section is labelled by its title; what cannot be
	// undone asks first.
	import { session } from '#lib/session.svelte.ts';
	import { pending } from '#lib/gesture.svelte.ts';
	import { ui, type Theme } from '#lib/ui.svelte.ts';
	import Group from '#lib/components/Group.svelte';
	import Icon from '#lib/components/Icon.svelte';
	import PageHead from '#lib/components/PageHead.svelte';
	import ToggleGroup from '#lib/components/ToggleGroup.svelte';
	import Devices from './Devices.svelte';
	import Identity from './Identity.svelte';
	import PasskeyList from './PasskeyList.svelte';
	import PasswordForm from './PasswordForm.svelte';
	import Playback from './Playback.svelte';
	import Recommendations from './Recommendations.svelte';

	const THEMES: { value: Theme; label: string }[] = [
		{ value: 'system', label: 'System' },
		{ value: 'light', label: 'Light' },
		{ value: 'dark', label: 'Dark' }
	];
</script>

<PageHead title="Account">
	{#snippet sub()}{session.user?.display_name}{/snippet}
	{#snippet end()}
		<button class="btn" {...pending(session.out.is('logout'))} onclick={session.logout}>
			<Icon name="log-out" busy={session.out.is('logout')} />Sign out
		</button>
	{/snippet}
</PageHead>

<div class="sections">
	<Identity />
	<PasskeyList />
	<PasswordForm />
	<Devices />
	<Playback />
	<Recommendations />
	<Group id="display-title" title="Display">
		<ToggleGroup type="single" label="Theme" options={THEMES} value={ui.theme} onchange={(t) => ui.setTheme(t)} />
		<p class="hint">System follows this device’s light or dark setting. Kept on this browser only.</p>
	</Group>
</div>

<style>
	.sections {
		display: grid;
		gap: var(--s-6);
		max-width: var(--measure);
	}
	.sections :global(.group + .group) {
		margin-top: 0;
	}
	.sections :global(.group p) {
		margin: 0;
	}
	/* a narrow screen: a row's actions go under its name, never squeezing it */
	@media (max-width: 599px) {
		.sections :global(.list-row) {
			grid-template-columns: minmax(0, 1fr);
		}
		.sections :global(.list-row > .end) {
			grid-column: 1;
			grid-row: auto;
			justify-content: start;
		}
	}
</style>
