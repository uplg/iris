<script lang="ts">
	// The library inside the app's query cache, for its tests (the layout provides it in the app).
	import '../../styles/app.css';
	import { QueryClientProvider } from '@tanstack/svelte-query';
	import { queryClient } from '#lib/query.ts';
	import Library from './Library.svelte';
</script>

<QueryClientProvider client={queryClient}>
	<main class="page"><Library /></main>
</QueryClientProvider>
