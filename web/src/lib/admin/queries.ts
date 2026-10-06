// The admin's server values, each said once (its key, how often it is read again): the
// page's views and a person's page share them. The long lists come a page at a time
// (`PAGE` rows, « Show more » asks for the next).

import { admin, type AuditLogEntry, type AuditLogFilter, type WatchHistoryEntry, type WatchHistoryFilter } from '@iris/api/client';

/** Rows a long list shows first, and adds on each « Show more ». */
export const PAGE = 20;

/** The next page's offset, none once a page came back short. */
const nextOffset = <T>(last: T[], all: T[][]) => (last.length < PAGE ? undefined : all.length * PAGE);

export const usersQuery = () => ({ queryKey: ['admin', 'users'], queryFn: admin.listUsers, refetchInterval: 60_000 });
export const invitationsQuery = () => ({ queryKey: ['admin', 'invitations'], queryFn: admin.listInvitations });
/** Who is watching now: the presence registry, read every 10 s. */
export const sessionsQuery = () => ({ queryKey: ['admin', 'active-sessions'], queryFn: admin.activeSessions, refetchInterval: 10_000 });
/** The household's plays, one person's or one kind's, newest first, read again every 30 s. */
export const playsQuery = (f: WatchHistoryFilter) => ({
	queryKey: ['admin', 'watch-history', f.user_id ?? null, f.kind ?? null],
	queryFn: ({ pageParam }: { pageParam: number }) => admin.watchHistory({ ...f, limit: PAGE, offset: pageParam }),
	initialPageParam: 0,
	getNextPageParam: (last: WatchHistoryEntry[], all: WatchHistoryEntry[][]) => nextOffset(last, all),
	refetchInterval: 30_000
});
export const storageQuery = () => ({ queryKey: ['admin', 'storage'], queryFn: admin.storage, refetchInterval: 10_000 });
export const remuxQuery = () => ({ queryKey: ['admin', 'remux'], queryFn: admin.listRemux, refetchInterval: 10_000 });
/** What was changed or deleted, by whom: one family of actions, one person's, read every 30 s. */
export const auditQuery = (f: AuditLogFilter) => ({
	queryKey: ['admin', 'audit-log', f.action ?? null, f.actor_id ?? null],
	queryFn: ({ pageParam }: { pageParam: number }) => admin.auditLog({ ...f, limit: PAGE, offset: pageParam }),
	initialPageParam: 0,
	getNextPageParam: (last: AuditLogEntry[], all: AuditLogEntry[][]) => nextOffset(last, all),
	refetchInterval: 30_000
});
/** The trackers, on or off, and how each one's last search went: read every 30 s. */
export const providersQuery = () => ({ queryKey: ['admin', 'providers'], queryFn: admin.providers, refetchInterval: 30_000 });
