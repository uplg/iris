<script lang="ts">
	// The timeline: a slider (APG) over a 6 px track with what is buffered and what is played,
	// a 24 px thumb, and a bubble that says where a hover or a key would land (and its chapter).
	// A drag previews only; the seek goes on release. Keys seek at once: ←/→ 5 s, Page Up/Down a
	// minute, Home/End the ends. <input type=range> cannot layer the buffered ranges, hence a div.
	import type { Chapter } from '@iris/core/manifest-client';
	import { clock } from '@iris/api/format';
	import { positionText } from './spoken.ts';

	interface Props {
		duration: number | null;
		current: number;
		buffered: [number, number][];
		chapters?: Chapter[];
		/** Dragging: the position previewed (the chrome shows it, nothing seeks yet). */
		onscrub: (seconds: number) => void;
		/** Release or a key: seek there. */
		onseek: (seconds: number) => void;
		/** A pointer drag ended without a seek (cancelled). */
		oncancel?: () => void;
	}
	let { duration, current, buffered, chapters = [], onscrub, onseek, oncancel }: Props = $props();

	const KEY_STEP = 5;
	const PAGE_STEP = 60;

	let track = $state<HTMLElement | null>(null);
	let dragging = false;
	let hover = $state<number | null>(null);
	let focused = $state(false);

	const dur = $derived(duration && duration > 0 ? duration : null);
	const pct = (s: number) => (dur ? Math.min(100, Math.max(0, (s / dur) * 100)) : 0);
	const preview = $derived(hover ?? (focused ? current : null));
	const chapterAt = (s: number) => chapters.find((c) => s >= c.start_s && s < c.end_s)?.title ?? null;

	function at(e: PointerEvent): number {
		if (!track || !dur) return 0;
		const r = track.getBoundingClientRect();
		const x = Math.max(0, Math.min(r.width, e.clientX - r.left));
		return r.width > 0 ? (x / r.width) * dur : 0;
	}

	function down(e: PointerEvent) {
		if (!dur || e.button !== 0) return;
		(e.currentTarget as HTMLElement).setPointerCapture?.(e.pointerId);
		dragging = true;
		onscrub(at(e));
	}
	function move(e: PointerEvent) {
		if (!dur) return;
		const s = at(e);
		if (e.pointerType === 'mouse') hover = s;
		if (dragging) onscrub(s);
	}
	function up(e: PointerEvent) {
		if (!dragging) return;
		dragging = false;
		onseek(at(e));
	}
	function cancel() {
		if (!dragging) return;
		dragging = false;
		oncancel?.();
	}

	function key(e: KeyboardEvent) {
		if (!dur) return;
		const by: Record<string, number> = {
			ArrowLeft: -KEY_STEP,
			ArrowDown: -KEY_STEP,
			ArrowRight: KEY_STEP,
			ArrowUp: KEY_STEP,
			PageDown: -PAGE_STEP,
			PageUp: PAGE_STEP
		};
		let to: number | null = null;
		if (e.key in by) to = current + by[e.key];
		else if (e.key === 'Home') to = 0;
		else if (e.key === 'End') to = dur;
		if (to === null) return;
		e.preventDefault();
		onseek(Math.max(0, Math.min(dur, to)));
	}
</script>

{#if dur}
	<div
		bind:this={track}
		class="scrub"
		role="slider"
		tabindex="0"
		aria-label="Seek"
		aria-valuemin={0}
		aria-valuemax={Math.round(dur)}
		aria-valuenow={Math.round(current)}
		aria-valuetext={positionText(current, dur)}
		onpointerdown={down}
		onpointermove={move}
		onpointerup={up}
		onpointercancel={cancel}
		onpointerleave={() => (hover = null)}
		onkeydown={key}
		onfocus={() => (focused = true)}
		onblur={() => (focused = false)}
	>
		<span class="rail">
			{#each buffered as [s, e], i (i)}
				<span class="buffered" style:left="{pct(s)}%" style:width="{Math.max(0, pct(e) - pct(s))}%"></span>
			{/each}
			<span class="played" style:width="{pct(current)}%"></span>
		</span>
		<span class="thumb" style:left="{pct(current)}%"></span>
		{#if preview !== null}
			{@const chapter = chapterAt(preview)}
			<span class="bubble" style:left="clamp(var(--s-6), {pct(preview)}%, calc(100% - var(--s-6)))" aria-hidden="true">
				{clock(preview)}{#if chapter}<span class="chapter">{chapter}</span>{/if}
			</span>
		{/if}
	</div>
{:else}
	<div class="scrub flat" aria-hidden="true"><span class="rail"></span></div>
{/if}

<style>
	/* a 44 px band to grab, the thin track inside it (WCAG 2.5.8) */
	.scrub {
		position: relative;
		flex: 1;
		min-width: 0;
		display: flex;
		align-items: center;
		min-height: var(--control-h);
		cursor: pointer;
		touch-action: none;
		border-radius: var(--radius-s);
	}
	.scrub.flat {
		cursor: default;
	}
	.rail {
		position: relative;
		flex: 1;
		height: var(--track-h);
		border-radius: var(--radius-pill);
		background: var(--stage-line);
		overflow: hidden;
	}
	.buffered {
		position: absolute;
		top: 0;
		bottom: 0;
		background: var(--stage-fill);
	}
	.played {
		position: absolute;
		left: 0;
		top: 0;
		bottom: 0;
		background: var(--accent);
	}
	.thumb {
		position: absolute;
		top: 50%;
		width: var(--thumb);
		height: var(--thumb);
		border-radius: 50%;
		transform: translate(-50%, -50%);
		background: var(--stage-ink);
		border: 3px solid var(--accent);
		box-shadow: var(--shadow);
		pointer-events: none;
	}
	.bubble {
		position: absolute;
		bottom: calc(100% + var(--s-1));
		transform: translateX(-50%);
		display: grid;
		justify-items: center;
		padding: var(--s-1) var(--s-2);
		border-radius: var(--radius);
		background: var(--stage-raised);
		color: var(--stage-ink);
		font: var(--t-meta);
		font-variant-numeric: tabular-nums;
		white-space: nowrap;
		pointer-events: none;
	}
	.chapter {
		font: var(--t-tiny);
		color: var(--stage-muted);
	}
</style>
