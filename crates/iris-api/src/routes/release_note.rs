//! The one sentence said after an update, read from `CHANGELOG.md` at build
//! time: the quote line right under the running version's heading.

use axum::Json;
use axum::http::HeaderMap;
use semver::Version;
use serde::Serialize;
use utoipa::ToSchema;

use crate::client_version::{CLIENT_HEADER, ClientVersion};
use crate::routes::extract::AuthUser;

const CHANGELOG: &str = include_str!("../../../../CHANGELOG.md");

#[derive(Debug, Serialize, ToSchema)]
pub struct ReleaseNote {
    /// The version this server runs, or the client's own while it is behind.
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
pub async fn get(_user: AuthUser, headers: HeaderMap) -> Json<ReleaseNote> {
    let client = headers
        .get(CLIENT_HEADER)
        .and_then(|h| h.to_str().ok())
        .and_then(ClientVersion::parse);
    Json(note_for(
        CHANGELOG,
        env!("CARGO_PKG_VERSION"),
        client.map(|c| c.version).as_ref(),
    ))
}

/// A client behind this server hasn't got the update yet (a TV also shows its
/// own "update available" banner): nothing is said, and it reads back its own
/// version so its "seen" stays put and the sentence comes once it has updated.
/// Shipped APKs need no change for this. No header: the server's version.
fn note_for(changelog: &str, running: &str, client: Option<&Version>) -> ReleaseNote {
    if let (Some(client), Ok(server)) = (client, Version::parse(running))
        && *client < server
    {
        return ReleaseNote {
            version: client.to_string(),
            note: None,
        };
    }
    ReleaseNote {
        version: running.to_owned(),
        note: sentence_of(changelog, running).map(str::to_owned),
    }
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
    use super::{CHANGELOG, Version, note_for, sentence_of};

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
    fn a_client_behind_the_server_hears_nothing_yet() {
        let note = note_for(LOG, "2.0.0", Some(&Version::new(1, 9, 0)));
        assert_eq!(note.version, "1.9.0");
        assert_eq!(note.note, None);
    }

    #[test]
    fn a_client_at_or_past_the_server_hears_the_sentence() {
        for client in [Version::new(2, 0, 0), Version::new(2, 0, 1)] {
            let note = note_for(LOG, "2.0.0", Some(&client));
            assert_eq!(note.version, "2.0.0");
            assert_eq!(note.note.as_deref(), Some("Two more trackers."));
        }
    }

    #[test]
    fn a_client_without_the_header_hears_the_sentence() {
        let note = note_for(LOG, "2.0.0", None);
        assert_eq!(note.version, "2.0.0");
        assert_eq!(note.note.as_deref(), Some("Two more trackers."));
    }

    #[test]
    fn the_changelog_is_embedded() {
        assert!(CHANGELOG.contains("## [1.6.4]"));
    }
}
