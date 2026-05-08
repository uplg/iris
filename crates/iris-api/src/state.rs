use std::sync::Arc;

use iris_auth::jwt::Issuer;
use iris_config::{AppConfig, AuthConfig};
use iris_db::SqlitePool;
use iris_media::{HlsManager, ProbeCache};
use iris_providers::ProviderRegistry;
use iris_torrent::{Engine, Gc};

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
    pub hls: HlsManager,
    pub gc: Gc,
    pub probes: ProbeCache,
    pub tmdb: Option<TmdbClient>,
}

impl AppState {
    pub fn new(
        cfg: AppConfig,
        db: SqlitePool,
        providers: ProviderRegistry,
        engine: Arc<Engine>,
        hls: HlsManager,
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
        Self {
            inner: Arc::new(Inner {
                cfg,
                db,
                providers,
                jwt,
                engine,
                hls,
                gc,
                probes: ProbeCache::new(),
                tmdb,
            }),
        }
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
    pub fn jwt(&self) -> &Issuer {
        &self.inner.jwt
    }
    pub fn engine(&self) -> &Arc<Engine> {
        &self.inner.engine
    }
    pub fn hls(&self) -> &HlsManager {
        &self.inner.hls
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
            email: admin.email.clone(),
            password_hash: hash,
            is_admin: true,
        },
    )
    .await?;
    tracing::info!(email = %user.email, "bootstrapped admin user");
    Ok(())
}
