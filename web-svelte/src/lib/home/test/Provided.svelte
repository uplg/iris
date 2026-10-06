<script lang="ts">
	// A page as the layout gives it: under the app's one query client (tests only).
	import type { Component } from 'svelte';
	import { QueryClientProvider } from '@tanstack/svelte-query';
	import { queryClient } from '#lib/query.ts';

	let { view: View }: { view: Component<Record<string, never>> } = $props();
</script>

<QueryClientProvider client={queryClient}><View /></QueryClientProvider>
