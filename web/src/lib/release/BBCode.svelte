<script lang="ts">
	// A tracker's BBCode description, drawn by us from its tree (bbcode.ts): emphasis, centring,
	// links and images kept; the tracker's colors and font sizes dropped (a size of 1 or 2 stays
	// a quieter line); links open elsewhere, without our address.
	import { nodesToText, prepareBBCode, safeHref, safeSrc, structural, type BBNode } from './bbcode.ts';

	let { source }: { source: string } = $props();
	const tree = $derived(prepareBBCode(source));
</script>

{#snippet nodes(list: readonly BBNode[])}
	{#each list as n, i (i)}
		{#if n.type === 'text'}
			{#each n.value.split('\n') as line, j (j)}{#if j > 0}<br />{/if}{line}{/each}
		{:else if n.name === 'b'}
			<strong>{@render nodes(n.children)}</strong>
		{:else if n.name === 'i'}
			<em>{@render nodes(n.children)}</em>
		{:else if n.name === 'u'}
			<u>{@render nodes(n.children)}</u>
		{:else if n.name === 'center'}
			<div class="center">{@render nodes(n.children)}</div>
		{:else if n.name === 'justify'}
			<div>{@render nodes(n.children)}</div>
		{:else if n.name === 'size' && n.arg && parseInt(n.arg, 10) <= 2}
			<span class="small">{@render nodes(n.children)}</span>
		{:else if n.name === 'table'}
			<table>
				<tbody>
					{#each structural(n.children, 'tr') as row, r (r)}
						{#if row.type === 'tag'}
							<tr>
								{#each structural(row.children, 'td') as cell, c (c)}
									{#if cell.type === 'tag'}<td>{@render nodes(cell.children)}</td>{/if}
								{/each}
							</tr>
						{/if}
					{/each}
				</tbody>
			</table>
		{:else if n.name === 'url'}
			{@const href = safeHref(n.arg ?? nodesToText(n.children))}
			{#if href}
				<a {href} target="_blank" rel="noopener noreferrer">{@render nodes(n.children)}</a>
			{:else}
				<span>{@render nodes(n.children)}</span>
			{/if}
		{:else if n.name === 'img'}
			{@const src = safeSrc(nodesToText(n.children))}
			{#if src}<img {src} alt="" loading="lazy" decoding="async" referrerpolicy="no-referrer" />{/if}
		{:else}
			<!-- color, uicolor, a size and any tag we do not know: the content, in our own colors -->
			<span>{@render nodes(n.children)}</span>
		{/if}
	{/each}
{/snippet}

<div class="bbcode">{@render nodes(tree)}</div>

<style>
	.bbcode {
		overflow-wrap: anywhere;
	}
	.center {
		text-align: center;
	}
	.small {
		font: var(--t-secondary);
		color: var(--ink-muted);
	}
	table {
		margin: var(--s-2) auto;
		border-collapse: collapse;
		text-align: left;
	}
	td {
		padding: var(--s-1) var(--s-2);
		vertical-align: top;
	}
	img {
		display: inline-block;
		max-width: 100%;
		max-height: 8rem;
		height: auto;
		margin-block: var(--s-1);
		border-radius: var(--radius-s);
	}
</style>
