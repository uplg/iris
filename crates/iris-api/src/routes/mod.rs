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
