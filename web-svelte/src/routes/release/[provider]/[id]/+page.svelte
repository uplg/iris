<script lang="ts">
	import { page } from '$app/state';
	import ReleaseScreen from '#lib/release/ReleaseScreen.svelte';
</script>

{#key `${page.params.provider}/${page.params.id}`}
	<ReleaseScreen provider={page.params.provider ?? ''} id={page.params.id ?? ''} />
{/key}
