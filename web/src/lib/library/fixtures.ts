// Library shapes for tests, as the backend sends them; each test overrides what it is about.

import type { CollectionListItem, TorrentView } from '@iris/api/client';

export const collection = (over: Partial<CollectionListItem> = {}): CollectionListItem => ({
	id: 'c-1',
	display_title: 'Severance',
	kind: 'tv',
	episode_count: 19,
	torrent_count: 2,
	total_size_bytes: 40 * 1024 ** 3,
	poster_path: '/sev.jpg',
	ghost: false,
	is_anime: false,
	tmdb_id: 95396,
	...over
});

export const torrent = (over: Partial<TorrentView> = {}): TorrentView => ({
	id: 't-1',
	infohash: 'aaa',
	name: 'Severance.S02.1080p.WEB.x264-GRP',
	state: 'live',
	finished: true,
	progress_pct: 100,
	progress_bytes: 2 * 1024 ** 3,
	total_size_bytes: 2 * 1024 ** 3,
	download_speed_bps: 0,
	upload_speed_bps: 0,
	uploaded_bytes: 0,
	uploaded_bytes_total: 3 * 1024 ** 3,
	downloaded_bytes_total: 2 * 1024 ** 3,
	peers: 0,
	files: [{ index: 0, path: 'Severance.S02E01.mkv', size_bytes: 2 * 1024 ** 3 }],
	fetched_at: '2026-10-06T10:00:00Z',
	added_at: '2026-10-01T10:00:00Z',
	added_by: 'u-1',
	added_by_name: 'Léo',
	can_delete: true,
	collection_id: 'c-1',
	kind: 'tv',
	source_provider: 'c411',
	tmdb_verified: true,
	...over
});
