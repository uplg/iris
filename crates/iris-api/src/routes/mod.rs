pub mod admin;
pub mod auth;
pub mod devices;
pub mod discover;
pub mod extract;
pub mod follows;
pub mod foryou;
pub mod health;
pub mod library;
pub mod live_tv;
pub mod me;
pub mod metadata;
pub mod moods;
pub mod passkeys;
pub mod playback_preferences;
pub mod preferences;
pub mod providers;
pub mod search;
pub mod torrents;

/// `?limit=&offset=` of the history-style listings.
#[derive(Debug, serde::Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct PageQuery {
    /// Max rows to return (clamped 1..=200, defaults to 50).
    limit: Option<i64>,
    /// Pagination offset (defaults to 0).
    offset: Option<i64>,
}

impl PageQuery {
    pub(crate) fn limit(&self) -> i64 {
        page_limit(self.limit)
    }

    pub(crate) fn offset(&self) -> i64 {
        self.offset.unwrap_or(0).max(0)
    }
}

/// The documented `limit` clamp of every history-style listing.
pub(crate) fn page_limit(limit: Option<i64>) -> i64 {
    limit.unwrap_or(50).clamp(1, 200)
}

/// Write an audit-log entry, best-effort: a failed write is logged, never
/// turned into a failed request.
pub(crate) async fn audit(
    state: &crate::state::AppState,
    actor: iris_core::ids::UserId,
    action: &str,
    resource_type: &str,
    resource_id: Option<&str>,
    details: Option<&str>,
) {
    if let Err(e) = iris_db::audit::record(
        state.db(),
        actor,
        action,
        resource_type,
        resource_id,
        details,
    )
    .await
    {
        tracing::warn!(error = %e, action, resource_id, "audit log write failed");
    }
}
