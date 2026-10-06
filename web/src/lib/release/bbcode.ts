// Tracker BBCode, parsed into a tree the page renders itself (never as HTML): the tags torr9
// and V3X write ([b] [i] [u] [center] [justify] [size] [color] [uicolor] [url] [img] [table]
// [tr] [td]). A stack parser, since trackers nest deeply; unknown or unbalanced tags stay as
// text so nothing written is lost. Colors and sizes are the tracker's palette, not ours: the
// renderer drops them, so the text stays readable in both themes.

export type BBNode = { type: 'text'; value: string } | { type: 'tag'; name: string; arg: string | null; children: BBNode[] };

// `[b]`, `[/b]`, `[color=#3d85c6]`, `[img scale=30%]` (extra attributes read and dropped)
const TAG_RE = /\[(\/?)([a-zA-Z]+)(?:=([^\] ]*))?(?:\s+[^\]]*)?\]/g;

export function parseBBCode(input: string): BBNode[] {
	type Frame = { name: string; arg: string | null; children: BBNode[] };
	const root: Frame = { name: '', arg: null, children: [] };
	const stack: Frame[] = [root];
	const top = () => stack[stack.length - 1];
	let cursor = 0;
	for (const m of input.matchAll(TAG_RE)) {
		const start = m.index ?? 0;
		if (start > cursor) top().children.push({ type: 'text', value: input.slice(cursor, start) });
		cursor = start + m[0].length;
		const name = m[2].toLowerCase();
		if (m[1] !== '/') {
			stack.push({ name, arg: m[3] ?? null, children: [] });
			continue;
		}
		const at = stack.findLastIndex((f, i) => i > 0 && f.name === name);
		if (at < 0) {
			top().children.push({ type: 'text', value: m[0] });
			continue;
		}
		// tags left open inside the one closed end with it
		while (stack.length > at) {
			const f = stack.pop()!;
			top().children.push({ type: 'tag', name: f.name, arg: f.arg, children: f.children });
		}
	}
	if (cursor < input.length) top().children.push({ type: 'text', value: input.slice(cursor) });
	// never closed: the opening tag as text, its content kept
	while (stack.length > 1) {
		const open = stack.pop()!;
		top().children.push({ type: 'text', value: `[${open.name}${open.arg ? `=${open.arg}` : ''}]` }, ...open.children);
	}
	return root.children;
}

export function nodesToText(nodes: readonly BBNode[]): string {
	return nodes.map((n) => (n.type === 'text' ? n.value : nodesToText(n.children))).join('');
}

/** A tracker-written link: only schemes that cannot run script. */
export function safeHref(raw: string | null | undefined): string | null {
	const v = (raw ?? '').trim();
	return /^(https?:|magnet:)/i.test(v) ? v : null;
}

/** A tracker-written image: http(s) only. */
export function safeSrc(raw: string): string | null {
	const v = raw.trim();
	return /^https?:\/\//i.test(v) ? v : null;
}

/** Table rows and cells only: the line breaks between them are not content. */
export const structural = (nodes: readonly BBNode[], name: string) => nodes.filter((c) => c.type === 'tag' && c.name === name);

/** Lines drawn with separator glyphs (━━━, ···) are decoration, not text. */
export function stripSeparators(input: string): string {
	return input
		.split('\n')
		.filter((line) => {
			const bare = line.replace(/\[[^\]]+\]/g, '').trim();
			if (!bare) return true;
			const glyphs = bare.match(/[━—–·•⋯]/g)?.length ?? 0;
			return glyphs / bare.length < 0.5;
		})
		.join('\n');
}

export const prepareBBCode = (source: string) => parseBBCode(stripSeparators(source.replace(/\r\n?/g, '\n')));
