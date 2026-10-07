import { IRIS_WEB_VERSION } from '@iris/api/client';
// For the admin tests: a household of realistic size (25 accounts, 300 plays of series and
// films, three people watching, a long audit log, nine trackers, invitations waiting, used
// and expired) and a stand-in backend that pages and filters it as the server does.

import type {
	ActiveSession,
	AuditLogEntry,
	Invitation,
	ProviderStatus,
	RemuxJobView,
	StorageStats,
	UserView,
	WatchHistoryEntry
} from '@iris/api/client';
import { stubApi, type ApiCall } from '#lib/test/api.ts';

const MIN = 60_000;
const HOUR = 60 * MIN;
const DAY = 24 * HOUR;
const ago = (ms: number) => new Date(Date.now() - ms).toISOString();
const ahead = (ms: number) => new Date(Date.now() + ms).toISOString();

const NAMES = [
	'Léonard',
	'Alex',
	'Camille',
	'Sam',
	'Léa',
	'Hugo',
	'Inès',
	'Jules',
	'Chloé',
	'Nathan',
	'Manon',
	'Lucas',
	'Emma',
	'Louis',
	'Jade',
	'Gabriel',
	'Zoé',
	'Arthur',
	'Lina',
	'Raphaël',
	'Rose',
	'Maël',
	'Anna',
	'Tom',
	'Marie-Charlotte de Villeneuve-Lefebvre'
];
const slug = (name: string) =>
	name
		.normalize('NFD')
		.replace(/[̀-ͯ]/g, '')
		.toLowerCase()
		.replace(/[^a-z]+/g, '.');

export const people: UserView[] = NAMES.map((name, i) => ({
	id: `u${i + 1}`,
	email: `${slug(name)}@example.com`,
	display_name: name,
	is_admin: i === 0 || i === 2,
	created_at: ago((400 - i * 9) * DAY),
	last_played_at: null,
	plays: 0
}));

interface Title {
	id: string;
	title: string;
	kind: 'tv' | 'movie';
	poster: string | null;
	year?: number;
	release: string;
	episodes?: { season: number; episode: number; name?: string; absolute?: number }[];
	minutes: number;
}

const ep = (season: number, names: string[]) => names.map((name, i) => ({ season, episode: i + 1, name }));
export const TITLES: Title[] = [
	{
		id: 'c-severance',
		title: 'Severance',
		kind: 'tv',
		poster: '/severance.jpg',
		release: 'Severance.S02.1080p.ATVP.WEB-DL.DDP5.1.H.264-FLUX',
		episodes: ep(2, ['Hello, Ms. Cobel', 'Goodbye, Mrs. Selvig', 'Who Is Alive?', 'Woe’s Hollow', 'Trojan’s Horse', 'Attila']),
		minutes: 52
	},
	{
		id: 'c-bear',
		title: 'The Bear',
		kind: 'tv',
		poster: '/bear.jpg',
		release: 'The.Bear.S03.MULTI.1080p.WEB.H264-FW',
		episodes: ep(3, ['Tomorrow', 'Next', 'Doors', 'Violet', 'Children']),
		minutes: 34
	},
	{
		id: 'c-shogun',
		title: 'Shōgun',
		kind: 'tv',
		poster: '/shogun.jpg',
		release: 'Shogun.2024.S01.VOSTFR.2160p.DSNP.WEB-DL.DV.HDR.H265-GROUP',
		episodes: ep(1, ['Anjin', 'Servants of Two Masters', 'Tomorrow Is Tomorrow', 'The Eightfold Fence']),
		minutes: 58
	},
	{
		id: 'c-onepiece',
		title: 'One Piece',
		kind: 'tv',
		poster: '/onepiece.jpg',
		release: '[Erai-raws] One Piece - 1150 ~ 1156 [1080p][Multiple Subtitle]',
		episodes: [1150, 1151, 1152, 1153, 1154, 1155, 1156].map((n, i) => ({ season: 22, episode: i + 1, absolute: n })),
		minutes: 24
	},
	{
		id: 'c-slowhorses',
		title: 'Slow Horses',
		kind: 'tv',
		poster: null,
		release: 'Slow.Horses.S04.FRENCH.720p.ATVP.WEB-DL.x264-NoTag',
		episodes: ep(4, ['Identity Theft', 'Hello Goodbye', 'Missing', 'Cleaning Up']),
		minutes: 46
	},
	{
		id: 'c-bureau',
		title: 'Le Bureau des Légendes',
		kind: 'tv',
		poster: '/bureau.jpg',
		release: 'Le.Bureau.des.Legendes.S05.FRENCH.1080p.WEB.x264-GROUP',
		episodes: ep(5, ['Épisode 1', 'Épisode 2', 'Épisode 3']),
		minutes: 55
	},
	{
		id: 'c-dune',
		title: 'Dune: Part Two',
		kind: 'movie',
		poster: '/dune2.jpg',
		year: 2024,
		release: 'Dune.Part.Two.2024.2160p.UHD.BluRay.x265-GROUP',
		minutes: 166
	},
	{
		id: 'c-anatomie',
		title: 'Anatomie d’une chute',
		kind: 'movie',
		poster: '/anatomie.jpg',
		year: 2023,
		release: 'Anatomie.d.une.chute.2023.FRENCH.1080p.BluRay.x264-GROUP',
		minutes: 151
	},
	{
		id: 'c-pastlives',
		title: 'Past Lives',
		kind: 'movie',
		poster: '/pastlives.jpg',
		year: 2023,
		release: 'Past.Lives.2023.1080p.WEB.H264-GROUP',
		minutes: 106
	},
	{
		id: 'c-zone',
		title: 'The Zone of Interest',
		kind: 'movie',
		poster: null,
		year: 2023,
		release: 'The.Zone.of.Interest.2023.MULTI.1080p.BluRay.x264-GROUP',
		minutes: 105
	},
	{
		id: 'c-perfectdays',
		title: 'Perfect Days',
		kind: 'movie',
		poster: '/perfectdays.jpg',
		year: 2023,
		release: 'Perfect.Days.2023.VOSTFR.1080p.WEB.x264-GROUP',
		minutes: 124
	}
];
/** A release the library could not place under any title. */
const LOOSE = 'Home.Movies.Xmas.2019.1080p.x264.mkv';

/** A deterministic sequence, so every run draws the same household. */
function seeded(seed: number) {
	let s = seed;
	return () => {
		s = (s * 1_103_515_245 + 12_345) % 2_147_483_648;
		return s / 2_147_483_648;
	};
}

function makePlays(n: number): WatchHistoryEntry[] {
	const rand = seeded(7);
	const out: WatchHistoryEntry[] = [];
	// the newest a few minutes ago, then spread over about two months
	let at = Date.now() - 4 * MIN;
	for (let i = 0; i < n; i++) {
		const who = people[Math.floor(rand() * 18)];
		const loose = i % 37 === 11;
		const t = TITLES[Math.floor(rand() * TITLES.length)];
		const pick = Math.floor(rand() * (t.episodes?.length ?? 1));
		const e = !loose && t.episodes ? t.episodes[pick] : undefined;
		const minutes = loose ? 12 : t.minutes;
		const completed = rand() < 0.45;
		const position = completed ? minutes * 60 - 30 : Math.floor(rand() * minutes * 60);
		const hash = `${(i + 1).toString(16).padStart(8, '0')}${'ab'.repeat(16)}`;
		out.push({
			user_id: who.id,
			display_name: who.display_name,
			infohash: hash,
			file_idx: e ? e.episode - 1 : 0,
			torrent_name: loose ? LOOSE : t.release,
			file_path: loose
				? LOOSE
				: e
					? `${t.release}/${t.release.replace(/S\d+/, `S${String(e.season).padStart(2, '0')}E${String(e.episode).padStart(2, '0')}`)}.mkv`
					: `${t.release}.mkv`,
			tmdb_id: loose ? null : 1000 + i,
			tmdb_verified: !loose && t.poster !== null,
			kind: loose ? null : t.kind,
			position_seconds: position,
			duration_seconds: minutes * 60,
			completed,
			last_watched_at: new Date(at).toISOString(),
			poster_path: loose ? null : t.poster,
			deleted: !loose && i > 200 && i % 5 === 0,
			collection_id: loose ? null : t.id,
			collection_title: loose ? null : t.title,
			season: e?.season ?? null,
			episode: e ? e.episode : null,
			absolute_episode: e?.absolute ?? null,
			episode_title: e?.name ?? null,
			year: t.year ?? null
		});
		at -= (5 + rand() * 300) * MIN;
	}
	return out;
}

export const plays = makePlays(300);
for (const u of people) {
	const mine = plays.filter((p) => p.user_id === u.id);
	u.last_played_at = mine[0]?.last_watched_at ?? null;
	u.plays = mine.length;
}

const session = (who: UserView, t: Title, e: number, position: number, more: Partial<ActiveSession>): ActiveSession => {
	const x = t.episodes?.[e];
	return {
		user_id: who.id,
		display_name: who.display_name,
		infohash: `live${who.id}`.padEnd(40, '0'),
		file_idx: x ? x.episode - 1 : 0,
		torrent_name: t.release,
		file_path: x ? `${t.release}/${t.release.replace(/S\d+/, `S0${x.season}E0${x.episode}`)}.mkv` : `${t.release}.mkv`,
		tmdb_id: 42,
		tmdb_verified: t.poster !== null,
		kind: t.kind,
		position_seconds: position,
		duration_seconds: t.minutes * 60,
		state: 'playing',
		client: 'web',
		client_version: IRIS_WEB_VERSION,
		started_at: ago(position * 1000 + 2 * MIN),
		last_seen_at: ago(4_000),
		poster_path: t.poster,
		collection_id: t.id,
		collection_title: t.title,
		season: x?.season ?? null,
		episode: x?.episode ?? null,
		absolute_episode: x?.absolute ?? null,
		episode_title: x?.name ?? null,
		year: t.year ?? null,
		...more
	};
};
/** Alex on the web (playing), Sam on the TV (paused), Marie-Charlotte buffering on an old app. */
export const sessions: ActiveSession[] = [
	session(people[1], TITLES[0], 3, 42 * 60 + 14, { client: 'web', client_version: IRIS_WEB_VERSION, browser: 'Firefox · macOS' }),
	session(people[3], TITLES[6], 0, 3734, { state: 'paused', client: 'tv', client_version: IRIS_WEB_VERSION, browser: null }),
	session(people[24], TITLES[3], 6, 9 * 60 + 5, { state: 'buffering', client: 'tv', client_version: '1.3.0', browser: null })
];

const ACTIONS: [string, string | null][] = [
	['torrent.delete', 'Dune.Part.Two.2024.2160p.UHD.BluRay.x265-GROUP'],
	['user.password_reset', 'alex@example.com'],
	['gc.evict', '4 torrent(s) evicted, 48318382080 bytes freed'],
	['remux.wipe', '2147483648 bytes freed'],
	['provider.disable', 'tr4ker'],
	['provider.enable', 'tr4ker'],
	['user.display_name_update', 'Camille'],
	['user.delete', 'old.friend@example.com'],
	['user.password_change', null],
	['torrent.delete', 'The.Bear.S03.MULTI.1080p.WEB.H264-FW']
];
export const audit: AuditLogEntry[] = Array.from({ length: 60 }, (_, i) => {
	const [action, details] = ACTIONS[i % ACTIONS.length];
	const actor = people[i % 7 === 3 ? 1 : i % 5 === 2 ? 2 : 0];
	return {
		id: 1000 - i,
		actor_id: actor.id,
		actor_display_name: actor.display_name,
		action,
		resource_type: action.split('.')[0],
		resource_id: null,
		details,
		created_at: ago(i * 9 * HOUR + 17 * MIN)
	};
});

const searched = (latency: number, error?: string) => ({ at: ago(3 * MIN), latency_ms: latency, ...(error ? { error } : {}) });
export const trackers: ProviderStatus[] = [
	{ id: 'c411', kind: 'c411', enabled: true, configured: true, last_search: searched(420) },
	{ id: 'yggtorrent', kind: 'torznab', enabled: true, configured: true, last_search: searched(1250) },
	{ id: 'seedpool', kind: 'unit3d', enabled: true, configured: true, last_search: searched(610) },
	{ id: 'v3x', kind: 'v3x', enabled: true, configured: true, last_search: searched(880) },
	{ id: 'tr4ker', kind: 'tr4ker', enabled: true, configured: true, last_search: searched(8000, 'provider error: timed out after 8s') },
	{ id: 'nyaa', kind: 'nyaa', enabled: true, configured: true, last_search: searched(340) },
	{ id: 'torrentleech', kind: 'torznab', enabled: true, configured: true, last_search: searched(990) },
	{ id: 'oldtracker', kind: 'unit3d', enabled: false, configured: true, last_search: searched(15000, 'timed out') },
	{ id: 'tos', kind: 'unit3d', enabled: false, configured: false }
];

export const invitations: Invitation[] = [
	{ id: 'i1', created_at: ago(1 * DAY), created_by: 'u1', expires_at: ahead(6 * DAY) },
	{ id: 'i2', created_at: ago(5 * DAY), created_by: 'u3', expires_at: ahead(20 * HOUR) },
	...Array.from({ length: 9 }, (_, i) => ({
		id: `i-used-${i}`,
		created_at: ago((30 + i * 20) * DAY),
		created_by: 'u1',
		expires_at: ago((23 + i * 20) * DAY),
		consumed_at: ago((29 + i * 20) * DAY),
		consumed_by: people[10 + i].id
	})),
	{ id: 'i-x1', created_at: ago(40 * DAY), created_by: 'u1', expires_at: ago(33 * DAY) },
	{ id: 'i-x2', created_at: ago(90 * DAY), created_by: 'u3', expires_at: ago(83 * DAY) }
];

const GB = 1024 ** 3;
export const storage: StorageStats = {
	used_bytes: 1_610 * GB,
	max_storage_bytes: 2_000 * GB,
	threshold_bytes: 1_700 * GB,
	target_bytes: 1_400 * GB,
	threshold_pct: 85,
	target_pct: 70,
	torrent_count: 214,
	total_uploaded_bytes: 9_400 * GB,
	total_downloaded_bytes: 6_100 * GB
};

export const remux: RemuxJobView[] = TITLES.slice(0, 9).map((t, i) => ({
	key: `${'cd'.repeat(20)}_${i}`,
	infohash: 'cd'.repeat(20),
	file_idx: i,
	torrent_name: t.release,
	in_flight: i === 2,
	size_bytes: i === 2 ? 0 : (1 + i) * 700 * 1024 ** 2,
	mtime: Math.floor((Date.now() - i * 7 * HOUR) / 1000)
}));

/** One page of `rows`, as the server cuts it (`limit` defaults to 50). */
function paged<T>(rows: T[], q: URLSearchParams): T[] {
	const limit = Number(q.get('limit') ?? 50);
	const offset = Number(q.get('offset') ?? 0);
	return rows.slice(offset, offset + limit);
}

/** The household's backend: every admin route, the long lists paged and filtered. */
export function household(more: Record<string, unknown> = {}, data = { plays, sessions, audit }) {
	return stubApi(
		{
			'GET /admin/users': people,
			'GET /admin/invitations': invitations,
			'GET /admin/active-sessions': data.sessions,
			'GET /admin/storage': storage,
			'GET /admin/remux': remux,
			'GET /admin/providers': trackers,
			...more
		},
		(c: ApiCall) => {
			const [path, query = ''] = c.path.split('?');
			const q = new URLSearchParams(query);
			if (c.method === 'GET' && path === '/admin/watch-history') {
				const user = q.get('user_id');
				const kind = q.get('kind');
				return paged(
					data.plays.filter((p) => (!user || p.user_id === user) && (!kind || p.kind === kind)),
					q
				);
			}
			if (c.method === 'GET' && path === '/admin/audit-log') {
				const action = q.get('action');
				const actor = q.get('actor_id');
				return paged(
					data.audit.filter(
						(e) => (!action || e.action === action || e.action.startsWith(`${action}.`)) && (!actor || e.actor_id === actor)
					),
					q
				);
			}
			return undefined;
		}
	);
}
