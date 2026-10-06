<script lang="ts">
	import { page } from '$app/state';
	import AdminGate from '#lib/admin/AdminGate.svelte';
	import UserHistoryPage from '#lib/admin/UserHistoryPage.svelte';
</script>

<AdminGate>
	<UserHistoryPage userId={page.params.userId ?? ''} />
</AdminGate>
