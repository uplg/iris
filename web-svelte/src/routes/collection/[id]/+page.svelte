<script lang="ts">
	import { page } from '$app/state';
	import Collection from '#lib/collection/Collection.svelte';
</script>

<!-- keyed: another title is another page (its seasons, its windows start afresh) -->
{#key page.params.id}
	<Collection id={page.params.id ?? ''} />
{/key}
