// The bench's outside world, on one local port: a Torznab indexer listing the fixtures (the
// backend grabs them through its real provider path), the IPTV catalogue Live TV reads, and the
// HLS directory a looping ffmpeg writes the live channel into.
import { createServer, type Server } from 'node:http';
import { readFile } from 'node:fs/promises';
import { extname, join, normalize } from 'node:path';
import type { AddressInfo } from 'node:net';
import type { BuiltTorrent } from './torrent.ts';

export interface IndexedRelease {
	id: string;
	torrent: BuiltTorrent;
	tv: boolean;
}

const xml = (s: string) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');

function rss(base: string, items: IndexedRelease[]): string {
	const body = items
		.map(
			(r) => `<item>
<title>${xml(r.torrent.name.replace(/\.(mp4|mkv)$/, ''))}</title>
<guid isPermaLink="false">${r.id}</guid>
<link>${base}/dl/${r.id}.torrent</link>
<pubDate>${new Date().toUTCString()}</pubDate>
<size>${r.torrent.size}</size>
<category>${r.tv ? 5040 : 2040}</category>
<enclosure url="${base}/dl/${r.id}.torrent" length="${r.torrent.size}" type="application/x-bittorrent" />
<torznab:attr name="category" value="${r.tv ? 5040 : 2040}" />
<torznab:attr name="seeders" value="5" />
<torznab:attr name="leechers" value="0" />
<torznab:attr name="infohash" value="${r.torrent.infohash}" />
</item>`
		)
		.join('\n');
	return `<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0" xmlns:torznab="http://torznab.com/schemas/2015/feed"><channel>
<torznab:response offset="0" total="${items.length}" />
${body}
</channel></rss>`;
}

/** Every word of the query (3+ letters) somewhere in the release name. */
function matches(name: string, q: string | null): boolean {
	if (!q) return true;
	const hay = name.toLowerCase().replace(/[._-]/g, ' ');
	return q
		.toLowerCase()
		.split(/[\s._-]+/)
		.filter((w) => w.length >= 3)
		.every((w) => hay.includes(w));
}

const TYPES: Record<string, string> = {
	'.m3u8': 'application/vnd.apple.mpegurl',
	'.ts': 'video/mp2t',
	'.m4s': 'video/iso.segment',
	'.mp4': 'video/mp4'
};

export interface Indexer {
	server: Server;
	base: string;
	close: () => Promise<void>;
}

export async function startIndexer(releases: IndexedRelease[], liveDir: string): Promise<Indexer> {
	let base = '';
	const server = createServer((req, res) => {
		void (async () => {
			const url = new URL(req.url ?? '/', base);
			const send = (status: number, type: string, body: string | Buffer) => {
				res.writeHead(status, { 'Content-Type': type, 'Cache-Control': 'no-store' });
				res.end(body);
			};
			if (url.pathname === '/api') {
				if (url.searchParams.get('t') === 'caps')
					return send(200, 'application/xml', '<caps><searching><search available="yes"/></searching></caps>');
				const q = url.searchParams.get('q');
				return send(
					200,
					'application/rss+xml',
					rss(
						base,
						releases.filter((r) => matches(r.torrent.name, q))
					)
				);
			}
			const dl = /^\/dl\/(.+)\.torrent$/.exec(url.pathname);
			if (dl) {
				const r = releases.find((x) => x.id === dl[1]);
				return r ? send(200, 'application/x-bittorrent', r.torrent.bytes) : send(404, 'text/plain', 'unknown');
			}
			if (url.pathname === '/iptv/countries.json')
				return send(200, 'application/json', JSON.stringify([{ code: 'FR', name: 'France', flag: '' }]));
			if (url.pathname === '/iptv/streams.json' || url.pathname === '/iptv/channels.json') return send(200, 'application/json', '[]');
			if (url.pathname === '/iptv/fr.m3u') {
				return send(
					200,
					'audio/x-mpegurl',
					`#EXTM3U\n#EXTINF:-1 tvg-id="BenchLive.fr" group-title="General",Bench Live\n${base}/live/index.m3u8\n`
				);
			}
			if (url.pathname.startsWith('/live/')) {
				const rel = normalize(url.pathname.slice('/live/'.length));
				if (rel.startsWith('..')) return send(403, 'text/plain', 'no');
				try {
					const body = await readFile(join(liveDir, rel));
					return send(200, TYPES[extname(rel)] ?? 'application/octet-stream', body);
				} catch {
					return send(404, 'text/plain', 'not yet');
				}
			}
			send(404, 'text/plain', 'not found');
		})();
	});
	await new Promise<void>((ok) => server.listen(0, '127.0.0.1', ok));
	base = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
	return {
		server,
		base,
		close: () => new Promise<void>((ok) => server.close(() => ok()))
	};
}
