// The admin's server values, each said once (its key, how often it is read again): the
// page's sections and the user history page share them.

import { admin } from '@iris/api/client';

export const usersQuery = () => ({ queryKey: ['admin', 'users'], queryFn: admin.listUsers });
export const invitationsQuery = () => ({ queryKey: ['admin', 'invitations'], queryFn: admin.listInvitations });
/** Who is watching now: the presence registry, read every 10 s. */
export const sessionsQuery = () => ({ queryKey: ['admin', 'active-sessions'], queryFn: admin.activeSessions, refetchInterval: 10_000 });
export const watchHistoryQuery = () => ({
	queryKey: ['admin', 'watch-history'],
	queryFn: () => admin.watchHistory(30),
	refetchInterval: 30_000
});
export const storageQuery = () => ({ queryKey: ['admin', 'storage'], queryFn: admin.storage, refetchInterval: 10_000 });
export const remuxQuery = () => ({ queryKey: ['admin', 'remux'], queryFn: admin.listRemux, refetchInterval: 10_000 });
export const auditQuery = () => ({ queryKey: ['admin', 'audit-log'], queryFn: () => admin.auditLog(50), refetchInterval: 30_000 });
/** The trackers, on or off, and how each one's last search went: read every 30 s. */
export const providersQuery = () => ({ queryKey: ['admin', 'providers'], queryFn: admin.providers, refetchInterval: 30_000 });
