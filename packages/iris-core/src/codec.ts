// Codec names as manifests (ffprobe), codec strings and release names spell them.

const HEVC = /hevc|hev1|hvc1|h265|x265/i;

/** True for HEVC under any of its spellings: `hevc`, `hev1.*`, `hvc1.*`, `h265`, `x265`. */
export function isHevc(codec: string | null | undefined): boolean {
	return !!codec && HEVC.test(codec);
}
