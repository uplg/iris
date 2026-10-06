// Decode pacing for the canvas engine (Tier C/D): decode only a short lead ahead of the
// playback clock, and call the end when the clock (not the decoder) gets there.

/** Video decoded ahead of the clock. Decoded frames are the expensive buffer (several MB
 *  each, and the decoder's own pool), encoded packets the cheap one. */
export const VIDEO_LEAD_S = 0.5;
/** Frames queued for the renderer before the decoder waits, under the renderer's cap. */
export const MAX_QUEUED_FRAMES = 24;
/** Audio decoded ahead of the clock: well inside the scheduler's 4 s ring. */
export const AUDIO_LEAD_S = 1;

/** Whether a packet at `ts` may be decoded with the clock at `clock` (all seconds). */
export function withinLead(ts: number, clock: number, lead: number): boolean {
	return ts - clock <= lead;
}

export type EndState = {
	videoDone: boolean;
	/** Latest video frame timestamp seen (seconds). */
	lastVideoTs: number | null;
	hasAudio: boolean;
	audioDone: boolean;
	/** End of the latest audio buffer seen (seconds). */
	audioEndTs: number | null;
};

/** Whether playback reached the end: every track decoded to its end and the clock got there.
 *  With audio the clock is the audio's own (it stops where the audio stops), so audio's end is
 *  the end; without it, the last frame's. */
export function playbackEnded(s: EndState, clock: number): boolean {
	if (!s.videoDone || (s.hasAudio && !s.audioDone)) return false;
	const end = s.hasAudio ? s.audioEndTs : s.lastVideoTs;
	return end === null || clock >= end - 0.1;
}
