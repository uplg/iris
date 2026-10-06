// The app's own addresses, written once.

/** Where a file plays. */
export const watchHref = (infohash: string, fileIdx: number) => `/watch/${infohash}/${fileIdx}`;

/** A person's page in the admin: what they watched. */
export const personHref = (userId: string) => `/admin/users/${userId}/history`;

/** `to` as a path on this site, or home: a backslash (read as `/` by URL parsing) or another
 * origin never leaves it. */
export function sameSite(to: string | null | undefined, origin: string): string {
	if (!to?.startsWith('/') || to.includes('\\')) return '/';
	try {
		const u = new URL(to, origin);
		return u.origin === origin ? u.pathname + u.search + u.hash : '/';
	} catch {
		return '/';
	}
}
