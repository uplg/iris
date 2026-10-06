// The admin's views, its long lists read by day, an invitation's state: said once for the
// views that show them.

import type { ActiveSession, Invitation, UserView } from '@iris/api/client';
import { dayHeading } from '@iris/api/format';

/** The page's views, in tab order; the first is where it opens. */
export const VIEWS = ['activity', 'people', 'system', 'log'] as const;
export type View = (typeof VIEWS)[number];
export const isView = (v: string | null | undefined): v is View => (VIEWS as readonly string[]).includes(v ?? '');

export interface Day<T> {
	key: string;
	/** « Today », « Yesterday », « Saturday 4 October ». */
	heading: string;
	items: T[];
}

/** `items` (newest first) under the day each one happened, in the reader's time zone. */
export function byDay<T>(items: readonly T[], at: (t: T) => string, now = Date.now()): Day<T>[] {
	const days: Day<T>[] = [];
	for (const it of items) {
		const d = new Date(at(it));
		const key = `${d.getFullYear()}-${d.getMonth()}-${d.getDate()}`;
		let day = days.at(-1);
		if (day?.key !== key) {
			day = { key, heading: dayHeading(d.getTime(), now), items: [] };
			days.push(day);
		}
		day.items.push(it);
	}
	return days;
}

/** Not used and not past its expiry: its link still makes an account. */
export const isWaiting = (i: Invitation, now = Date.now()) => !i.consumed_at && new Date(i.expires_at).getTime() > now;

/** The people to pick from in a filter, by name, after the choice of anyone (`any`). */
export function peopleChoices(users: readonly UserView[] | undefined, any: { value: string; label: string }) {
	return [
		any,
		...(users ?? []).toSorted((a, b) => a.display_name.localeCompare(b.display_name)).map((u) => ({ value: u.id, label: u.display_name }))
	];
}

/** Whether semver `a` is older than `b` (pre-release tags ignored); false when either is unreadable. */
export function olderThan(a: string | null | undefined, b: string): boolean {
	const parse = (v: string) => /^(\d+)\.(\d+)\.(\d+)/.exec(v.trim())?.slice(1).map(Number);
	const x = a ? parse(a) : undefined;
	const y = parse(b);
	if (!x || !y) return false;
	for (let i = 0; i < 3; i++) if (x[i] !== y[i]) return x[i] < y[i];
	return false;
}

/** A session heard from longer ago than this has stopped telling where it is. */
export const STALLED_AFTER_S = 20;

export type LiveState = 'playing' | 'paused' | 'buffering' | 'stalled';

/** Where a live session is, from its last heartbeat: playing, paused, buffering, or stalled
 * (silent past {@link STALLED_AFTER_S} while meant to play). */
export function liveState(s: ActiveSession, now: number): LiveState {
	const silent = (now - new Date(s.last_seen_at).getTime()) / 1000;
	if (s.state === 'paused') return 'paused';
	if (silent > STALLED_AFTER_S) return 'stalled';
	return s.state === 'buffering' ? 'buffering' : 'playing';
}
