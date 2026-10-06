// A v1 single-file .torrent for a fixture: the bencoded metainfo and its infohash. No announce
// URL: the backend finds the payload already in its download dir and only rechecks it.
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { basename } from 'node:path';

type Bencodable = number | string | Uint8Array | Bencodable[] | { [key: string]: Bencodable };

function bencode(v: Bencodable): Buffer {
	if (typeof v === 'number') return Buffer.from(`i${Math.trunc(v)}e`);
	if (typeof v === 'string') return bencode(Buffer.from(v, 'utf8'));
	if (v instanceof Uint8Array) return Buffer.concat([Buffer.from(`${v.length}:`), v]);
	if (Array.isArray(v)) return Buffer.concat([Buffer.from('l'), ...v.map(bencode), Buffer.from('e')]);
	// keys sorted as raw byte strings (BEP 3)
	const keys = Object.keys(v).toSorted((a, b) => Buffer.compare(Buffer.from(a), Buffer.from(b)));
	return Buffer.concat([Buffer.from('d'), ...keys.flatMap((k) => [bencode(k), bencode(v[k])]), Buffer.from('e')]);
}

const PIECE = 256 * 1024;

export interface BuiltTorrent {
	name: string;
	infohash: string;
	bytes: Buffer;
	size: number;
}

export function buildTorrent(path: string): BuiltTorrent {
	const data = readFileSync(path);
	const pieces: Buffer[] = [];
	for (let off = 0; off < data.length; off += PIECE)
		pieces.push(
			createHash('sha1')
				.update(data.subarray(off, off + PIECE))
				.digest()
		);
	const name = basename(path);
	const info = { length: data.length, name, 'piece length': PIECE, pieces: Buffer.concat(pieces) };
	const encodedInfo = bencode(info);
	const infohash = createHash('sha1').update(encodedInfo).digest('hex');
	const bytes = Buffer.concat([Buffer.from('d4:info'), encodedInfo, Buffer.from('e')]);
	return { name, infohash, bytes, size: data.length };
}
