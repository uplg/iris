use std::sync::Arc;

use iris_auth::jwt::Issuer;
use iris_config::{AppConfig, AuthConfig};
use iris_db::SqlitePool;
use iris_media::{ProbeCache, RemuxManager};
use iris_providers::ProviderRegistry;
use iris_torrent::{Engine, Gc};

use crate::anilist::AniListClient;
use crate::live_tv::LiveTvService;
use crate::presence::Presence;
use crate::tmdb::TmdbClient;

#[derive(Clone)]
pub struct AppState {
    inner: Arc<Inner>,
}

struct Inner {
    pub cfg: AppConfig,
    pub db: SqlitePool,
    pub providers: ProviderRegistry,
    pub jwt: Issuer,
    pub engine: Arc<Engine>,
    pub remuxer: RemuxManager,
    pub gc: Gc,
    pub probes: ProbeCache,
    pub tmdb: Option<TmdbClient>,
    pub anilist: Option<AniListClient>,
    pub presence: Presence,
    pub live_tv: Option<LiveTvService>,
    pub passkeys: Option<crate::passkeys::Passkeys>,
}

impl AppState {
    pub fn new(
        cfg: AppConfig,
        db: SqlitePool,
        providers: ProviderRegistry,
        engine: Arc<Engine>,
        remuxer: RemuxManager,
        gc: Gc,
    ) -> Self {
        let tmdb = cfg
            .tmdb
            .as_ref()
            .and_then(|c| match TmdbClient::new(c.api_key.clone()) {
                Ok(client) => Some(client),
                Err(e) => {
                    tracing::warn!(error = %e, "tmdb client init failed; metadata disabled");
                    None
                }
            });
        let jwt = Issuer::new(
            &cfg.auth.jwt_secret,
            cfg.server.public_url.clone(),
            cfg.auth.access_ttl_secs,
            cfg.auth.refresh_ttl_secs,
        );
        // Live TV proxy/catalogue — config-gated; a build failure disables
        // the feature instead of blocking boot.
        let live_tv = cfg
            .live_tv
            .enabled
            .then(|| LiveTvService::new(cfg.live_tv.clone(), &cfg.auth.jwt_secret))
            .and_then(|r| match r {
                Ok(svc) => Some(svc),
                Err(e) => {
                    tracing::warn!(error = %e, "live tv init failed; feature disabled");
                    None
                }
            });
        // Keyless public GraphQL client — used to enrich anime
        // collections with an AniList id (poster / recommendations) and
        // to corroborate the offline anime classifier. Best-effort: a
        // build failure just disables the enrichment. The one instance:
        // the schedulers get clones, so its 2 s throttle covers every
        // AniList request the process makes.
        let anilist = match AniListClient::new() {
            Ok(client) => Some(client),
            Err(e) => {
                tracing::warn!(error = %e, "anilist client init failed; anime enrichment disabled");
                None
            }
        };
        // Passkeys hang off `public_url` (the RP ID can't change without
        // losing every passkey): an unusable URL turns them off, never boot.
        let passkeys = match crate::passkeys::Passkeys::new(&cfg.server.public_url) {
            Ok(p) => Some(p),
            Err(e) => {
                tracing::warn!(error = %e, "passkeys disabled");
                None
            }
        };
        Self {
            inner: Arc::new(Inner {
                cfg,
                db,
                providers,
                jwt,
                engine,
                remuxer,
                gc,
                probes: ProbeCache::new(),
                tmdb,
                anilist,
                presence: Presence::new(),
                live_tv,
                passkeys,
            }),
        }
    }

    /// A state over `db` and `providers` whose engine never touches the
    /// network, for route tests. Its files live in a fresh temp dir.
    #[cfg(test)]
    pub(crate) async fn for_tests(db: SqlitePool, providers: ProviderRegistry) -> Self {
        let dir = std::env::temp_dir().join(format!("iris-state-{}", uuid::Uuid::new_v4()));
        let cfg: AppConfig = serde_json::from_value(serde_json::json!({
            "server": {},
            "storage": { "data_dir": dir.join("data"), "download_dir": dir.join("downloads") },
            "auth": { "jwt_secret": "test-secret-test-secret-test-secret" },
        }))
        .expect("test config");
        let engine = Engine::offline(cfg.storage.download_dir.clone())
            .await
            .expect("offline engine");
        let remuxer = RemuxManager::with_encode_config(
            dir.join("remux"),
            iris_media::EncodeConfig {
                preset: cfg.transcode.preset.clone(),
                crf: cfg.transcode.crf,
            },
        );
        let gc = Gc::new(
            engine.clone(),
            db.clone(),
            iris_torrent::GcConfig {
                max_storage_bytes: cfg.storage.max_storage_bytes(),
                cleanup_threshold_pct: cfg.storage.cleanup_threshold_pct,
                cleanup_target_pct: cfg.storage.cleanup_target_pct,
                interval: std::time::Duration::from_mins(15),
                active_window: std::time::Duration::from_hours(1),
            },
            cfg.storage.download_dir.clone(),
            None,
            |_| {},
        );
        Self::new(cfg, db, providers, engine, remuxer, gc)
    }

    /// The passkey service, or 404 when this server can't offer passkeys.
    pub fn passkeys(&self) -> crate::error::ApiResult<&crate::passkeys::Passkeys> {
        self.inner
            .passkeys
            .as_ref()
            .ok_or(crate::error::ApiError::NotFound)
    }
    pub fn cfg(&self) -> &AppConfig {
        &self.inner.cfg
    }
    pub fn db(&self) -> &SqlitePool {
        &self.inner.db
    }
    pub fn providers(&self) -> &ProviderRegistry {
        &self.inner.providers
    }
    /// The enabled provider `id`: a `409 provider_off` when an admin turned
    /// it off, a `400` naming it when the config doesn't build it.
    pub fn provider(
        &self,
        id: &str,
    ) -> Result<std::sync::Arc<dyn iris_providers::SearchProvider>, crate::error::ApiError> {
        self.providers().get(id).ok_or_else(|| {
            if self.providers().is_switched_off(id) {
                crate::error::ApiError::ProviderOff
            } else {
                crate::error::ApiError::BadRequest(format!("unknown provider `{id}`"))
            }
        })
    }
    pub fn jwt(&self) -> &Issuer {
        &self.inner.jwt
    }
    pub fn engine(&self) -> &Arc<Engine> {
        &self.inner.engine
    }
    pub fn remuxer(&self) -> &RemuxManager {
        &self.inner.remuxer
    }
    pub fn gc(&self) -> &Gc {
        &self.inner.gc
    }
    pub fn probes(&self) -> &ProbeCache {
        &self.inner.probes
    }
    pub fn tmdb(&self) -> Option<&TmdbClient> {
        self.inner.tmdb.as_ref()
    }
    pub fn anilist(&self) -> Option<&AniListClient> {
        self.inner.anilist.as_ref()
    }
    pub fn presence(&self) -> &Presence {
        &self.inner.presence
    }
    pub fn live_tv(&self) -> Option<&LiveTvService> {
        self.inner.live_tv.as_ref()
    }
}

/// If `auth.bootstrap_admin` is set and there are zero users in the DB,
/// create the admin account. Idempotent: a no-op once any user exists.
pub async fn bootstrap_admin_if_configured(
    pool: &SqlitePool,
    auth: &AuthConfig,
) -> anyhow::Result<()> {
    let Some(admin) = auth.bootstrap_admin.as_ref() else {
        return Ok(());
    };
    let n = iris_db::users::count(pool).await?;
    if n > 0 {
        tracing::debug!("users table not empty, skipping bootstrap admin");
        return Ok(());
    }
    let hash = iris_auth::hash_password(&admin.password)
        .map_err(|e| anyhow::anyhow!("hash admin password: {e}"))?;
    let user = iris_db::users::create(
        pool,
        iris_db::users::NewUser {
            // Match the runtime normalization in routes::auth so the
            // bootstrap admin can later log in regardless of how the
            // operator capitalised the address in the config file.
            email: admin.email.trim().to_ascii_lowercase(),
            password_hash: hash,
            is_admin: true,
        },
    )
    .await?;
    tracing::info!(email = %user.email, "bootstrapped admin user");
    Ok(())
}
