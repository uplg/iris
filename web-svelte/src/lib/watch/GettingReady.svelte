<script lang="ts">
	// The stage before the picture: each step in words (done, in progress, waiting), how far the
	// current one is, and a problem said plainly with what can be done about it.
	import Icon from '#lib/components/Icon.svelte';
	import { percent } from '@iris/api/format';
	import { stepStateText, type Readiness } from './ready.ts';

	interface Props {
		ready: Readiness;
		/** Remove the dead release and search for another. */
		onreplace?: () => void;
		replacing?: boolean;
		replaceError?: string;
	}
	let { ready, onreplace, replacing = false, replaceError = '' }: Props = $props();
	const current = $derived(ready.steps.find((s) => s.state === 'current'));
	// said once per step (not per percent): the live region changes when the step does
	const SPOKEN: Record<string, string> = {
		peers: 'Connecting to peers',
		head: 'Downloading the first minutes',
		read: 'Reading the file',
		resume: 'Finding where you stopped',
		server: 'Preparing the stream on the server',
		start: 'Starting playback'
	};
</script>

<div class="ready">
	<p class="sr-only" role="status">{ready.problem ? ready.problem.title : current ? SPOKEN[current.id] : ''}</p>
	{#if ready.problem}
		<div class="problem" role="alert">
			<Icon name="triangle-alert" size={28} />
			<h2 class="title">{ready.problem.title}</h2>
			<p class="detail">{ready.problem.detail}</p>
			{#if ready.problem.deadSwarm && onreplace}
				<button class="btn primary" type="button" aria-busy={replacing || undefined} onclick={() => !replacing && onreplace()}>
					<Icon name="search" busy={replacing} />Remove it and pick another release
				</button>
				{#if replaceError}<p class="detail warn">Iris could not remove it: {replaceError}</p>{/if}
			{/if}
		</div>
	{:else}
		<h2 class="title">Getting ready</h2>
		<ol class="steps">
			{#each ready.steps as s (s.id)}
				<li class={s.state}>
					<span class="mark" aria-hidden="true">
						{#if s.state === 'done'}<Icon name="circle-check" />{:else if s.state === 'current'}<Icon
								name="loader-circle"
								busy
							/>{:else}<Icon name="circle" />{/if}
					</span>
					<span class="text">
						<span class="label">{s.label}<span class="sr-only">, {stepStateText(s.state)}</span></span>
						{#if s.state === 'current' && s.detail}<span class="detail">{s.detail}</span>{/if}
						{#if s.state === 'current' && s.pct != null}
							<!-- the label already says the percentage: a bar of its own, named by it -->
							<span
								class="bar"
								role="progressbar"
								aria-label={s.label}
								aria-valuemin={0}
								aria-valuemax={100}
								aria-valuenow={Math.round(s.pct)}
								aria-valuetext={percent(s.pct)}><span style:width="{s.pct}%"></span></span
							>
						{/if}
					</span>
				</li>
			{/each}
		</ol>
	{/if}
</div>

<style>
	.ready {
		color-scheme: dark;
		width: 100%;
		height: 100%;
		overflow: auto;
		display: grid;
		place-content: center;
		gap: var(--s-4);
		padding: var(--s-5) var(--s-4);
		background: var(--stage);
		color: var(--stage-ink);
	}
	.title {
		font: var(--t-group);
		margin: 0;
	}
	.steps {
		list-style: none;
		margin: 0;
		padding: 0;
		display: grid;
		gap: var(--s-3);
		width: min(26rem, 100%);
	}
	.steps li {
		display: flex;
		gap: var(--s-3);
		align-items: flex-start;
	}
	.mark {
		flex: none;
		display: grid;
		place-items: center;
		width: var(--lead);
		padding-top: var(--s-half);
	}
	.text {
		display: grid;
		gap: var(--s-1);
		min-width: 0;
		flex: 1;
	}
	.label {
		font: var(--t-label);
	}
	.waiting {
		color: var(--stage-muted);
	}
	.done .mark {
		color: var(--accent);
	}
	.current .label {
		color: var(--stage-ink);
	}
	.detail {
		margin: 0;
		font: var(--t-secondary);
		color: var(--stage-muted);
		overflow-wrap: anywhere;
	}
	.bar {
		display: block;
		height: var(--track-h);
		border-radius: var(--radius-pill);
		background: var(--stage-line);
		overflow: hidden;
	}
	.bar span {
		display: block;
		height: 100%;
		background: var(--accent);
	}
	.detail.warn {
		color: var(--warn-text);
	}
	.problem {
		display: grid;
		justify-items: center;
		text-align: center;
		gap: var(--s-3);
		max-width: 32rem;
		color: var(--stage-ink);
	}
	.problem > :global(svg) {
		color: var(--warn-text);
	}
	.problem .btn {
		min-height: var(--control-h);
	}
</style>
