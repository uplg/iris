// The server's refusals a client acts on, recognised in this one place: by their code, and, for
// the ones the server sends under a generic code (`bad_request`, `conflict`), by the words of
// their message. The codes named here and not sent yet are the ones the server is to add; once
// it does, the words fall away.

type Said = { code?: string; message: string };

function said(e: unknown): Said | null {
	if (typeof e !== 'object' || e === null || !('message' in e) || typeof e.message !== 'string') return null;
	const code = 'code' in e && typeof e.code === 'string' ? e.code : undefined;
	return { code, message: e.message };
}

const by = (codes: readonly string[], words: RegExp) => (e: unknown) => {
	const s = said(e);
	return !!s && ((s.code !== undefined && codes.includes(s.code)) || words.test(s.message));
};

/** The file (or its head) is not on disk yet: poll the probe or the manifest again. */
export const isNotOnDisk = by(['not_on_disk'], /not yet on disk|download in progress|not yet probable/i);

/** Nobody shares the release: its swarm is dead, nothing will arrive. */
export const isNoSeeders = by(['dead_torrent', 'stalled'], /no seeders|^stalled:/i);

export type RegisterRefusal = 'invitation' | 'email_taken' | 'email_invalid' | 'password';

/** Which part of a registration the server refused. */
export function registerRefusal(e: unknown): RegisterRefusal | null {
	const s = said(e);
	if (!s) return null;
	if (s.code === 'password_too_short') return 'password';
	if (s.code === 'invalid_invitation' || s.code === 'invitation_used' || /invitation/i.test(s.message)) return 'invitation';
	if (s.code === 'email_taken' || /already registered/i.test(s.message)) return 'email_taken';
	if (s.code === 'invalid_email' || /email/i.test(s.message)) return 'email_invalid';
	return null;
}
