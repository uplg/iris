// Native (`<track>`) cues lifted above the chrome while it shows, back down when it hides. The
// browser draws them inside the video, so the only lever is each cue's `line` (counted from the
// bottom when negative). A cue the file positions itself keeps its place.

/** Lines from the bottom a cue sits on while the controls show (the bar is about four lines). */
const LIFTED_LINE = -4;

const lifted = new WeakSet<VTTCue>();

function place(cue: TextTrackCue, lift: boolean) {
	if (typeof VTTCue === 'undefined' || !(cue instanceof VTTCue)) return;
	if (lift) {
		if (cue.line !== 'auto' || lifted.has(cue)) return;
		lifted.add(cue);
		cue.line = LIFTED_LINE;
	} else if (lifted.has(cue)) {
		lifted.delete(cue);
		cue.line = 'auto';
	}
}

function placeAll(track: TextTrack, lift: boolean) {
	const cues = track.cues;
	if (!cues) return;
	for (let i = 0; i < cues.length; i += 1) place(cues[i], lift);
}

/** Keeps the showing tracks' cues lifted (or not) as cues load and change; returns the detach,
 * which puts every lifted cue back. */
export function liftCues(video: HTMLVideoElement, lift: boolean): () => void {
	const tracks = video.textTracks;
	const onCue = (e: Event) => {
		const t = e.target as TextTrack;
		if (t.mode === 'showing') placeAll(t, lift);
	};
	const each = (fn: (t: TextTrack) => void) => {
		for (let i = 0; i < tracks.length; i += 1) fn(tracks[i]);
	};
	each((t) => {
		if (t.mode === 'showing') placeAll(t, lift);
		t.addEventListener('cuechange', onCue);
	});
	return () => {
		each((t) => {
			t.removeEventListener('cuechange', onCue);
			placeAll(t, false);
		});
	};
}
