import { describe, expect, it } from 'vitest';
import type { LibraryMatch } from '@iris/api/client';
import { matchTarget } from './release.ts';

const match = (over: Partial<LibraryMatch>) =>
	({ collection_id: 'c1', kind: 'tv', episode_count: 0, torrent_count: 1, ...over }) as LibraryMatch;

describe('a library match', () => {
	it('says its episodes on disk, never « 0 episodes » for a pack not split yet', () => {
		expect(matchTarget(match({ episode_count: 8 })).facts).toBe('8 episodes on disk');
		expect(matchTarget(match({ episode_count: 0 })).facts).toBe('On disk');
		expect(matchTarget(match({ episode_count: 0, torrent_count: 2 })).facts).toBe('2 releases on disk');
	});
});
