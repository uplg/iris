// « Getting ready »: what stands between the click and the picture, step by step and in words,
// from the torrent snapshot, the probe, the saved position and (Tier F) the server's remux.

import type { PlayStatus, TorrentView } from '@iris/api/client';
import { percent, plural, speed } from '@iris/api/format';
import { notOnDisk } from './tier.ts';
import { isFetching } from '#lib/torrent.ts';

export type StepState = 'done' | 'current' | 'waiting';
export interface ReadyStep {
	id: 'peers' | 'head' | 'read' | 'resume' | 'server' | 'start';
	label: string;
	state: StepState;
	detail?: string;
	/** 0–100 when the step can say how far it is. */
	pct?: number;
}
export interface ReadyProblem {
	title: string;
	detail: string;
	/** Nobody shares the release: offer to remove it and pick another. */
	deadSwarm: boolean;
}
export interface Readiness {
	steps: ReadyStep[];
	problem: ReadyProblem | null;
}

export interface ReadyInput {
	torrent: TorrentView;
	/** Tier F waits for the server's remux; the others start once the file is read. */
	serverPrep: boolean;
	hasProbe: boolean;
	hasManifest: boolean;
	probeFetching: boolean;
	probeError: unknown;
	progressPending: boolean;
	playStatus: PlayStatus | null;
	playError: unknown;
}

const clamp = (n: number, hi = 100) => Math.min(hi, Math.max(0, n));
const peers = (n: number) => plural(n, 'peer');

/**
 * Nobody is sharing it. The backend says so (`/probe` spends 30 s trying, then answers « no
 * seeders » or « stalled: »), and the snapshot catches the swarm that dies once the head is on
 * disk: no peers, no throughput, not finished, more than two minutes after the add. Both
 * timestamps are the server's, so the test is pure and immune to the client's clock.
 */
export function isDeadSwarm(t: TorrentView, probeError: unknown): boolean {
	if (probeError instanceof Error && /no seeders|^stalled:/i.test(probeError.message)) return true;
	const ageMinutes = (new Date(t.fetched_at).getTime() - new Date(t.added_at).getTime()) / 60_000;
	return t.state !== 'initializing' && isFetching(t) && t.peers === 0 && t.download_speed_bps === 0 && ageMinutes > 2;
}

export function readiness(i: ReadyInput): Readiness {
	const t = i.torrent;
	const downloaded = clamp(t.progress_pct);
	const flow = `${speed(t.download_speed_bps)} from ${peers(t.peers)}`;

	let problem: ReadyProblem | null = null;
	if (isDeadSwarm(t, i.probeError)) {
		problem = {
			title: 'Nobody is sharing this release',
			detail:
				`The tracker advertised seeders, but none of them answered. Iris has ${percent(downloaded)} of the file and no way to get the rest. ` +
				'Removing it frees the partial download and clears the mapping the library keeps pointing at it.',
			deadSwarm: true
		};
	} else if (t.state === 'error') {
		problem = {
			title: 'The torrent stopped with an error',
			detail: t.error ?? 'The engine reported a fault. Try removing it and adding it again.',
			deadSwarm: false
		};
	} else if (i.probeError instanceof Error && !notOnDisk(i.probeError) && !i.probeFetching) {
		problem = { title: 'Iris could not read this file', detail: i.probeError.message, deadSwarm: false };
	} else if (i.serverPrep && i.playStatus?.error) {
		problem = { title: 'The server could not prepare this file', detail: i.playStatus.error, deadSwarm: false };
	} else if (i.serverPrep && i.playError instanceof Error) {
		problem = { title: 'The server could not prepare this file', detail: i.playError.message, deadSwarm: false };
	}

	const onDisk = !notOnDisk(i.probeError);
	const steps: Omit<ReadyStep, 'state'>[] = [];
	const met: boolean[] = [];
	const add = (s: Omit<ReadyStep, 'state'>, done: boolean) => {
		steps.push(s);
		met.push(done);
	};

	add({ id: 'peers', label: 'Connecting to peers', detail: 'Finding the swarm and the map of the file.' }, t.state !== 'initializing');
	add(
		{
			id: 'head',
			label: onDisk ? 'Downloading the first minutes' : `Downloading the first minutes, ${percent(downloaded)}`,
			detail: flow,
			pct: onDisk ? undefined : downloaded
		},
		onDisk
	);
	add(
		{ id: 'read', label: 'Reading the file', detail: 'Its video, audio and subtitles.' },
		i.serverPrep ? i.hasProbe : i.hasManifest || i.hasProbe
	);
	add({ id: 'resume', label: 'Finding where you stopped' }, !i.progressPending);
	if (i.serverPrep) {
		const s = i.playStatus;
		const fraction = typeof s?.progress === 'number' ? clamp(s.progress * 100, 99) : null;
		let label = 'Preparing the stream on the server';
		let detail: string | undefined = 'Starting.';
		let pct: number | undefined;
		if (s?.reason === 'downloading') {
			pct = fraction ?? downloaded;
			label = `Downloading on the server, ${percent(pct)}`;
			detail = flow;
		} else if (s?.reason === 'remuxing') {
			pct = fraction ?? undefined;
			label = pct !== undefined ? `Remuxing on the server, ${percent(pct)}` : 'Remuxing on the server';
			detail = 'The video is copied as it is; the audio is converted where the browser needs it.';
		} else if (s) {
			detail = 'Almost there.';
		}
		add({ id: 'server', label, detail, pct }, s?.ready === true);
	}
	add({ id: 'start', label: 'Starting playback' }, false);

	const current = met.findIndex((m) => !m);
	return {
		problem,
		steps: steps.map((s, k): ReadyStep =>
			Object.assign(s, { state: (met[k] ? 'done' : k === current ? 'current' : 'waiting') as StepState })
		)
	};
}

/** The step in words for a screen reader and the stage's status line. */
export function stepStateText(s: StepState): string {
	return s === 'done' ? 'Done' : s === 'current' ? 'In progress' : 'Waiting';
}
