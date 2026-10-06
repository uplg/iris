// For the account tests: a passkey authenticator that answers without a dialog, the backend's
// ceremony routes (real WebAuthn JSON, so Chromium's parsers read it), and the account's
// default backend. After Maison's test passkeys.

import { vi } from 'vitest';
import type { User } from '@iris/api/client';
import type { PasskeyView } from '@iris/api/passkeys';

export const leonard: User = { id: 'u1', email: 'leonard.k@example.com', display_name: 'Léonard', is_admin: true };
export const alex: User = { id: 'u2', email: 'alex@example.com', display_name: 'Alex', is_admin: false };

const challenge = 'Y2hhbGxlbmdlLWNoYWxsZW5nZQ';
const creationJson = {
	rp: { id: 'localhost', name: 'Iris' },
	user: { id: 'dXNlci1oYW5kbGU', name: 'leonard', displayName: 'Léonard' },
	challenge,
	pubKeyCredParams: [{ type: 'public-key', alg: -7 }],
	timeout: 300000,
	excludeCredentials: [],
	authenticatorSelection: { residentKey: 'required', requireResidentKey: true, userVerification: 'required' },
	attestation: 'none'
};
const credentialJson = { id: 'Y3JlZA', rawId: 'Y3JlZA', type: 'public-key', response: {} };
const credential = { id: credentialJson.id, toJSON: () => credentialJson } as unknown as Credential;

/** The device's authenticator: answers with a credential, or with `outcome` (null: closed). */
export function authenticator(outcome?: null | Error) {
	const answer = async () => {
		if (outcome === undefined) return credential;
		if (outcome instanceof Error) throw outcome;
		return outcome;
	};
	return { create: vi.spyOn(navigator.credentials, 'create').mockImplementation(answer) };
}

export const passkey = (over: Partial<PasskeyView> = {}): PasskeyView => ({
	id: 'k1',
	name: 'iPhone',
	backed_up: true,
	created_at: new Date(2026, 9, 2, 20).toISOString(),
	last_used_at: null,
	...over
});

/** The registration ceremony, ending with `key`. */
export const registerRoutes = (key: PasskeyView = passkey({ id: 'k3', name: 'MacBook' })) => ({
	'POST /me/passkeys/register/start': { ceremony: 'c2', options: { publicKey: creationJson } },
	'POST /me/passkeys/register/finish': key
});

export const noContent = () => new Response(null, { status: 204 });
