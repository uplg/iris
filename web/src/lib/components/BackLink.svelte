<script lang="ts">
	// The way back to where the person came from, one look for every page: an arrow and the
	// place's name. On the stage (the watch and live pages) it takes the stage's ink and, on a
	// phone, keeps to its arrow (the name stays for screen readers).
	import Icon from './Icon.svelte';

	interface Props {
		href: string;
		/** Where it leads: « Home », « Back to results ». */
		label: string;
		onclick?: (e: MouseEvent) => void;
		stage?: boolean;
	}
	let { href, label, onclick, stage = false }: Props = $props();
</script>

<a class="back" class:link-btn={!stage} class:quiet={!stage} class:stage {href} {onclick}
	><Icon name="arrow-left" /><span class="text">{label}</span></a
>

<style>
	.back {
		display: inline-flex;
		align-items: center;
		gap: var(--s-2);
		min-height: var(--control-h);
		justify-self: start;
	}
	.back:not(.stage) {
		margin-top: var(--s-3);
	}
	.stage {
		color: var(--stage-ink);
		font: var(--t-label);
		text-decoration: none;
	}
	.stage:hover {
		text-decoration: underline;
	}
	@media (max-width: 599px) {
		.stage {
			flex: none;
			width: var(--control-h);
			justify-content: center;
		}
		.stage .text {
			position: absolute;
			width: 1px;
			height: 1px;
			overflow: hidden;
			clip: rect(0, 0, 0, 0);
			white-space: nowrap;
		}
	}
</style>
