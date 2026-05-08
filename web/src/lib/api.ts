export type User = {
  id: string;
  email: string;
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

export const admin = {
  listInvitations: () => api.get<Invitation[]>("/admin/invitations"),
  createInvitation: (ttl_secs?: number) =>
    api.post<CreatedInvitation>("/admin/invitations", { ttl_secs }),
  revokeInvitation: (id: string) => api.delete<void>(`/admin/invitations/${id}`),
  storage: () => api.get<StorageStats>("/admin/storage"),
  triggerGc: () => api.post<GcReport>("/admin/gc"),
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
};

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
};

export const search = {
  query: (q: string, opts: SearchOpts = {}) => {
    const qs = new URLSearchParams({ q });
    if (opts.page) qs.set("page", String(opts.page));
    if (opts.limit) qs.set("limit", String(opts.limit));
    if (opts.sort_by) qs.set("sort_by", opts.sort_by);
    if (opts.order) qs.set("order", opts.order);
    return api.get<AggregatedResults>(`/search?${qs}`);
  },
};

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
  added_at: string;
  last_played_at: string | null;
  source_provider: string | null;
  source_external_id: string | null;
};

export type IngestResponse = {
  id: string;
  already_managed: boolean;
  snapshot: TorrentSnapshot;
};

export const torrents = {
  preview: (provider_id: string, external_id: string) =>
    api.post<TorrentPreview>("/torrents/preview", { provider_id, external_id }),
  ingest: (provider_id: string, external_id: string) =>
    api.post<IngestResponse>("/torrents", { provider_id, external_id }),
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
  /** HLS master playlist for the given audio track. */
  hlsUrl: (infohash: string, idx: number, audioIdx: number) =>
    `/api/torrents/${infohash}/files/${idx}/hls/${audioIdx}/master.m3u8`,
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
