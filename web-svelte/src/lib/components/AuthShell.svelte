<script lang="ts">
	// The door (Ariane's AuthShell): Iris's identity on one side, on petrol (its mark, its name
	// in Borel, the promise), what to do on the other. One column on a phone, two from 840 px.
	// Signing in, registering and "Iris does not answer" share it; each page brings a title
	// (h1), a lead and one big action (`.big`, 52 px).
	import type { Snippet } from 'svelte';

	let { children }: { children: Snippet } = $props();
</script>

<div class="door">
	<header class="side">
		<!-- the sleepy screen of the mark, drawn large and faint: decoration only -->
		<svg class="trail" viewBox="0 0 400 400" preserveAspectRatio="xMidYMid slice" aria-hidden="true" focusable="false">
			<g fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
				<rect x="60" y="120" width="280" height="200" rx="40" />
				<path d="M160 70l40 50 40-50" />
				<path d="M160 360h80" />
				<path d="M135 220a20 20 0 0 1 40 0M225 220a20 20 0 0 1 40 0" />
			</g>
		</svg>
		<div class="id">
			<img class="mark" src="/brand.svg" alt="" width="88" height="88" />
			<p class="name">Iris</p>
			<p class="promise">Your films and series, ready when you are.</p>
		</div>
	</header>
	<div class="content">{@render children()}</div>
</div>

<style>
	.door {
		min-height: 100dvh;
		display: grid;
		grid-template-rows: auto 1fr;
		background: var(--ground);
	}
	/* the identity keeps the brand's own petrol and cloud in both themes (6.54:1) */
	.side {
		position: relative;
		overflow: hidden;
		display: grid;
		place-items: center;
		padding: var(--s-6) var(--s-4) var(--s-5);
		background: var(--raw-petrol);
		color: var(--raw-cloud);
	}
	.trail {
		position: absolute;
		inset: 0;
		width: 100%;
		height: 100%;
		opacity: 0.16;
		color: var(--raw-cloud);
	}
	.id {
		position: relative;
		display: grid;
		justify-items: center;
		gap: var(--s-2);
		text-align: center;
	}
	/* the mark's own petrol tile on the petrol panel: a faint cloud ring sets it apart */
	.mark {
		display: block;
		border-radius: var(--radius-2xl);
		box-shadow: 0 0 0 1px color-mix(in srgb, var(--raw-cloud) 35%, transparent);
	}
	.name {
		margin: 0;
		font: var(--t-brand-door);
	}
	.promise {
		margin: 0;
		font: var(--t-body);
		opacity: 0.9;
		max-width: 26rem;
		text-wrap: balance;
	}
	.content {
		display: grid;
		align-content: start;
		justify-items: center;
		padding: var(--s-6) var(--s-4) var(--s-7);
	}
	.content > :global(*) {
		width: min(100%, 26rem);
	}
	/* the door's pages: a title, a lead, one big action */
	.content :global(h1) {
		font: var(--t-page);
		margin: 0;
	}
	.content :global(.lead) {
		margin: 0;
		font: var(--t-body);
		color: var(--ink-muted);
	}
	.content :global(.big) {
		min-height: var(--control-h-l);
		justify-content: center;
		gap: var(--s-2);
		font: var(--t-body);
		font-weight: 500;
		border-radius: var(--radius-xl);
	}
	.content :global(.aside) {
		padding-top: var(--s-4);
		border-top: 1px solid var(--line);
	}
	@media (min-width: 840px) {
		.door {
			grid-template-rows: none;
			grid-template-columns: minmax(20rem, 40%) 1fr;
		}
		.side {
			padding: var(--s-7);
		}
		.name {
			font: var(--t-brand-door-wide);
		}
		.content {
			align-content: center;
			padding: var(--s-7);
		}
	}
</style>
