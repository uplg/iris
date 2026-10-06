// What went wrong, said plainly and with what to do, once for the whole app: a refusal by its
// name (`code`), no answer by what it means for the person, else the server's own words (Iris
// error messages are written for people). Never a prefix such as "Error:" in front of them.

import { ApiError } from '@iris/api/client';

const BY_CODE: Record<string, string> = {
	// the browser's WebAuthn refusals (DOMException names)
	NotAllowedError: 'The passkey request was cancelled or timed out. Try again when you are ready.',
	AbortError: 'The passkey request was cancelled. Try again when you are ready.',
	// the browser answered a passkey request with no credential (`@iris/api/passkeys`)
	cancelled: 'The passkey request was cancelled. Try again when you are ready.',
	SecurityError: 'Passkeys need this site over https. Open Iris at its usual address.',
	InvalidStateError: 'This device already holds a passkey for your account.',
	// the server's refusals (backend error codes)
	unauthorized: 'You are signed out. Sign in again to continue.',
	forbidden: 'Only an admin, or the person who added it, can do this.',
	client_outdated: 'Iris was updated. Reload the page to get the new version.'
};

/** The sentence for `e`, whatever threw it. */
export function errorText(e: unknown): string {
	const code = e instanceof ApiError ? e.code : e instanceof DOMException ? e.name : undefined;
	const known = code ? BY_CODE[code] : undefined;
	if (known) return known;
	// fetch found no server at all
	if (e instanceof TypeError) return 'Iris did not answer. Check your connection, then try again.';
	if (e instanceof ApiError) {
		if (e.message && e.message !== 'error') return e.message;
		if (e.status === 404) return 'This no longer exists.';
		if (e.status === 502 || e.status === 503 || e.status === 504) return 'Iris is restarting. Try again in a moment.';
		return 'Something went wrong on the server. Try again.';
	}
	return e instanceof Error && e.message ? e.message : 'Something went wrong. Try again.';
}
