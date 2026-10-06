<script lang="ts">
	// What a grab waits on, said where it was asked (a row, the release page): the reason in
	// words, and the one way on when there is one. It takes the focus when it appears, so the
	// person hears why nothing started yet, and gives it back to the grab button (`back`) once
	// closed: the notice goes away with the focused button in it.
	import { formatSize } from '@iris/api/format';
	import { refocus } from '#lib/focus.ts';
	import { pending } from '#lib/gesture.svelte.ts';
	import Icon from '#lib/components/Icon.svelte';
	import type { Grab, GrabTarget } from './grab.svelte.ts';

	let { grab, target, back }: { grab: Grab; target: GrabTarget; back: () => HTMLElement | null | undefined } = $props();
	let box = $state<HTMLElement>();

	function close() {
		grab.cancel();
		void refocus(back, 'main h1');
	}

	$effect(() => {
		if (grab.need && box) void refocus(box);
	});
</script>

{#if grab.need}
	<div class="callout warn notice" role="group" aria-label="Before downloading">
		{#if grab.need.kind === 'archive'}
			<p bind:this={box} tabindex="-1">
				<Icon name="triangle-alert" />This release is packed in RAR archives, which Iris cannot stream. Choose a release with a plain video
				file (.mkv or .mp4).
			</p>
			<div class="actions"><button class="btn" onclick={close}>Close</button></div>
		{:else if grab.need.kind === 'huge'}
			<p bind:this={box} tabindex="-1" class="lead">
				<Icon name="triangle-alert" />This release is {formatSize(grab.need.bytes)}. Do you really want all of it?
			</p>
			<p>
				Large packs (a complete series, a box set) fill the shared disk, and everyone's library gets cleaned up sooner. If you only want one
				season or one episode, grab that release instead.
			</p>
			<div class="actions">
				<button class="btn primary" {...pending(grab.busy)} onclick={() => grab.run(target, { huge: true })}>
					<Icon name="download" busy={grab.busy} />Yes, download {formatSize(grab.need.bytes)}
				</button>
				<button class="btn" onclick={close}>Cancel</button>
			</div>
		{:else}
			<p bind:this={box} tabindex="-1" class="lead">
				<Icon name="triangle-alert" />{grab.need.message.replace(/\.$/, '')}. Download another copy anyway?
			</p>
			<div class="actions">
				<button class="btn primary" {...pending(grab.busy)} onclick={() => grab.run(target, { duplicate: true })}>
					<Icon name="download" busy={grab.busy} />Download another copy
				</button>
				<button class="btn" onclick={close}>Cancel</button>
			</div>
		{/if}
	</div>
{/if}

<style>
	.notice p {
		display: flex;
		gap: var(--s-2);
		align-items: flex-start;
	}
	.notice p:not(:first-child) {
		display: block;
	}
	.lead {
		font-weight: 600;
	}
	.notice .btn {
		min-height: var(--control-h);
	}
	.notice :global(.icon) {
		margin-top: var(--s-half);
	}
</style>
