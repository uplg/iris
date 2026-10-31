//! The one sentence said after an update, read from `CHANGELOG.md` at build
//! time: the quote line right under the running version's heading.

use axum::Json;
use serde::Serialize;
use utoipa::ToSchema;

use crate::routes::extract::AuthUser;

const CHANGELOG: &str = include_str!("../../../../CHANGELOG.md");

#[derive(Debug, Serialize, ToSchema)]
pub struct ReleaseNote {
    /// The version this server runs.
    pub version: String,
    /// What changed, in a sentence. Absent when the release has none to say.
    pub note: Option<String>,
}

#[utoipa::path(
    get,
    path = "/api/release-note",
    operation_id = "releaseNote",
    responses((status = 200, description = "The running version and its update sentence", body = ReleaseNote)),
    tag = "health",
)]
pub async fn get(_user: AuthUser) -> Json<ReleaseNote> {
    let version = env!("CARGO_PKG_VERSION");
    Json(ReleaseNote {
        version: version.to_owned(),
        note: sentence_of(CHANGELOG, version).map(str::to_owned),
    })
}

fn sentence_of<'a>(changelog: &'a str, version: &str) -> Option<&'a str> {
    let heading = format!("## [{version}]");
    let mut lines = changelog.lines().skip_while(|l| !l.starts_with(&heading));
    lines.next()?;
    let first = lines.find(|l| !l.trim().is_empty())?;
    let sentence = first.strip_prefix('>')?.trim();
    (!sentence.is_empty()).then_some(sentence)
}

#[cfg(test)]
mod tests {
    use super::{CHANGELOG, sentence_of};

    const LOG: &str = "## [Unreleased]\n\n## [2.0.0] - 2026-11-02\n\n> Two more trackers.\n\n### Added\n\n- x\n\n## [1.9.0] - 2026-10-01\n\n### Fixed\n\n> not a note\n";

    #[test]
    fn reads_the_quote_right_under_the_heading() {
        assert_eq!(sentence_of(LOG, "2.0.0"), Some("Two more trackers."));
    }

    #[test]
    fn a_release_without_a_quote_says_nothing() {
        assert_eq!(sentence_of(LOG, "1.9.0"), None);
    }

    #[test]
    fn an_unknown_version_says_nothing() {
        assert_eq!(sentence_of(LOG, "3.0.0"), None);
    }

    #[test]
    fn the_changelog_is_embedded() {
        assert!(CHANGELOG.contains("## [1.6.4]"));
    }
}
