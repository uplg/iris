// Who is signed in, for any web app (framework-free; each app mirrors `state` into its own
// reactivity). One bootstrap, one keep-alive, one way to sign in or out.
//
// Timers (approved, CLAUDE.md web rule): the bootstrap's backoff while the server can't be
// reached, and the 25 min keep-alive that rotates the access cookie before its 1 h TTL (else a
// long playback's byte-range requests silently 401).

import { ApiError, AUTH_EXPIRED_EVENT, auth, type User } from "./client";

export type SessionState =
  | { status: "loading"; retrying: boolean }
  | { status: "signed_out" }
  | { status: "signed_in"; user: User };

const KEEP_ALIVE_MS = 25 * 60_000;
const FIRST_RETRY_MS = 2_000;
const MAX_RETRY_MS = 30_000;

/** A genuine auth death: the refresh token is gone. Anything else (429, 5xx, no network) leaves
 * the session alive. */
const authDead = (e: unknown) => e instanceof ApiError && (e.status === 401 || e.status === 403);

export class Session {
  state: SessionState = { status: "loading", retrying: false };
  #listeners = new Set<(s: SessionState) => void>();
  #retry: ReturnType<typeof setTimeout> | undefined;
  #keepAlive: ReturnType<typeof setInterval> | undefined;
  #onExpired = () => this.#set({ status: "signed_out" });

  /** Calls `fn` now and on every change; returns the unsubscribe. */
  subscribe(fn: (s: SessionState) => void): () => void {
    this.#listeners.add(fn);
    fn(this.state);
    return () => this.#listeners.delete(fn);
  }

  /** Learn who is signed in, retrying with backoff while the server can't say. */
  start() {
    window.addEventListener(AUTH_EXPIRED_EVENT, this.#onExpired);
    void this.#bootstrap(FIRST_RETRY_MS);
  }

  stop() {
    window.removeEventListener(AUTH_EXPIRED_EVENT, this.#onExpired);
    clearTimeout(this.#retry);
    clearInterval(this.#keepAlive);
  }

  /** Signed in by any door (password, passkey, registration). */
  signedIn(user: User) {
    this.#set({ status: "signed_in", user });
  }

  async login(email: string, password: string) {
    this.signedIn(await auth.login(email, password));
  }

  async register(token: string, email: string, password: string) {
    this.signedIn(await auth.register(token, email, password));
  }

  async logout() {
    await auth.logout();
    this.#set({ status: "signed_out" });
  }

  async #bootstrap(delayMs: number) {
    let verdict: SessionState | undefined;
    try {
      verdict = { status: "signed_in", user: await auth.me() };
    } catch {
      try {
        verdict = { status: "signed_in", user: await auth.refresh() };
      } catch (e) {
        if (authDead(e)) verdict = { status: "signed_out" };
      }
    }
    if (verdict) return this.#set(verdict);
    this.#set({ status: "loading", retrying: true });
    this.#retry = setTimeout(
      () => void this.#bootstrap(Math.min(delayMs * 2, MAX_RETRY_MS)),
      delayMs,
    );
  }

  #set(next: SessionState) {
    const was = this.state.status;
    this.state = next;
    if (next.status === "signed_in" && was !== "signed_in") {
      clearInterval(this.#keepAlive);
      this.#keepAlive = setInterval(() => {
        auth.refresh().then(
          (user) => this.#set({ status: "signed_in", user }),
          (e: unknown) => authDead(e) && this.#set({ status: "signed_out" }),
        );
      }, KEEP_ALIVE_MS);
    }
    if (next.status !== "signed_in") clearInterval(this.#keepAlive);
    for (const fn of this.#listeners) fn(next);
  }
}
