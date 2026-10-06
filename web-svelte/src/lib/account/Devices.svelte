<script lang="ts">
	// My paired devices (a TV signs in with a code it shows: /account?pair=CODE fills it in),
	// each signed out after asking (it needs a new code to come back). Once a code is accepted,
	// the TV still has to fetch its session: the list is read again every 2 s (the query's
	// refetchInterval, no timer of our own) until the new device appears, or until the code's
	// life is over (10 min on the server), when the wait ends and says so.
	import { createQuery } from '@tanstack/svelte-query';
	import { plural } from '@iris/api/format';
	import { page } from '$app/state';
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { devices, type DeviceView } from '@iris/api/client';
	import { loadable, queryClient } from '#lib/query.ts';
	import { ui } from '#lib/ui.svelte.ts';
	import { Gesture, pending } from '#lib/gesture.svelte.ts';
	import { refocus } from '#lib/focus.ts';
	import ConfirmDialog from '#lib/components/ConfirmDialog.svelte';
	import Group from '#lib/components/Group.svelte';
	import Icon from '#lib/components/Icon.svelte';
	import ListRow from '#lib/components/ListRow.svelte';
	import Loaded from '#lib/components/Loaded.svelte';
	import { onDay } from '#lib/history/words.ts';

	/** A pairing code's life on the server (routes/devices.rs DEVICE_CODE_TTL_SECS). */
	const CODE_LIFE_MS = 10 * 60_000;
	const KINDS: Record<string, string> = { 'android-tv': 'Android TV', web: 'Web' };

	/** Waiting for the TV whose code was accepted: since when, and how many devices there were. */
	let waiting = $state<{ since: number; had: number } | null>(null);
	const list = createQuery(
		() => ({
			queryKey: ['devices'],
			queryFn: devices.list,
			refetchInterval: waiting ? 2_000 : (false as const)
		}),
		() => queryClient
	);
	const value = loadable(list);
	const g = new Gesture();
	let code = $state((page.url.searchParams.get('pair') ?? '').toUpperCase());
	let label = $state('');
	let codeField = $state<HTMLInputElement>();
	let invalid = $state('');
	let title = $state<HTMLElement>();

	// a code from the TV's link: taken once, then out of the address (a reload does not refill it)
	onMount(() => {
		if (!page.url.searchParams.has('pair')) return;
		const url = new URL(page.url.href);
		url.searchParams.delete('pair');
		void goto(url, { state: page.state, shallow: true, replace: true });
	});

	// every read of the list answers the wait: the TV is in, or the code is over
	$effect(() => {
		if (!waiting || !list.data) return;
		void list.dataUpdatedAt;
		if (list.data.length > waiting.had) {
			waiting = null;
			ui.toast('Your TV is paired and signed in.');
		} else if (Date.now() - waiting.since > CODE_LIFE_MS) {
			waiting = null;
			ui.toast('The TV did not sign in in time. Show a new code on the TV and enter it here.', { warn: true });
		}
	});

	const name = (d: DeviceView) => d.label || (d.kind ? (KINDS[d.kind] ?? d.kind) : 'Unnamed device');

	function pair(e: SubmitEvent) {
		e.preventDefault();
		invalid = '';
		const c = code.trim().toUpperCase();
		if (c.length < 4) {
			invalid = 'Enter the code your TV shows, like WX7K-ABCD.';
			return codeField?.focus();
		}
		return g.run(
			() => devices.link(c, label.trim() || undefined),
			async () => {
				waiting = { since: Date.now(), had: list.data?.length ?? 0 };
				code = label = '';
				ui.say('Code accepted. Waiting for the TV to sign in.');
				await list.refetch();
			},
			'pair',
			{ field: () => codeField }
		);
	}

	const revoke = (d: DeviceView) =>
		g.run(
			() => devices.revoke(d.jti),
			async () => {
				ui.toast(`${name(d)} signed out.`);
				await list.refetch();
				await refocus(title);
			},
			`revoke:${d.jti}`
		);

	const problem = $derived(invalid || g.error);
</script>

<Group id="devices-title" title="Devices" fact={list.data?.length ? plural(list.data.length, 'device') : undefined} bind:heading={title}>
	<p class="hint">Pair an Android TV, or another Iris app, by entering the code it shows.</p>
	<form class="pair" onsubmit={pair} novalidate>
		<div class="field">
			<label for="pair-code">Pairing code</label>
			<input
				id="pair-code"
				class="code"
				bind:this={codeField}
				value={code}
				oninput={(e) => (code = e.currentTarget.value.toUpperCase())}
				autocomplete="off"
				autocapitalize="characters"
				spellcheck="false"
				placeholder="WX7K-ABCD"
				aria-invalid={problem ? 'true' : undefined}
				aria-describedby="pair-code-error"
			/>
			<p class="form-error" id="pair-code-error">{problem}</p>
		</div>
		<div class="field">
			<label for="pair-label">Device name (optional)</label>
			<input id="pair-label" bind:value={label} autocomplete="off" placeholder="Living room TV" maxlength={64} />
		</div>
		<div class="go">
			<button class="btn primary" {...pending(g.is('pair'))}><Icon name="tv" busy={g.is('pair')} />Pair the TV</button>
		</div>
	</form>
	{#if waiting}
		<div class="callout" role="status">
			<p><Icon name="loader-circle" busy /> Code accepted. Waiting for the TV to sign in; it appears below when it does.</p>
			<div><button class="btn" onclick={() => (waiting = null)}>Stop waiting</button></div>
		</div>
	{/if}
	<Loaded {value} empty={list.data?.length === 0} emptyText="No paired devices yet.">
		<ul class="plain-list">
			{#each list.data ?? [] as d (d.jti)}
				<ListRow second="Paired {onDay(d.issued_at)} · Signed in until {onDay(d.expires_at).replace(/^on /, '')}">
					<Icon name="tv" /><span>{name(d)}</span>{#if d.kind && d.label}<span class="chip">{KINDS[d.kind] ?? d.kind}</span>{/if}
					{#snippet end()}
						<ConfirmDialog
							ghost
							danger
							label="Sign out"
							ariaLabel="Sign out {name(d)}"
							title="Sign out {name(d)}?"
							description="It stops playing from Iris at once. To use it again, pair it with a new code."
							action="Sign the device out"
							busy={g.is(`revoke:${d.jti}`)}
							onconfirm={() => revoke(d)}
						/>
					{/snippet}
				</ListRow>
			{/each}
		</ul>
	</Loaded>
</Group>

<style>
	.pair {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(min(12rem, 100%), 1fr));
		gap: var(--s-3);
		align-items: start;
	}
	.code {
		font-family: var(--font-mono);
		letter-spacing: 0.08em;
	}
	.go {
		padding-top: calc(1.25rem + var(--s-2));
	}
	.go .btn {
		min-height: var(--control-h);
	}
	.callout p {
		display: flex;
		gap: var(--s-2);
		align-items: center;
	}
</style>
