<script lang="ts">
	// The server refuses this bundle (426): nothing works until a reload pulls the new one. The
	// account page stays reachable, as the TV keeps its Settings.
	import { IRIS_WEB_VERSION } from '@iris/api/client';
	import AuthShell from './AuthShell.svelte';
</script>

<AuthShell>
	<section class="notice" aria-labelledby="outdated-title">
		<p class="eyebrow">Update needed</p>
		<h1 id="outdated-title" tabindex="-1">This server needs a newer Iris</h1>
		<p class="lead">This page runs Iris {IRIS_WEB_VERSION}, too old for it. Reload the page to get the new version.</p>
		<button class="btn primary big" onclick={() => location.reload()}>Reload</button>
		<a class="btn" href="/account">Account</a>
	</section>
</AuthShell>

<style>
	.notice {
		display: grid;
		gap: var(--s-4);
		justify-items: start;
	}
	.eyebrow {
		margin: 0;
	}
</style>
