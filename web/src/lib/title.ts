/** A page's document title: what it shows, then the app (the home page: the app alone). */
export function pageTitle(page: string): string {
	return page === 'Iris' ? page : `${page} · Iris`;
}

/** A time as people say it: a clock time today, a weekday this week, else a date. */
export function when(ms: number, now = Date.now()): string {
	const d = new Date(ms);
	const days = Math.floor((now - ms) / 86_400_000);
	if (days < 1 && new Date(now).getDate() === d.getDate()) return d.toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit' });
	if (days < 6) return d.toLocaleDateString('en-GB', { weekday: 'long' });
	return d.toLocaleDateString('en-GB', { day: 'numeric', month: 'short' });
}
