// Passkeys in the browser (WebAuthn Level 3): the server holds the ceremony, the browser only
// asks the authenticator. Discoverable credentials, so signing in needs no name. Same flow as
// ariane / maison; the routes are Iris's (`/api/auth/passkey/*`, `/api/me/passkeys`).

import type { components } from "./api-types";
import { api, ApiError, type User } from "./client";

export type PasskeyView = components["schemas"]["PasskeyView"];

/** The person closed the system dialog. */
const cancelled = () => new ApiError(0, "cancelled", "cancelled");

/** WebAuthn needs a secure context: HTTPS, or http://localhost. */
export const supported = () =>
  typeof window !== "undefined" && window.isSecureContext && "PublicKeyCredential" in window;

/** Autofill sign-in (`mediation: "conditional"`) is offered by this browser. */
export async function conditionalSupported(): Promise<boolean> {
  if (!supported()) return false;
  const pkc = PublicKeyCredential as unknown as {
    isConditionalMediationAvailable?: () => Promise<boolean>;
  };
  return (await pkc.isConditionalMediationAvailable?.()) ?? false;
}

// base64url, for browsers without the Level 3 JSON helpers
const b64 = {
  decode(s: string): ArrayBuffer {
    const bin = atob(
      s
        .replace(/-/g, "+")
        .replace(/_/g, "/")
        .padEnd(Math.ceil(s.length / 4) * 4, "="),
    );
    return Uint8Array.from(bin, (c) => c.charCodeAt(0)).buffer;
  },
  encode(b: ArrayBuffer): string {
    return btoa(String.fromCharCode(...new Uint8Array(b)))
      .replace(/\+/g, "-")
      .replace(/\//g, "_")
      .replace(/=+$/, "");
  },
};

type Json = Record<string, unknown>;
const PKC = () =>
  PublicKeyCredential as unknown as {
    parseCreationOptionsFromJSON?: (o: Json) => PublicKeyCredentialCreationOptions;
    parseRequestOptionsFromJSON?: (o: Json) => PublicKeyCredentialRequestOptions;
  };

type WithId = Json & { id: string };
type CreationJson = Json & { challenge: string; user: WithId; excludeCredentials?: WithId[] };
type RequestJson = Json & { challenge: string; allowCredentials?: WithId[] };
const withBytes = (c: WithId) => ({ ...c, id: b64.decode(c.id) });

export function creationOptions(o: Json): PublicKeyCredentialCreationOptions {
  const parse = PKC().parseCreationOptionsFromJSON;
  if (parse) return parse(o);
  const x = o as CreationJson;
  return {
    ...x,
    challenge: b64.decode(x.challenge),
    user: withBytes(x.user),
    excludeCredentials: (x.excludeCredentials ?? []).map(withBytes),
  } as unknown as PublicKeyCredentialCreationOptions;
}

export function requestOptions(o: Json): PublicKeyCredentialRequestOptions {
  const parse = PKC().parseRequestOptionsFromJSON;
  if (parse) return parse(o);
  const x = o as RequestJson;
  return {
    ...x,
    challenge: b64.decode(x.challenge),
    allowCredentials: (x.allowCredentials ?? []).map(withBytes),
  } as unknown as PublicKeyCredentialRequestOptions;
}

export function toJSON(c: PublicKeyCredential): Json {
  const cred = c as unknown as { toJSON?: () => Json };
  if (typeof cred.toJSON === "function") return cred.toJSON();
  const response: Json = { clientDataJSON: b64.encode(c.response.clientDataJSON) };
  if (c.response instanceof AuthenticatorAttestationResponse) {
    response.attestationObject = b64.encode(c.response.attestationObject);
    response.transports = c.response.getTransports?.() ?? [];
  } else {
    const r = c.response as AuthenticatorAssertionResponse;
    response.authenticatorData = b64.encode(r.authenticatorData);
    response.signature = b64.encode(r.signature);
    response.userHandle = r.userHandle ? b64.encode(r.userHandle) : null;
  }
  return {
    id: c.id,
    rawId: b64.encode(c.rawId),
    type: c.type,
    response,
    clientExtensionResults: c.getClientExtensionResults(),
    authenticatorAttachment: c.authenticatorAttachment,
  };
}

/** A ceremony begun by the server: its id and the browser's options. */
type Started = { ceremony: string; options: { publicKey: Json; mediation?: string } };

/** Signs in with a passkey. `conditional` waits on the email field's autofill (the browser
 * shows saved passkeys there); `signal` gives that wait up (the user typed a password). */
export async function signIn(opts: { conditional?: boolean; signal?: AbortSignal } = {}): Promise<User> {
  const start = await api.post<Started>("/auth/passkey/login/start", {
    conditional: opts.conditional ?? false,
  });
  const credential = (await navigator.credentials.get({
    publicKey: requestOptions(start.options.publicKey),
    ...(opts.conditional ? { mediation: "conditional" as CredentialMediationRequirement } : {}),
    signal: opts.signal,
  })) as PublicKeyCredential | null;
  if (!credential) throw cancelled();
  return api.post<User>("/auth/passkey/login/finish", {
    ceremony: start.ceremony,
    credential: toJSON(credential),
  });
}

/** Creates a passkey for the signed-in user. `name`: what they call it (else the device's). */
export async function register(name?: string): Promise<PasskeyView> {
  const start = await api.post<Started>("/me/passkeys/register/start");
  const credential = (await navigator.credentials.create({
    publicKey: creationOptions(start.options.publicKey),
  })) as PublicKeyCredential | null;
  if (!credential) throw cancelled();
  return api.post<PasskeyView>("/me/passkeys/register/finish", {
    ceremony: start.ceremony,
    credential: toJSON(credential),
    name,
  });
}

export const passkeys = {
  list: () => api.get<PasskeyView[]>("/me/passkeys"),
  rename: (id: string, name: string) =>
    api.patch<void>(`/me/passkeys/${encodeURIComponent(id)}`, { name }),
  remove: (id: string) => api.delete<void>(`/me/passkeys/${encodeURIComponent(id)}`),
};
