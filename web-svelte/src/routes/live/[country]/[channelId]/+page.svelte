<script lang="ts">
	import { page } from '$app/state';
	import LiveWatch from '#lib/live/LiveWatch.svelte';

	const country = $derived(page.params.country ?? 'fr');
	const channelId = $derived(page.params.channelId ?? '');
</script>

<!-- another channel: a player from nothing -->
{#key `${country}:${channelId}`}
	<LiveWatch {country} {channelId} />
{/key}
