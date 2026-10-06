// Brings the bench up once for the whole run: the fixtures, a fresh web build, the backend on a
// throwaway data dir with the fixtures grabbed through a local Torznab indexer (their payload is
// already in the download dir, so librqbit's recheck finishes them with no peer), and a looping
// live HLS channel. Returns the teardown. Set IRIS_E2E_KEEP=1 to keep the temp dir and its log;
// `bun e2e/harness/serve.ts` keeps a bench up, which runs with IRIS_E2E_REUSE=1 then use.
import { spawn, spawnSync, type ChildProcess } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, openSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:net';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { BENCH_PORT, CATALOG, type BenchState, type ClipKey } from './catalog.ts';
import { startIndexer, type IndexedRelease } from './indexer.ts';
import { buildTorrent } from './torrent.ts';

const here = dirname(fileURLToPath(import.meta.url));
const e2e = resolve(here, '..');
const web = resolve(e2e, '..');
const root = resolve(web, '..');
const media = join(e2e, '.media');
export const STATE_FILE = join(e2e, '.state.json');

const EMAIL = 'bench@iris.local';
const PASSWORD = 'bench-password-1234';

function run(cmd: string, args: string[], cwd: string) {
	const r = spawnSync(cmd, args, { cwd, stdio: 'inherit', env: process.env });
	if (r.status !== 0) throw new Error(`${cmd} ${args.join(' ')} failed (${r.status})`);
}

/** Fails fast when something already holds the bench's port (a previous run left behind). */
async function assertFree(port: number): Promise<void> {
	return new Promise((ok, ko) => {
		const s = createServer();
		s.once('error', () => ko(new Error(`port ${port} is taken: set IRIS_E2E_PORT or stop what holds it`)));
		s.listen(port, '127.0.0.1', () => s.close(() => ok()));
	});
}

async function until<T>(what: string, timeoutMs: number, probe: () => Promise<T | undefined>): Promise<T> {
	const end = Date.now() + timeoutMs;
	let last: unknown;
	while (Date.now() < end) {
		try {
			const v = await probe();
			if (v !== undefined) return v;
		} catch (e) {
			last = e;
		}
		await new Promise((r) => setTimeout(r, 500));
	}
	throw new Error(`timed out waiting for ${what}${last ? `: ${String(last)}` : ''}`);
}

class Api {
	private cookies = new Map<string, string>();
	constructor(readonly base: string) {}

	async call(method: string, path: string, body?: unknown): Promise<Response> {
		const res = await fetch(this.base + path, {
			method,
			headers: {
				'Content-Type': 'application/json',
				Cookie: [...this.cookies].map(([k, v]) => `${k}=${v}`).join('; ')
			},
			body: body === undefined ? undefined : JSON.stringify(body)
		});
		for (const c of res.headers.getSetCookie()) {
			const [pair] = c.split(';');
			const i = pair.indexOf('=');
			this.cookies.set(pair.slice(0, i), pair.slice(i + 1));
		}
		return res;
	}

	async json<T>(method: string, path: string, body?: unknown): Promise<T> {
		const res = await this.call(method, path, body);
		if (!res.ok) throw new Error(`${method} ${path}: ${res.status} ${await res.text()}`);
		return (await res.json()) as T;
	}
}

function configToml(o: { port: number; dataDir: string; downloads: string; providers: string; webDist: string; iptv: string }): string {
	return `providers_file = "${o.providers}"

[server]
bind = "127.0.0.1:${o.port}"
public_url = "http://127.0.0.1:${o.port}"
web_dist = "${o.webDist}"

[storage]
data_dir = "${o.dataDir}"
download_dir = "${o.downloads}"
max_storage_gb = 50
torrent_port = ${o.port + 1}

[auth]
jwt_secret = "bench-only-secret-bench-only-secret-bench-only-secret-0123456789"
access_ttl_secs = 3600
refresh_ttl_secs = 604800
invitation_ttl_secs = 604800

[auth.bootstrap_admin]
email = "${EMAIL}"
password = "${PASSWORD}"

[live_tv]
default_country = "fr"
playlist_url_template = "${o.iptv}/{code}.m3u"
countries_url = "${o.iptv}/countries.json"
streams_url = "${o.iptv}/streams.json"
channels_url = "${o.iptv}/channels.json"
extra_playlists = {}
epg_urls = {}
vavoo_enabled = false

[live_tv.tuner]
enabled = false
`;
}

/** A bench left up by `serve.ts` (IRIS_E2E_REUSE=1): the run uses it as is. */
async function reusable(): Promise<boolean> {
	if (process.env.IRIS_E2E_REUSE !== '1' || !existsSync(STATE_FILE)) return false;
	try {
		const { baseUrl } = JSON.parse(readFileSync(STATE_FILE, 'utf8')) as BenchState;
		return (await fetch(`${baseUrl}/api/health`)).ok;
	} catch {
		return false;
	}
}

export default async function globalSetup(): Promise<() => Promise<void>> {
	if (await reusable()) return async () => undefined;
	const children: ChildProcess[] = [];
	const stopAll = () => {
		for (const c of children) if (c.exitCode === null) c.kill('SIGTERM');
	};
	process.once('exit', stopAll);

	run('bash', [join(e2e, 'fixtures/make.sh')], web);
	// the AC-3 / E-AC-3 libav.js build comes out of the Dockerfile's libav-builder stage, never
	// from npm: point IRIS_E2E_LIBAV_DIR at its three files to run the E-AC-3 cases
	const libavDir = process.env.IRIS_E2E_LIBAV_DIR;
	if (libavDir) {
		for (const ext of ['js', 'mjs', 'wasm']) {
			const name = `libav-6.10.9.0-iris.wasm.${ext}`;
			copyFileSync(join(libavDir, name), join(web, 'static/libavjs', name));
		}
	}
	if (!(process.env.IRIS_E2E_SKIP_WEB_BUILD === '1' && existsSync(join(web, 'build/index.html')))) run('bun', ['run', 'build'], web);
	run('cargo', ['build', '-q', '-p', 'iris-api', '--bin', 'iris'], root);
	const bin = join(process.env.CARGO_TARGET_DIR ?? join(root, 'target'), 'debug/iris');

	const tmp = mkdtempSync(join(tmpdir(), 'iris-e2e-'));
	const dataDir = join(tmp, 'data');
	const downloads = join(tmp, 'downloads');
	const liveDir = join(tmp, 'live');
	for (const d of [dataDir, downloads, liveDir]) mkdirSync(d, { recursive: true });

	const releases: IndexedRelease[] = [];
	const infohash = {} as Record<ClipKey, string>;
	for (const [key, clip] of Object.entries(CATALOG) as [ClipKey, (typeof CATALOG)[ClipKey]][]) {
		const src = join(media, clip.file);
		copyFileSync(src, join(downloads, clip.file));
		const torrent = buildTorrent(src);
		releases.push({ id: `bench-${key}`, torrent, tv: /S\d\dE\d\d/.test(clip.file) });
		infohash[key] = torrent.infohash;
	}

	const indexer = await startIndexer(releases, liveDir);

	// the live channel: the loop re-encoded forever (continuous timestamps), a sliding window
	// with program date-times like a broadcaster's
	const ffmpegLog = openSync(join(tmp, 'ffmpeg-live.log'), 'w');
	children.push(
		spawn(
			'ffmpeg',
			[
				'-hide_banner',
				'-loglevel',
				'warning',
				'-re',
				'-stream_loop',
				'-1',
				'-i',
				join(media, 'live-loop.ts'),
				'-c:v',
				'libx264',
				'-preset',
				'ultrafast',
				'-g',
				'48',
				'-keyint_min',
				'48',
				'-sc_threshold',
				'0',
				'-c:a',
				'aac',
				'-b:a',
				'128k',
				'-f',
				'hls',
				'-hls_time',
				'2',
				'-hls_list_size',
				'8',
				'-hls_flags',
				'delete_segments+program_date_time+independent_segments',
				join(liveDir, 'index.m3u8')
			],
			{ stdio: ['ignore', ffmpegLog, ffmpegLog] }
		)
	);

	const port = BENCH_PORT;
	await assertFree(port);
	const providers = join(tmp, 'providers.toml');
	writeFileSync(
		providers,
		`[[providers]]\nid = "bench"\nkind = "torznab"\nenabled = true\nbase_url = "${indexer.base}"\napi_key = "bench"\ndefault_language = "english"\n`
	);
	const config = join(tmp, 'config.toml');
	writeFileSync(config, configToml({ port, dataDir, downloads, providers, webDist: join(web, 'build'), iptv: `${indexer.base}/iptv` }));

	const log = openSync(join(tmp, 'iris.log'), 'w');
	const backend = spawn(bin, ['--config', config], {
		cwd: root,
		stdio: ['ignore', log, log],
		env: { ...process.env, RUST_LOG: process.env.RUST_LOG ?? 'info,iris_api=debug' }
	});
	children.push(backend);
	const base = `http://127.0.0.1:${port}`;
	await until('the backend', 120_000, async () => {
		if (backend.exitCode !== null) throw new Error(`backend exited (${backend.exitCode}), see ${tmp}/iris.log`);
		return (await fetch(`${base}/api/health`)).ok ? true : undefined;
	});

	const api = new Api(base);
	await api.json('POST', '/api/auth/login', { email: EMAIL, password: PASSWORD });
	for (const r of releases) {
		const q = encodeURIComponent(r.torrent.name.replace(/\.(mp4|mkv)$/, ''));
		await until(`search hit ${r.id}`, 30_000, async () => {
			const s = await api.json<{ results: { provider_id: string; external_id: string }[] }>('GET', `/api/search?q=${q}`);
			return s.results.some((x) => x.external_id === r.id) ? true : undefined;
		});
		await api.json('POST', '/api/torrents', { provider_id: 'bench', external_id: r.id, allow_duplicate: true });
	}
	for (const r of releases) {
		await until(`${r.id} finished`, 60_000, async () => {
			const t = await api.json<{ finished: boolean }>('GET', `/api/torrents/${r.torrent.infohash}`);
			return t.finished ? true : undefined;
		});
	}

	let livePath: string | null = null;
	try {
		await until('the live playlist', 30_000, async () => (existsSync(join(liveDir, 'index.m3u8')) ? true : undefined));
		const ch = await api.json<{ channels: { id: string; name: string }[] }>('GET', '/api/livetv/fr/channels');
		const live = ch.channels.find((c) => c.name === 'Bench Live');
		if (live) livePath = `/live/fr/${live.id}`;
	} catch (e) {
		console.warn(`[e2e] live channel unavailable: ${String(e)}`);
	}

	const libavIris = existsSync(join(web, 'build/libavjs/libav-6.10.9.0-iris.wasm.mjs'));
	const state: BenchState = { baseUrl: base, email: EMAIL, password: PASSWORD, infohash, livePath, libavIris, dataDir: tmp };
	writeFileSync(STATE_FILE, JSON.stringify(state, null, 2));
	process.env.IRIS_E2E_BASE_URL = base;
	console.log(`[e2e] backend up at ${base} (data in ${tmp})`);

	return async () => {
		stopAll();
		await indexer.close();
		if (process.env.IRIS_E2E_KEEP !== '1') rmSync(tmp, { recursive: true, force: true });
		rmSync(STATE_FILE, { force: true });
	};
}
