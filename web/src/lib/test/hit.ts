// Where a click lands: the whole-card tests ask what is under a point, as a pointer would.

/** The link or button a click at the centre of `el` reaches (scrolled into view first: a
 * point off screen hits nothing). */
export function controlAt(el: Element | null): Element | null {
	if (!el) return null;
	el.scrollIntoView({ block: 'center', inline: 'center' });
	const r = el.getBoundingClientRect();
	return document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2)?.closest('a, button') ?? null;
}
