import type { components } from "./api-types";

/** Wire types generated from the OpenAPI spec (`bun run gen-api`) — the Rust
 *  route handlers + serde types are the source of truth (see
 *  crates/iris-api/src/openapi.rs). Annotated groups re-export their schema
 *  under the existing names so call sites don't change; the rest stays
 *  hand-written until its endpoints are annotated in later waves. */
export type User = components["schemas"]["UserResponse"];

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

/** Event fired when any backend request answers HTTP 426 — the
 *  cached bundle is below the server's `MIN_WEB_VERSION`. App.tsx
 *  listens for it and renders a full-screen lock-out with a "Reload"
 *  action so the user pulls the freshly-deployed bundle. */
export const CLIENT_OUTDATED_EVENT = "iris:client-outdated";

/** Bundle version baked at build time via Vite `define` — see
 *  `vite.config.ts`. Used in the `X-Iris-Client` header. */
export const IRIS_WEB_VERSION: string = __IRIS_WEB_VERSION__;

const NO_RETRY_PATHS = new Set(["/auth/refresh", "/auth/login", "/auth/register", "/auth/logout"]);

function clientHeaders(extra?: HeadersInit): HeadersInit {
  // X-Iris-Client lands on every outbound API request so the server
  // can log usage and (optionally) gate via `MIN_WEB_VERSION`. Cheap:
  // ~30 bytes per request.
  const base: Record<string, string> = {
    "X-Iris-Client": `web/${IRIS_WEB_VERSION}`,
  };
  if (extra) {
    return { ...base, ...(extra as Record<string, string>) };
  }
  return base;
}

/** Outcome of a session refresh. `ok` carries the refreshed user; otherwise
 *  `status` separates a genuine auth death (401/403 → the refresh token is
 *  gone, the user must log in again) from a transient failure (429 rate-limit,
 *  5xx, or 0 = network error → the session is still valid, keep it). */
type RefreshOutcome = { ok: true; user: User } | { ok: false; status: number };

/** Single-flight guard around POST /auth/refresh. A watch page fires several
 *  polls at once, so the moment the access cookie expires they all 401 in the
 *  same tick. Without this, each independently POSTs /auth/refresh and races
 *  the server's refresh-token rotation: the first rotates the token, the
 *  stragglers present the now-revoked one, get 401, and each logs the user out
 *  even though the session is alive. Funnelling every refresh through one
 *  in-flight promise collapses the stampede into a single rotation. */
let inFlightRefresh: Promise<RefreshOutcome> | null = null;

function refreshSession(): Promise<RefreshOutcome> {
  if (inFlightRefresh) return inFlightRefresh;
  const run = (async (): Promise<RefreshOutcome> => {
    try {
      const res = await fetch("/api/auth/refresh", {
        method: "POST",
        credentials: "include",
        headers: clientHeaders(),
      });
      if (res.ok) return { ok: true, user: (await res.json()) as User };
      return { ok: false, status: res.status };
    } catch {
      return { ok: false, status: 0 }; // network error — not an auth failure
    }
  })();
  inFlightRefresh = run;
  // Release the singleton once settled so the next expiry refreshes anew;
  // current awaiters already hold `run`.
  void run.finally(() => {
    if (inFlightRefresh === run) inFlightRefresh = null;
  });
  return run;
}

async function request<T>(method: string, path: string, body?: unknown): Promise<T> {
  const fire = () =>
    fetch(`/api${path}`, {
      method,
      credentials: "include",
      headers: clientHeaders(body ? { "Content-Type": "application/json" } : undefined),
      body: body ? JSON.stringify(body) : undefined,
    });
  let res = await fire();
  // Transparent re-auth on expired access cookie. Without this any in-
  // flight query that fires after the access cookie expires (but before
  // the keep-alive timer rotates it) bubbles a 401 up to the component,
  // which then renders the raw error message ("Unauthorized" / "Forbidden")
  // instead of the actual content.
  if (res.status === 401 && !NO_RETRY_PATHS.has(path)) {
    const outcome = await refreshSession();
    if (outcome.ok) {
      res = await fire(); // retry once with the rotated cookie
    } else if (outcome.status === 401 || outcome.status === 403) {
      // The refresh token itself is dead — genuinely logged out. Bounce to
      // login via a window event so api.ts stays unaware of the auth context's
      // setState; AuthProvider wires the listener.
      window.dispatchEvent(new Event(AUTH_EXPIRED_EVENT));
    }
    // 429 / 5xx / network (status 0): the session is still valid, so do NOT
    // log out. The original 401 surfaces to the caller as a transient error;
    // the next poll or the keep-alive recovers once the backend is reachable.
  }
  // 426 = the server has decided this cached bundle is below
  // `MIN_WEB_VERSION`. Surface globally so App.tsx can lock the UI
  // and prompt the user to reload; checked after the auth retry so
  // a refresh-then-426 still surfaces.
  if (res.status === 426) {
    window.dispatchEvent(new Event(CLIENT_OUTDATED_EVENT));
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

export type Invitation = components["schemas"]["InvitationView"];

export type CreatedInvitation = components["schemas"]["CreatedInvitation"];

export const auth = {
  me: () => api.get<User>("/me"),
  login: (email: string, password: string) => api.post<User>("/auth/login", { email, password }),
  register: (invite_token: string, email: string, password: string) =>
    api.post<User>("/auth/register", { invite_token, email, password }),
  // Shares the single-flight guard with the reactive 401 retry path, and
  // throws a typed ApiError on failure so callers can tell a genuine auth
  // death (401/403) from a transient one (429/5xx/network).
  refresh: async (): Promise<User> => {
    const outcome = await refreshSession();
    if (outcome.ok) return outcome.user;
    throw new ApiError(outcome.status, "refresh_failed", "session refresh failed");
  },
  logout: () => api.post<void>("/auth/logout"),
  changePassword: (old_password: string, new_password: string) =>
    api.post<void>("/me/password", { old_password, new_password }),
  changeDisplayName: (display_name: string) => api.post<void>("/me/display-name", { display_name }),
};

export type StorageStats = components["schemas"]["StorageStats"];

export type GcEvictedEntry = components["schemas"]["EvictedEntry"];

export type GcReport = components["schemas"]["GcReport"];

export type UserView = components["schemas"]["UserView"];

export type RemuxJobView = components["schemas"]["RemuxJobView"];

/** A live "who's watching what" entry from `/admin/active-sessions`. */
export type ActiveSession = components["schemas"]["ActiveSessionView"];

/** A recent playback row from `/admin/watch-history` (all users). */
export type WatchHistoryEntry = components["schemas"]["WatchHistoryView"];

/** One persisted audit-log row — a sensitive action (deletion, password
 *  reset, admin-triggered GC) and who performed it. */
export type AuditLogEntry = components["schemas"]["AuditLogView"];

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
  setDisplayName: (userId: string, display_name: string) =>
    api.post<void>(`/admin/users/${userId}/display-name`, { display_name }),
  listRemux: () => api.get<RemuxJobView[]>("/admin/remux"),
  wipeRemux: (key: string) => api.delete<{ freed_bytes: number }>(`/admin/remux/${key}`),
  activeSessions: () => api.get<ActiveSession[]>("/admin/active-sessions"),
  watchHistory: (limit?: number) =>
    api.get<WatchHistoryEntry[]>(`/admin/watch-history${limit ? `?limit=${limit}` : ""}`),
  /** Full watch history for one user — admin drill-down equivalent of
   *  `me.history()`. */
  userHistory: (userId: string, limit?: number, offset?: number) =>
    api.get<UserHistoryItem[]>(
      `/admin/users/${userId}/history?${new URLSearchParams({
        ...(limit ? { limit: String(limit) } : {}),
        ...(offset ? { offset: String(offset) } : {}),
      }).toString()}`,
    ),
  /** Persisted "who changed/deleted what" log — deletions, password resets,
   *  admin-triggered GC. */
  auditLog: (limit?: number, offset?: number) =>
    api.get<AuditLogEntry[]>(
      `/admin/audit-log?${new URLSearchParams({
        ...(limit ? { limit: String(limit) } : {}),
        ...(offset ? { offset: String(offset) } : {}),
      }).toString()}`,
    ),
};

export type DeviceView = components["schemas"]["DeviceView"];

export const devices = {
  list: () => api.get<DeviceView[]>("/me/devices"),
  link: (code: string, label?: string) => api.post<void>("/me/devices", { code, label }),
  revoke: (jti: string) => api.delete<void>(`/me/devices/${jti}`),
};

export type SearchResult = components["schemas"]["SearchResult"];
export type MediaKind = components["schemas"]["MediaKind"];
export type SortField = components["schemas"]["SortField"];
export type SortOrder = components["schemas"]["SortOrder"];
export type ProviderResultMeta = components["schemas"]["ProviderResultMeta"];
export type ParsedQueryInfo = components["schemas"]["ParsedQueryInfo"];
export type LibraryMatch = components["schemas"]["LibraryMatch"];
/** The `/api/search` response = `AggregatedResults` (flattened) + library
 *  matches. Kept under the name `AggregatedResults` for call-site stability;
 *  the Rust source type is `SearchResponse`. */
export type AggregatedResults = components["schemas"]["SearchResponse"];

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

export type TmdbMetadata = components["schemas"]["MediaMetadata"];

export type TmdbSuggestion = components["schemas"]["TmdbSuggestion"];

export const metadata = {
  /** TMDB id lookup. `kind` disambiguates the movie/tv namespaces (the
   *  same numerical id can refer to two unrelated entries — pass the
   *  collection / search-result kind to land on the right one). */
  tmdb: (id: number, kind?: "movie" | "tv") =>
    api.get<TmdbMetadata>(kind ? `/metadata/tmdb/${id}?kind=${kind}` : `/metadata/tmdb/${id}`),
  /** Typeahead: TMDB multi-search proxied through the backend. Empty
   *  array on missing config / network failure (best-effort). */
  tmdbSearch: (q: string) =>
    api.get<TmdbSuggestion[]>(`/metadata/tmdb/search?q=${encodeURIComponent(q)}`),
  /** Resolve a raw release title to its single best TMDB match. Scored
   *  server-side by kind + year (not popularity) and served from the
   *  persistent 30d resolve cache — this is the poster path for search
   *  results. `null` when nothing matched / TMDB unconfigured. Send the
   *  untouched release name; the backend parses title/year/kind out of
   *  it (one source of truth instead of a per-client SCENE parser). */
  tmdbResolve: (title: string, kind?: MediaKind | null) =>
    api.get<TmdbSuggestion | null>(
      `/metadata/tmdb/resolve?title=${encodeURIComponent(title)}${kind ? `&kind=${kind}` : ""}`,
    ),
};

// Torrent details (search result preview)

export type AudioInfo = components["schemas"]["AudioInfo"];
export type SubInfo = components["schemas"]["SubInfo"];
export type VideoInfo = components["schemas"]["VideoInfo"];
export type MediaInfoSummary = components["schemas"]["MediaInfoSummary"];
export type DescriptionFormat = components["schemas"]["DescriptionFormat"];
export type TorrentDetails = components["schemas"]["TorrentDetails"];

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

export type ProviderInfo = components["schemas"]["ProviderInfo"];

export const providers = {
  list: () => api.get<ProviderInfo[]>("/providers"),
};

export type ProgressView = components["schemas"]["ProgressView"];

/** Per-user playback language preferences (cross-file / cross-device). Applied
 *  by matching the file's tracks: per-file saved index wins, else this, else
 *  the file default. `subtitle_language: "off"` = subtitles disabled; null =
 *  no preference. Volume is NOT here — it's persisted device-locally. */
export type PlaybackPrefs = components["schemas"]["PlaybackPrefsResponse"];

export type ContinueWatchingItem = components["schemas"]["ContinueWatchingItem"];

/** A row of the caller's full watch history (in-progress AND completed),
 *  including items whose source torrent has since been deleted —
 *  `deleted: true` means there's nothing left to resume. */
export type HistoryItem = components["schemas"]["HistoryItem"];

/** Admin per-user drill-down equivalent of {@link HistoryItem} — same
 *  shape, reached through `/admin/users/{id}/history` instead of the
 *  caller's own session. */
export type UserHistoryItem = components["schemas"]["UserHistoryView"];

export type FileProgressEntry = components["schemas"]["FileProgressEntry"];

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
      /** Whether the player is actively playing (vs paused) at this
       *  heartbeat. Feeds the admin "Now watching" presence state. */
      playing?: boolean;
      /** True when this save follows a deliberate user seek. Required for
       *  a near-zero position to overwrite substantial stored progress —
       *  without it the server's reset guard treats the save as an
       *  error-recovery artifact and keeps the old position. */
      seek?: boolean;
    },
  ) => api.put<void>(`/torrents/${infohash}/files/${idx}/progress`, body),
  /** Remove this file from the caller's Continue Watching + history. */
  remove: (infohash: string, idx: number) =>
    api.delete<void>(`/torrents/${infohash}/files/${idx}/progress`),
  /** Mark this file watched for the caller (also skips a "next up" tile). */
  markWatched: (infohash: string, idx: number) =>
    api.post<void>(`/torrents/${infohash}/files/${idx}/progress/complete`),
};

/** Per-user recommendation preferences (Slice 1 of "For You"). The
 *  `languages` list uses the backend `Language` vocabulary
 *  ("french" / "english"), ordered most-preferred first; `genres`
 *  holds TMDB genre ids. `onboarding_completed` gates the first-login
 *  onboarding dialog. */
export type Preferences = components["schemas"]["PreferencesResponse"];

/** A recommendation candidate as rendered on a "For You" shelf. Shape is
 *  kept close to SearchResult / WatchlistItem so the same card renders it. */
export type CatalogCard = components["schemas"]["CatalogCard"];

export type ForYouShelf = components["schemas"]["Shelf"];

export type ForYouResponse = components["schemas"]["ForYou"];

/** One tile on the mood board (a curated mood + a taste-derived backdrop). */
export type MoodTile = components["schemas"]["MoodTile"];
export type MoodBoard = components["schemas"]["MoodBoard"];
export type MoodResults = components["schemas"]["MoodResults"];
export const me = {
  /** `include_grabbable` opts into synthesised "next episode isn't on
   *  disk yet" tiles (`grabbable: true`, empty infohash) — the web bundle
   *  ships with the backend so it always opts in. */
  continueWatching: () =>
    api.get<ContinueWatchingItem[]>("/me/continue-watching?include_grabbable=true"),
  /** Remove a tile from Continue Watching. For a TV series pass its
   *  `collection_id` (hides the whole show until a newer episode plays);
   *  for a movie / standalone pass `infohash` + `file_idx`. */
  dismissContinueWatching: (body: {
    collection_id?: string | null;
    infohash?: string;
    file_idx?: number;
  }) => api.post<void>("/me/continue-watching/dismiss", body),
  /** Per-user hide of a Gone entry (ghost collection via
   *  `collection_id`, single release via `infohash`). History stays;
   *  newer activity resurfaces it. */
  dismissGone: (body: { collection_id?: string | null; infohash?: string }) =>
    api.post<void>("/me/gone/dismiss", body),
  /** Full watch history — in-progress AND completed, survives deletion of
   *  the source torrent (see {@link HistoryItem}). */
  history: (limit?: number, offset?: number) =>
    api.get<HistoryItem[]>(
      `/me/history?${new URLSearchParams({
        ...(limit ? { limit: String(limit) } : {}),
        ...(offset ? { offset: String(offset) } : {}),
      }).toString()}`,
    ),
  watchlist: () => api.get<WatchlistItem[]>("/me/watchlist"),
  /** Remove from MY Watchlist (auto-recreated on next grab/play). */
  removeFromWatchlist: (normalized_name: string) =>
    api.post<void>("/me/watchlist/remove", { normalized_name }),
  preferences: () => api.get<Preferences>("/me/preferences"),
  savePreferences: (body: Preferences) => api.put<Preferences>("/me/preferences", body),
  /** The home blended "For You" shelf. */
  forYou: () => api.get<ForYouResponse>("/me/for-you"),
  /** The organized "For You" page (top picks + per-genre + anime sections). */
  forYouPage: () => api.get<ForYouResponse>("/me/for-you/page"),
  /** Hide a recommendation candidate from future shelves. */
  dismissForYou: (catalog_id: string) => api.post<void>("/me/for-you/dismiss", { catalog_id }),
  /** The mood board for a kind — TMDB genres ordered by the user's taste. */
  moodBoard: (kind: MediaKind) => api.get<MoodBoard>(`/me/moods?kind=${kind}`),
  /** Results for a genre — catalogue ∪ broad TMDB, recency-filtered, taste-ranked. */
  moodResults: (id: string, kind: MediaKind) =>
    api.get<MoodResults>(`/me/moods/${encodeURIComponent(id)}?kind=${kind}`),
  /** The user's preferred audio + subtitle language (applied across episodes
   *  / devices). */
  playbackPreferences: () => api.get<PlaybackPrefs>("/me/playback-preferences"),
  /** Save preferred audio + subtitle language. Send the full current state. */
  savePlaybackPreferences: (body: PlaybackPrefs) => api.put<void>("/me/playback-preferences", body),
};

export type FilePreview = components["schemas"]["TorrentFilePreview"];
export type TorrentPreview = components["schemas"]["TorrentPreview"];

export type FileEntry = components["schemas"]["FileEntry"];
export type TorrentSnapshot = components["schemas"]["TorrentSnapshot"];

export type TorrentView = components["schemas"]["TorrentView"];

export type IngestResponse = components["schemas"]["IngestResponse"];

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

export type MediaProbe = components["schemas"]["MediaProbe"];
export type VideoStream = components["schemas"]["VideoStream"];
export type AudioStream = components["schemas"]["AudioStream"];
export type PlayStatus = components["schemas"]["PlayStatus"];
export type SubtitleStream = components["schemas"]["SubtitleStream"];

// Discovery: featured carousels (torr9 /featured/{movies,series}, etc.)

export type FeaturedResponse = components["schemas"]["FeaturedResponse"];

/** One entry of TMDB's genre taxonomy (merged movie+TV, deduped). The
 *  `id` is what we persist in a user's `genres` preference. */
export type GenreOption = components["schemas"]["GenreOption"];

export type GenresResponse = components["schemas"]["GenresResponse"];

/** A user-selectable language: `value` is the backend `Language` wire
 *  token ("french"/"english"), `label` the display string. Served by the
 *  backend so adding a language needs no client redeploy. */
export type LanguageOption = components["schemas"]["LanguageOption"];

export type LanguagesResponse = components["schemas"]["LanguagesResponse"];

export const discover = {
  featured: () => api.get<FeaturedResponse>("/discover/featured"),
  /** Merged movie + TV genre taxonomy — feeds the onboarding picker.
   *  Note: served at the top-level `/api/genres`, not under /discover. */
  genres: () => api.get<GenresResponse>("/genres"),
  /** Server-driven selectable languages for onboarding. Top-level
   *  `/api/languages`. */
  languages: () => api.get<LanguagesResponse>("/languages"),
};

// Library — collections (default) or raw torrents (toggle)

export type CollectionListItem = components["schemas"]["CollectionListItem"];
export type LibraryResponse = components["schemas"]["LibraryResponse"];
export type CollectionEpisodeEntry = components["schemas"]["EpisodeEntry"];
export type SeasonPackEntry = components["schemas"]["SeasonPackEntry"];
export type AvailableEpisodeEntry = components["schemas"]["AvailableEpisodeEntry"];
/** A reclaimed release with indexer provenance — re-ingestable via
 *  {@link torrents.ingest}; carries the caller's watch state. */
export type GoneReleaseEntry = components["schemas"]["GoneReleaseEntry"];
/** Ghost twin of {@link CollectionEpisodeEntry} — reclaimed (S, E)
 *  rows with the caller's watch state. */
export type GoneEpisodeEntry = components["schemas"]["GoneEpisodeEntry"];
export type CollectionDetail = components["schemas"]["CollectionDetail"];

export const library = {
  list: (view: "collections" | "torrents" = "collections") =>
    api.get<LibraryResponse>(`/library?view=${view}`),
  collection: (id: string) => api.get<CollectionDetail>(`/library/collections/${id}`),
  /** Grab a specific (season, episode) for a TV collection. Idempotent —
   *  returns `already_grabbed: true` if the episode is already on disk
   *  under any infohash. When `language` is set, the server picks
   *  strictly from that language slot in the cache (no cross-language
   *  fallback) — used when the user clicked an FR / EN badge. */
  grabCollectionEpisode: (
    id: string,
    season: number,
    episode: number,
    language?: string | null,
  ) => {
    const qs = language ? `?language=${encodeURIComponent(language)}` : "";
    return api.post<GrabEpisodeResponse>(
      `/library/collections/${id}/grab/${season}/${episode}${qs}`,
      {},
    );
  },
};

// Series follows (Watchlist + Series detail page)

/// Post-0.4 Watchlist tile — returned by `/api/me/watchlist`.
/// Per-user: derived from the calling user's `series_follows`
/// rows (auto-created on grab). `id` is the collection id when one
/// already exists for this normalised name, otherwise the follow
/// row's own id (used as a routing token for `/collection/:id`).
export type WatchlistItem = components["schemas"]["WatchlistItem"];

export type FollowSummary = components["schemas"]["FollowSummary"];

export type EpisodeStatus = components["schemas"]["EpisodeStatus"];

export type EpisodeItem = components["schemas"]["EpisodeItem"];

export type EpisodesResponse = components["schemas"]["EpisodesResponse"];

export type EpisodePoint = components["schemas"]["EpisodePoint"];

export type EpisodeContext = components["schemas"]["EpisodeContext"];

export type GrabEpisodeResponse = components["schemas"]["GrabResponse"];

export const follows = {
  list: () => api.get<FollowSummary[]>("/me/follows"),
  add: (name: string, tmdb_id?: number | null) =>
    api.post<FollowSummary>("/me/follows", { name, tmdb_id: tmdb_id ?? null }),
  remove: (id: string) => api.delete<void>(`/me/follows/${id}`),
  /** Pass `season` to filter; omit for the full set. */
  episodes: (id: string, season?: number) =>
    api.get<EpisodesResponse>(
      season != null ? `/me/follows/${id}/episodes?season=${season}` : `/me/follows/${id}/episodes`,
    ),
  grabEpisode: (id: string, season: number, episode: number) =>
    api.post<GrabEpisodeResponse>(`/me/follows/${id}/episodes/${season}/${episode}/grab`),
  /** Fetch context for the file currently playing — drives the
   *  "Watch next?" modal at episode end. */
  episodeContext: (infohash: string, file_idx: number) =>
    api.get<EpisodeContext>(
      `/me/follows/episode-context?infohash=${encodeURIComponent(infohash)}&file_idx=${file_idx}`,
    ),
};

// Live TV: per-country IPTV channels + now/next guide, played through the
// backend's signed HLS proxy (see crates/iris-api/src/live_tv).

export type LiveCountry = components["schemas"]["LiveCountry"];
export type LiveCountriesResponse = components["schemas"]["LiveCountriesResponse"];
export type LiveChannel = components["schemas"]["LiveChannel"];
export type LiveChannelsResponse = components["schemas"]["LiveChannelsResponse"];
export type LiveProgramme = components["schemas"]["LiveProgramme"];
export type LiveNowNext = components["schemas"]["LiveNowNext"];
export type LiveEpgNowResponse = components["schemas"]["LiveEpgNowResponse"];
export type LiveSearchResponse = components["schemas"]["LiveSearchResponse"];
export type LiveSearchResult = components["schemas"]["LiveSearchResult"];

export const livetv = {
  countries: () => api.get<LiveCountriesResponse>("/livetv/countries"),
  /** Cross-country channel search (server-side, diacritics-insensitive). */
  search: (q: string) => api.get<LiveSearchResponse>(`/livetv/search?q=${encodeURIComponent(q)}`),
  channels: (country: string) =>
    api.get<LiveChannelsResponse>(`/livetv/${encodeURIComponent(country)}/channels`),
  epgNow: (country: string) =>
    api.get<LiveEpgNowResponse>(`/livetv/${encodeURIComponent(country)}/epg/now`),
  /** HLS master playlist for a channel — hand to hls.js / native HLS. */
  masterUrl: (country: string, channelId: string) =>
    `/api/livetv/${encodeURIComponent(country)}/channels/${encodeURIComponent(channelId)}/master.m3u8`,
  /** The served stream is unplayable client-side: the backend cools the
   *  active source down and elects the next feed. */
  reportPlaybackError: (country: string, channelId: string) =>
    api.post<void>(
      `/livetv/${encodeURIComponent(country)}/channels/${encodeURIComponent(channelId)}/playback-error`,
    ),
};
