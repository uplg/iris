<script lang="ts">
	// The admin's pages, for admins: anyone else is told so, with the way home (the server
	// refuses them anyway; this only spares a page of refusals).
	import type { Snippet } from 'svelte';
	import { session } from '#lib/session.svelte.ts';
	import PageHead from '#lib/components/PageHead.svelte';

	let { children }: { children: Snippet } = $props();
</script>

{#if session.user?.is_admin}
	{@render children()}
{:else}
	<PageHead title="Admin" />
	<div class="empty">
		<p>Only an admin can open this page.</p>
		<a class="btn" href="/">Go to Home</a>
	</div>
{/if}
