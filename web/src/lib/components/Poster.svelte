<script lang="ts">
	// A title's artwork: a 2:3 poster or a 16:9 still, lazily loaded at the size it is drawn.
	// No artwork: the ground with a glyph and the title, never an empty box. Decoration only:
	// the card around it names the title in text.
	import Icon from './Icon.svelte';

	interface Props {
		src: string | null | undefined;
		title: string;
		shape?: 'poster' | 'still';
		/** Above the fold (a hero): fetched at once. */
		eager?: boolean;
	}
	let { src, title, shape = 'poster', eager = false }: Props = $props();
	// a failure belongs to the address that failed: a new `src` gets its chance
	let failedSrc = $state<string | null>(null);
	const failed = $derived(!!src && failedSrc === src);
	const [w, h] = $derived(shape === 'poster' ? [342, 513] : [500, 281]);
</script>

<div class="art {shape}" aria-hidden="true">
	{#if src && !failed}
		<img
			{src}
			alt=""
			width={w}
			height={h}
			loading={eager ? 'eager' : 'lazy'}
			decoding="async"
			fetchpriority={eager ? 'high' : 'auto'}
			onerror={() => (failedSrc = src ?? null)}
		/>
	{:else}
		<div class="fallback">
			<Icon name="film" size={28} />
			<span>{title}</span>
		</div>
	{/if}
</div>

<style>
	.art {
		position: relative;
		overflow: hidden;
		border: 1px solid var(--line);
		border-radius: var(--radius-m);
		background: var(--ground-raised);
	}
	.poster {
		aspect-ratio: 2 / 3;
	}
	.still {
		aspect-ratio: 16 / 9;
	}
	img {
		display: block;
		width: 100%;
		height: 100%;
		object-fit: cover;
	}
	.fallback {
		position: absolute;
		inset: 0;
		display: flex;
		flex-direction: column;
		justify-content: flex-end;
		gap: var(--s-2);
		padding: var(--s-3);
		color: var(--ink-muted);
	}
	.fallback span {
		font: var(--t-group);
		font-family: var(--font-display);
		color: var(--ink);
		overflow-wrap: anywhere;
	}
</style>
