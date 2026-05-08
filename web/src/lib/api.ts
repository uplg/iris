export type User = {
  id: string;
  email: string;
  display_name: string;
  is_admin: boolean;
};

export class ApiError extends Error {
  status: number;
  code: string;
  constructor(status: number, code: string, message: string) {
    super(message);
    this.status = status;
    this.code = code;
  }
}

async function request<T>(method: string, path: string, body?: unknown): Promise<T> {
  const res = await fetch(`/api${path}`, {
    method,
    credentials: "include",
    headers: body ? { "Content-Type": "application/json" } : undefined,
    body: body ? JSON.stringify(body) : undefined,
  });
  if (res.status === 204) return undefined as T;
  const data = res.headers.get("content-type")?.includes("application/json")
    ? await res.json()
    : await res.text();
  if (!res.ok) {
    const err = data as { error?: string; message?: string };
    throw new ApiError(res.status, err?.error ?? "error", err?.message ?? res.statusText);
  }
  return data as T;
}

export const api = {
  get: <T>(p: string) => request<T>("GET", p),
  post: <T>(p: string, body?: unknown) => request<T>("POST", p, body),
  put: <T>(p: string, body?: unknown) => request<T>("PUT", p, body),
  delete: <T>(p: string) => request<T>("DELETE", p),
};

export type Invitation = {
  id: string;
  created_by: string;
  created_at: string;
  expires_at: string;
  consumed_at: string | null;
  consumed_by: string | null;
};

export type CreatedInvitation = {
  id: string;
  token: string;
  expires_at: string;
};

export const auth = {
  me: () => api.get<User>("/me"),
  login: (email: string, password: string) => api.post<User>("/auth/login", { email, password }),
  register: (invite_token: string, email: string, password: string) =>
    api.post<User>("/auth/register", { invite_token, email, password }),
  refresh: () => api.post<User>("/auth/refresh"),
  logout: () => api.post<void>("/auth/logout"),
  changePassword: (old_password: string, new_password: string) =>
    api.post<void>("/me/password", { old_password, new_password }),
  changeDisplayName: (display_name: string) =>
    api.post<void>("/me/display-name", { display_name }),
};

export type StorageStats = {
  used_bytes: number;
  max_storage_bytes: number;
  threshold_bytes: number;
  target_bytes: number;
  threshold_pct: number;
  target_pct: number;
  torrent_count: number;
};

export type GcEvictedEntry = {
  infohash: string;
  name: string;
  freed_bytes: number;
};

export type GcReport = {
  used_bytes_before: number;
  used_bytes_after: number;
  threshold_bytes: number;
  target_bytes: number;
  evicted: GcEvictedEntry[];
};

export type UserView = {
  id: string;
  email: string;
  display_name: string;
  is_admin: boolean;
  created_at: string;
};

export type HlsJobView = {
  key: string;
  infohash: string | null;
  file_idx: number | null;
  torrent_name: string | null;
  running: boolean;
  master_present: boolean;
  video_segments: number;
  done: boolean;
  last_failed_at: number | null;
  has_log: boolean;
  expected_duration_secs: number | null;
  disk_bytes: number;
};

export const admin = {
  listInvitations: () => api.get<Invitation[]>("/admin/invitations"),
  createInvitation: (ttl_secs?: number) =>
    api.post<CreatedInvitation>("/admin/invitations", { ttl_secs }),
  revokeInvitation: (id: string) => api.delete<void>(`/admin/invitations/${id}`),
  storage: () => api.get<StorageStats>("/admin/storage"),
  triggerGc: () => api.post<GcReport>("/admin/gc"),
  listUsers: () => api.get<UserView[]>("/admin/users"),
  resetPassword: (userId: string, new_password: string) =>
    api.post<void>(`/admin/users/${userId}/password`, { new_password }),
  listHls: () => api.get<HlsJobView[]>("/admin/hls"),
  wipeHls: (key: string) => api.delete<{ freed_bytes: number }>(`/admin/hls/${key}`),
};

export type DeviceView = {
  jti: string;
  label: string | null;
  kind: string | null;
  issued_at: string;
  expires_at: string;
};

export const devices = {
  list: () => api.get<DeviceView[]>("/me/devices"),
  link: (code: string, label?: string) =>
    api.post<void>("/me/devices", { code, label }),
  revoke: (jti: string) => api.delete<void>(`/me/devices/${jti}`),
};

export type SearchResult = {
  provider_id: string;
  external_id: string;
  title: string;
  year: number | null;
  size_bytes: number | null;
  seeders: number | null;
  leechers: number | null;
  infohash: string | null;
  magnet: string | null;
  category: string | null;
  tags: string[];
  freeleech: boolean;
  uploader: string | null;
  uploaded_at: string | null;
  tmdb_id: number | null;
  kind: MediaKind | null;
};

export type MediaKind = "movie" | "tv";
export type SortField = "title" | "size" | "seeders" | "leechers" | "uploaded";
export type SortOrder = "asc" | "desc";

export type ProviderResultMeta = {
  id: string;
  current_page: number;
  limit: number;
  total_count: number | null;
  total_pages: number | null;
  error: string | null;
};

export type AggregatedResults = {
  results: SearchResult[];
  providers: ProviderResultMeta[];
};

export type SearchOpts = {
  page?: number;
  limit?: number;
  sort_by?: SortField;
  order?: SortOrder;
  kind?: MediaKind;
};

export const search = {
  query: (q: string, opts: SearchOpts = {}) => {
    const qs = new URLSearchParams({ q });
    if (opts.page) qs.set("page", String(opts.page));
    if (opts.limit) qs.set("limit", String(opts.limit));
    if (opts.sort_by) qs.set("sort_by", opts.sort_by);
    if (opts.order) qs.set("order", opts.order);
    if (opts.kind) qs.set("kind", opts.kind);
    return api.get<AggregatedResults>(`/search?${qs}`);
  },
};

export type TmdbMetadata = {
  kind: "movie" | "tv";
  tmdb_id: number;
  title: string;
  overview: string | null;
  year: number | null;
  poster_path: string | null;
  backdrop_path: string | null;
  vote_score: number | null;
  vote_count: number | null;
  genres: string[];
};

export const metadata = {
  tmdb: (id: number) => api.get<TmdbMetadata>(`/metadata/tmdb/${id}`),
};

/** Build a TMDB image URL. Sizes: w92, w154, w185, w342, w500, original. */
export function tmdbImage(
  path: string | null | undefined,
  size: "w92" | "w154" | "w185" | "w342" | "w500" | "original" = "w185",
): string | null {
  if (!path) return null;
  return `https://image.tmdb.org/t/p/${size}${path}`;
}

export type ProviderInfo = {
  id: string;
  capabilities: {
    returns_magnet: boolean;
    returns_torrent_file: boolean;
    returns_infohash: boolean;
  };
};

export const providers = {
  list: () => api.get<ProviderInfo[]>("/providers"),
};

export type ProgressView = {
  position_seconds: number;
  duration_seconds: number | null;
  audio_track_idx: number | null;
  subtitle_track_idx: number | null;
  completed: boolean;
  last_watched_at: string;
};

export type ContinueWatchingItem = {
  infohash: string;
  torrent_name: string;
  tmdb_id: number | null;
  /** True only if the server matched the TMDB runtime against the file's
   *  probed duration. Until then frontends should NOT fetch TMDB metadata
   *  for this entry — the wrong-poster / wrong-title experience is worse
   *  than no metadata at all. */
  tmdb_verified: boolean;
  file_idx: number;
  file_path: string | null;
  position_seconds: number;
  duration_seconds: number | null;
  last_watched_at: string;
  completed: boolean;
};

export type FileProgressEntry = {
  file_idx: number;
  position_seconds: number;
  duration_seconds: number | null;
  completed: boolean;
  last_watched_at: string;
};

export const progress = {
  get: (infohash: string, idx: number) =>
    api.get<ProgressView | null>(`/torrents/${infohash}/files/${idx}/progress`),
  forTorrent: (infohash: string) => api.get<FileProgressEntry[]>(`/torrents/${infohash}/progress`),
  put: (
    infohash: string,
    idx: number,
    body: {
      position_seconds: number;
      duration_seconds?: number | null;
      audio_track_idx?: number | null;
      subtitle_track_idx?: number | null;
      completed?: boolean;
    },
  ) => api.put<void>(`/torrents/${infohash}/files/${idx}/progress`, body),
};

export const me = {
  continueWatching: () => api.get<ContinueWatchingItem[]>("/me/continue-watching"),
};

export type FilePreview = {
  index: number;
  path: string;
  size_bytes: number;
  extension: string | null;
  is_video: boolean;
};

export type TorrentPreview = {
  infohash: string;
  name: string;
  total_size_bytes: number;
  piece_length: number;
  piece_count: number;
  announce_urls: string[];
  files: FilePreview[];
};

export type FileEntry = {
  index: number;
  path: string;
  size_bytes: number;
};

export type TorrentSnapshot = {
  infohash: string;
  name: string | null;
  total_size_bytes: number;
  state: "initializing" | "live" | "paused" | "error";
  progress_bytes: number;
  progress_pct: number;
  download_speed_bps: number;
  upload_speed_bps: number;
  uploaded_bytes: number;
  peers: number;
  files: FileEntry[];
  error: string | null;
  finished: boolean;
};

export type TorrentView = TorrentSnapshot & {
  id: string;
  added_by: string;
  /** Public display name of the user that added this torrent. */
  added_by_name: string;
  added_at: string;
  last_played_at: string | null;
  source_provider: string | null;
  source_external_id: string | null;
  tmdb_id: number | null;
  /** Server-validated TMDB association (runtime matches probed duration ±15 %). */
  tmdb_verified: boolean;
};

export type IngestResponse = {
  id: string;
  already_managed: boolean;
  snapshot: TorrentSnapshot;
};

export const torrents = {
  preview: (provider_id: string, external_id: string) =>
    api.post<TorrentPreview>("/torrents/preview", { provider_id, external_id }),
  ingest: (provider_id: string, external_id: string, tmdb_id?: number | null) =>
    api.post<IngestResponse>("/torrents", { provider_id, external_id, tmdb_id }),
  list: () => api.get<TorrentView[]>("/torrents"),
  get: (infohash: string) => api.get<TorrentView>(`/torrents/${infohash}`),
  remove: (infohash: string) => api.delete<void>(`/torrents/${infohash}`),
  /** Browser will save the file to disk (filename suggested by content-disposition or attribute). */
  downloadUrl: (infohash: string, idx: number) => `/api/torrents/${infohash}/files/${idx}/stream`,
  streamUrl: (infohash: string, idx: number) => `/api/torrents/${infohash}/files/${idx}/stream`,
  /**
   * Browser-friendly playback URL: native byte-stream for MP4/WebM, on-the-fly
   * MKV→fMP4 remux otherwise (same codecs, no re-encode). No seek support on
   * remuxed stream — prefer `hlsUrl` for full UX.
   */
  playUrl: (infohash: string, idx: number) => `/api/torrents/${infohash}/files/${idx}/play`,
  /**
   * HLS master playlist. The master references one video variant +
   * N audio renditions in a single `EXT-X-MEDIA:TYPE=AUDIO` group, so
   * switching audio is a player-side toggle (no URL change, no re-download
   * of video segments).
   */
  hlsUrl: (infohash: string, idx: number) =>
    `/api/torrents/${infohash}/files/${idx}/hls/master.m3u8`,
  /** Non-blocking poll endpoint — kicks the ffmpeg job and reports progress. */
  hlsStatus: (infohash: string, idx: number) =>
    api.get<HlsStatus>(`/torrents/${infohash}/files/${idx}/hls/status`),
  probe: (infohash: string, idx: number) =>
    api.get<MediaProbe>(`/torrents/${infohash}/files/${idx}/probe`),
  subtitleUrl: (infohash: string, idx: number, streamIdx: number) =>
    `/api/torrents/${infohash}/files/${idx}/sub/${streamIdx}/track.vtt`,
};

export type MediaProbe = {
  container: string;
  duration_seconds: number | null;
  size_bytes: number | null;
  bit_rate: number | null;
  video: VideoStream[];
  audio: AudioStream[];
  subtitle: SubtitleStream[];
};

export type VideoStream = {
  index: number;
  absolute_index: number;
  codec: string;
  profile: string | null;
  width: number | null;
  height: number | null;
  bit_rate: number | null;
  frame_rate: number | null;
};

export type AudioStream = {
  index: number;
  absolute_index: number;
  codec: string;
  channels: number;
  channel_layout: string | null;
  sample_rate: number | null;
  language: string | null;
  title: string | null;
  default: boolean;
  forced: boolean;
  browser_compatible: boolean;
};

export type HlsStatus = {
  ffmpeg_running: boolean;
  segments_produced: number;
  estimated_total_segments: number | null;
  endlist_present: boolean;
  error: string | null;
};

export type SubtitleStream = {
  index: number;
  absolute_index: number;
  codec: string;
  language: string | null;
  title: string | null;
  default: boolean;
  forced: boolean;
  text_based: boolean;
};
