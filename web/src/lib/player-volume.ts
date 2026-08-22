// Volume is device-specific, so it's persisted locally (not per-user on the
// server like the audio/subtitle language preference). Shared by every
// IrisPlayer mount point (VOD WatchPage, Live TV) so the level survives
// across pages, episodes and sessions on this device.
import { readLocal, writeLocal } from "./safe-storage";

const VOLUME_KEY = "iris:volume";

export function readStoredVolume(): number | undefined {
  const raw = readLocal(VOLUME_KEY);
  if (raw == null) return undefined;
  const v = Number(raw);
  return Number.isFinite(v) ? Math.max(0, Math.min(1, v)) : undefined;
}

export function writeStoredVolume(v: number): void {
  writeLocal(VOLUME_KEY, String(Math.max(0, Math.min(1, v))));
}
