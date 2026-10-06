<script lang="ts">
	// The one slider (APG slider, through Bits UI; docs/ux.md § 3). The value is spoken in words
	// (`valueText`), Page Up / Page Down move `page` steps, the 44 px band can be touched
	// anywhere. `oncommit` gets the value on release and on each key press; `send` is a call
	// that may fail (said the one way, Gesture, and the thumb goes back to `value`), on release,
	// or with `live` on every move too. No timers: whatever it drives answers at once.
	// One that cannot act stays in place, not operable, and says why (`reason`).
	import { Slider } from 'bits-ui';
	import { Gesture, unavailable } from '#lib/gesture.svelte.ts';

	interface Props {
		label: string;
		value: number;
		min?: number;
		max?: number;
		step?: number;
		/** Page Up / Page Down move this many steps (APG: a larger step). */
		page?: number;
		/** What the value means, spoken and shown: "Volume 40 %". */
		valueText: (v: number) => string;
		oncommit?: (v: number) => void;
		send?: (v: number) => Promise<unknown>;
		/** Send while dragging too, not only on release. */
		live?: boolean;
		/** The id of what says why it cannot act now; none: it can. */
		reason?: string;
		/** Hide the visible label when the card already says it; it stays for readers. */
		hideLabel?: boolean;
	}
	let {
		label,
		value,
		min = 0,
		max = 100,
		step = 1,
		page = 10,
		valueText,
		oncommit,
		send,
		live = false,
		reason,
		hideLabel = false
	}: Props = $props();
	const g = new Gesture();
	/** Each send its own key: a live slider may have two in flight. */
	let sends = 0;

	// a local value while dragging; the given value wins again when it changes
	let draft = $derived(value);
	const id = $props.id();

	function push(v: number, final: boolean) {
		if (final) oncommit?.(v);
		if (!send || (!final && !live)) return;
		void g.run(() => send(v), undefined, `send-${++sends}`, {
			// said by the gesture; the thumb goes back to what is
			refused: () => void (draft = value)
		});
	}

	function onkeydown(e: KeyboardEvent) {
		const dir = e.key === 'PageUp' ? 1 : e.key === 'PageDown' ? -1 : 0;
		if (!dir || reason) return;
		e.preventDefault();
		draft = Math.min(max, Math.max(min, draft + dir * page * step));
		push(draft, true);
	}
</script>

<div class="range">
	<div class="head" class:sr-only={hideLabel}>
		<span id="{id}-label">{label}</span>
		<output for="{id}-thumb" class="value">{valueText(draft)}</output>
	</div>
	<Slider.Root
		type="single"
		bind:value={() => draft, (v) => !reason && (draft = v)}
		{min}
		{max}
		{step}
		onValueChange={(v) => !reason && push(v, false)}
		onValueCommit={(v) => !reason && push(v, true)}
		class="range-band"
	>
		<span class="track">
			<Slider.Range class="range-fill" />
		</span>
		<!-- the thumb drawn here, so `unavailable` has the last word over Bits' aria-disabled
		     (Bits' own disabled would take it out of the Tab order: the focus would fall) -->
		<Slider.Thumb index={0} id="{id}-thumb" aria-labelledby="{id}-label" aria-valuetext={valueText(draft)} {onkeydown}>
			{#snippet child({ props })}
				<span {...props} class="range-thumb" {...unavailable(reason)}></span>
			{/snippet}
		</Slider.Thumb>
	</Slider.Root>
</div>

<style>
	.range {
		display: grid;
		gap: var(--s-2);
	}
	.head {
		display: flex;
		justify-content: space-between;
		gap: var(--s-3);
		font: var(--t-secondary);
		color: var(--ink-muted);
	}
	.value {
		color: var(--ink);
		font-variant-numeric: tabular-nums;
	}
	/* a 44 px band to grab (touching it places the thumb, WCAG 2.5.7), a thin track inside */
	.range :global(.range-band) {
		position: relative;
		display: flex;
		align-items: center;
		min-height: var(--control-h);
		user-select: none;
	}
	.track {
		position: relative;
		flex: 1;
		height: var(--track-h);
		border-radius: var(--radius-pill);
		background: var(--ground-raised);
		overflow: hidden;
	}
	.range :global(.range-fill) {
		position: absolute;
		height: 100%;
		background: var(--accent);
	}
	.range :global(.range-thumb) {
		display: block;
		width: var(--thumb);
		height: var(--thumb);
		border-radius: 50%;
		background: var(--surface);
		border: 2px solid var(--accent);
		box-shadow: var(--shadow);
		cursor: grab;
	}
	.range :global(.range-thumb:focus-visible) {
		outline: var(--focus-ring);
		outline-offset: var(--focus-offset);
	}
	/* unreachable or off: the last known value, greyed */
	.range:has(:global([aria-disabled='true'])) .value {
		color: var(--ink-muted);
	}
	.range :global(.range-band:has([aria-disabled='true'])) {
		opacity: var(--disabled-opacity);
	}
</style>
