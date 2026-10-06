/** A page's document title: what it shows, then the app (the home page: the app alone). */
export function pageTitle(page: string): string {
	return page === 'Iris' ? page : `${page} · Iris`;
}
