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

/** Event fired when a refresh attempt failed and the user must be
 *  treated as logged out. The AuthProvider listens for this and flips
 *  the auth state to `anonymous` so the route guards send the user to
 *  /login instead of leaving stale React Query errors on screen. */
export const AUTH_EXPIRED_EVENT = "iris:auth-expired";

const NO_RETRY_PATHS = new Set([
  "/auth/refresh",
  "/auth/login",
  "/auth/register",
  "/auth/logout",
]);

async function request<T>(method: string, path: string, body?: unknown): Promise<T> {
  const fire = () =>
    fetch(`/api${path}`, {
      method,
      credentials: "include",
      headers: body ? { "Content-Type": "application/json" } : undefined,
      body: body ? JSON.stringify(body) : undefined,
    });
  let res = await fire();
  // Transparent re-auth on expired access cookie. Without this any in-
  // flight query that fires after the access cookie expires (but before
  // the keep-alive timer rotates it) bubbles a 401 up to the component,
  // which then renders the raw error message ("Unauthorized" / "Forbidden")
  // instead of the actual content.
  if (res.status === 401 && !NO_RETRY_PATHS.has(path)) {
    const refreshed = await fetch("/api/auth/refresh", {
      method: "POST",
      credentials: "include",
    });
    if (refreshed.ok) {
      res = await fire();
    } else {
      // Refresh token itself dead — bounce the user to login. Done via
      // a window event so api.ts stays unaware of the auth context's
      // setState; AuthProvider wires the listener.
      window.dispatchEvent(new Event(AUTH_EXPIRED_EVENT));
    }
  }
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
  changeDisplayName: (display_name: string) => api.post<void>("/me/display-name", { display_name }),
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

export type RemuxJobView = {
  /** `<infohash>_<file_idx>` — also the cache filename stem. */
  key: string;
  infohash: string | null;
  file_idx: number | null;
  torrent_name: string | null;
  /** True if an ffmpeg run for this key is currently in flight. */
  in_flight: boolean;
  /** Bytes occupied by the cached `.fmp4` (0 when not yet built). */
  size_bytes: number;
  /** Last-modified time of the cache file (epoch seconds). */
  mtime: number | null;
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
  listRemux: () => api.get<RemuxJobView[]>("/admin/remux"),
  wipeRemux: (key: string) => api.delete<{ freed_bytes: number }>(`/admin/remux/${key}`),
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
  link: (code: string, label?: string) => api.post<void>("/me/devices", { code, label }),
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
  /** Movies only: TMDB `runtime` minutes. TV: typical episode runtime. */
  runtime_minutes: number | null;
  /** TV only — total seasons published. NULL for movies. */
  number_of_seasons: number | null;
};

export type TmdbSuggestion = {
  kind: MediaKind;
  tmdb_id: number;
  title: string;
  year: number | null;
  overview: string | null;
  poster_path: string | null;
};

export const metadata = {
  tmdb: (id: number) => api.get<TmdbMetadata>(`/metadata/tmdb/${id}`),
  /** Typeahead: TMDB multi-search proxied through the backend. Empty
   *  array on missing config / network failure (best-effort). */
  tmdbSearch: (q: string) =>
    api.get<TmdbSuggestion[]>(`/metadata/tmdb/search?q=${encodeURIComponent(q)}`),
};

// ---------------------------------------------------------------------------
// Torrent details (search result preview)
// ---------------------------------------------------------------------------

export type AudioInfo = {
  lang: string | null;
  codec: string | null;
  channels: number | null;
  bitrate_kbps: number | null;
  title: string | null;
  default: boolean;
  commercial_name: string | null;
};

export type SubInfo = {
  lang: string | null;
  format: string | null;
  title: string | null;
  default: boolean;
  forced: boolean;
};

export type VideoInfo = {
  codec: string | null;
  resolution: string | null;
  duration_secs: number | null;
  fps: number | null;
  bitrate_kbps: number | null;
  hdr: string | null;
};

export type MediaInfoSummary = {
  video: VideoInfo | null;
  audio: AudioInfo[];
  subtitles: SubInfo[];
};

export type TorrentDetails = {
  provider_id: string;
  external_id: string;
  title: string;
  description: string | null;
  nfo: string | null;
  media_info: MediaInfoSummary | null;
  tags: string[];
  category: string | null;
  uploader: string | null;
  uploaded_at: string | null;
  age: string | null;
  seeders: number | null;
  leechers: number | null;
  times_completed: number | null;
  views: number | null;
  freeleech: boolean;
  exclusive: boolean;
  file_count: number | null;
  file_size_bytes: number | null;
};

export const searchDetails = {
  get: (provider_id: string, external_id: string) =>
    api.get<TorrentDetails>(
      `/search/details?provider=${encodeURIComponent(provider_id)}&id=${encodeURIComponent(external_id)}`,
    ),
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
  /** Raw source download (range-supported). Browser saves to disk. */
  downloadUrl: (infohash: string, idx: number) => `/api/torrents/${infohash}/files/${idx}/stream`,
  streamUrl: (infohash: string, idx: number) => `/api/torrents/${infohash}/files/${idx}/stream`,
  /**
   * Universal playback URL — returns the HLS-CMAF master playlist.
   * Both web (Vidstack via hls.js) and Android (Media3 HlsMediaSource)
   * consume it the same way; multi-audio renditions are exposed via
   * EXT-X-MEDIA in the manifest. First request to master.m3u8 blocks
   * until ffmpeg has built enough of the cache; later asset fetches
   * hit static files via byte-range.
   */
  playUrl: (infohash: string, idx: number) =>
    `/api/torrents/${infohash}/files/${idx}/play/master.m3u8`,
  /** Polled by the player UI before mounting `<video>`, surfaces the
   *  download / remux progress so we can render a meaningful loader. */
  playStatus: (infohash: string, idx: number) =>
    api.get<PlayStatus>(`/torrents/${infohash}/files/${idx}/play/status`),
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

export type PlayStatus = {
  ready: boolean;
  /** "downloading" | "remuxing" | "preparing" — null when ready. */
  reason: string | null;
  /** 0..1 — only meaningful when reason === "downloading". */
  progress: number | null;
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

// ---------------------------------------------------------------------------
// Discovery: featured carousels (torr9 /featured/{movies,series}, etc.)
// ---------------------------------------------------------------------------

export type FeaturedResponse = {
  movies: SearchResult[];
  series: SearchResult[];
};

export const discover = {
  featured: () => api.get<FeaturedResponse>("/discover/featured"),
};

// ---------------------------------------------------------------------------
// Library — collections (default) or raw torrents (toggle)
// ---------------------------------------------------------------------------

export type CollectionListItem = {
  id: string;
  tmdb_id: number | null;
  display_title: string;
  kind: "tv" | "movie";
  torrent_count: number;
  total_size_bytes: number;
  episode_count: number;
  representative_infohash: string | null;
};

export type LibraryResponse =
  | { view: "collections"; items: CollectionListItem[] }
  | { view: "torrents"; items: TorrentView[] };

export type CollectionEpisodeEntry = {
  season: number;
  episode: number;
  infohash: string;
  file_idx: number;
  watched: boolean;
};

export type CollectionDetail = {
  id: string;
  tmdb_id: number | null;
  display_title: string;
  kind: "tv" | "movie";
  torrents: TorrentView[];
  episodes: CollectionEpisodeEntry[];
};

export const library = {
  list: (view: "collections" | "torrents" = "collections") =>
    api.get<LibraryResponse>(`/library?view=${view}`),
  collection: (id: string) => api.get<CollectionDetail>(`/library/collections/${id}`),
};

// ---------------------------------------------------------------------------
// Series follows (Watchlist + Series detail page)
// ---------------------------------------------------------------------------

export type FollowSummary = {
  tmdb_id: number;
  name: string;
  total_seasons: number | null;
  poster_path: string | null;
  backdrop_path: string | null;
  /** Number of episodes the notify scheduler has surfaced as available
   *  since the user last opened the series page. Drives the "X nouveaux"
   *  badge on Watchlist cards. */
  new_count: number;
  last_visited_at: string | null;
  created_at: string;
};

export type EpisodeStatus = "downloaded" | "available" | "unavailable";

export type EpisodeItem = {
  season: number;
  episode: number;
  name: string | null;
  overview: string | null;
  air_date: string | null;
  still_path: string | null;
  runtime_minutes: number | null;
  status: EpisodeStatus;
  /** Per-user `playback_progress.completed` for the underlying file
   *  (only meaningful when status === "downloaded"). */
  watched: boolean;
  /** Set when status === "downloaded": where to play. */
  infohash: string | null;
  file_idx: number | null;
  /** Set when status === "available": ready for the on-demand grab
   *  endpoint (Phase 4). */
  indexer_provider: string | null;
  indexer_torrent_id: string | null;
};

export type EpisodesResponse = {
  season: number;
  total_seasons: number | null;
  items: EpisodeItem[];
};

export type EpisodePoint = {
  tmdb_id: number;
  season: number;
  episode: number;
  status: EpisodeStatus;
};

export type EpisodeContext = {
  followed: boolean;
  current: EpisodePoint | null;
  next: EpisodePoint | null;
};

export type GrabEpisodeResponse = {
  infohash: string;
  file_idx: number;
  /** True when the episode was already in the library — the call short-
   *  circuited through the idempotent path and didn't trigger a fresh
   *  ingest. */
  already_grabbed: boolean;
};

export const follows = {
  list: () => api.get<FollowSummary[]>("/me/follows"),
  add: (tmdb_id: number, name?: string, total_seasons?: number) =>
    api.post<FollowSummary>("/me/follows", { tmdb_id, name, total_seasons }),
  remove: (tmdb_id: number) => api.delete<void>(`/me/follows/${tmdb_id}`),
  episodes: (tmdb_id: number, season: number = 1) =>
    api.get<EpisodesResponse>(`/me/follows/${tmdb_id}/episodes?season=${season}`),
  grabEpisode: (tmdb_id: number, season: number, episode: number) =>
    api.post<GrabEpisodeResponse>(
      `/me/follows/${tmdb_id}/episodes/${season}/${episode}/grab`,
    ),
  /** Fetch context for the file currently playing — drives the
   *  "Préparer le suivant ?" modal at episode end. Returns nulls when
   *  the file isn't a TV episode or the user doesn't follow the show. */
  episodeContext: (infohash: string, file_idx: number) =>
    api.get<EpisodeContext>(
      `/me/follows/episode-context?infohash=${encodeURIComponent(infohash)}&file_idx=${file_idx}`,
    ),
};
