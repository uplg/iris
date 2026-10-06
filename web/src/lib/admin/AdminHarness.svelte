<script lang="ts">
	// The admin's pages in a page landmark, as the layout draws them, for their tests.
	import AdminPage from './AdminPage.svelte';
	import UserHistoryPage from './UserHistoryPage.svelte';

	let { person }: { person?: string } = $props();
</script>

<main class="page">
	{#if person}<UserHistoryPage userId={person} />{:else}<AdminPage />{/if}
</main>
