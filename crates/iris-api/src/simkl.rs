//! SIMKL "trending today" — the public, keyless JSON snapshots SIMKL
//! publishes on its data CDN (`data.simkl.in/discover/trending/…`).
//!
//! A complementary pulse to TMDB trending: SIMKL ranks by what its users
//! actually watched today, not by page views. Each entry carries a TMDB id
//! (as a string), so it joins the same pipeline. Best-effort: any failure
//! yields an empty list and the pulse keeps its previous snapshot.

use std::time::Duration;

use serde::Deserialize;

use crate::tmdb::TmdbKind;

const BASE: &str = "https://data.simkl.in/discover/trending";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trending {
    pub tmdb_id: i64,
    pub title: String,
    /// Viewers who watched it today.
    pub watched: Option<i64>,
}

#[derive(Clone)]
pub struct SimklClient {
    http: reqwest::Client,
}

#[derive(Deserialize)]
struct RawEntry {
    title: Option<String>,
    #[serde(default)]
    ids: RawIds,
    watched: Option<i64>,
}

#[derive(Deserialize, Default)]
struct RawIds {
    /// A string on the wire (`"97546"`), occasionally empty.
    tmdb: Option<serde_json::Value>,
}

impl SimklClient {
    pub fn new() -> anyhow::Result<Self> {
        let http = iris_providers::tls::client_builder()
            .timeout(Duration::from_secs(20))
            .build()?;
        Ok(Self { http })
    }

    /// Today's top 100 for `kind`, in SIMKL's order.
    pub async fn trending_today(&self, kind: TmdbKind) -> Vec<Trending> {
        let path = match kind {
            TmdbKind::Movie => "movies",
            TmdbKind::Tv => "tv",
        };
        let url = format!("{BASE}/{path}/today_100.json");
        let res = match self.http.get(&url).send().await {
            Ok(r) if r.status().is_success() => r,
            Ok(r) => {
                tracing::warn!(status = %r.status(), path, "simkl trending refused");
                return Vec::new();
            }
            Err(e) => {
                tracing::warn!(error = %e, path, "simkl trending fetch failed");
                return Vec::new();
            }
        };
        match res.json::<Vec<RawEntry>>().await {
            Ok(raw) => parse(raw),
            Err(e) => {
                tracing::warn!(error = %e, path, "simkl trending parse failed");
                Vec::new()
            }
        }
    }
}

fn parse(raw: Vec<RawEntry>) -> Vec<Trending> {
    raw.into_iter()
        .filter_map(|e| {
            let tmdb_id = match e.ids.tmdb? {
                serde_json::Value::String(s) => s.trim().parse().ok()?,
                serde_json::Value::Number(n) => n.as_i64()?,
                _ => return None,
            };
            Some(Trending {
                tmdb_id,
                title: e.title.unwrap_or_default(),
                watched: e.watched,
            })
        })
        .filter(|t| t.tmdb_id > 0)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_keep_order_and_skip_missing_tmdb_ids() {
        let raw: Vec<RawEntry> = serde_json::from_str(
            r#"[
                {"title":"Ted Lasso","ids":{"simkl_id":1359610,"tmdb":"97546"},"watched":1792},
                {"title":"No Tmdb","ids":{"simkl_id":1,"tmdb":""}},
                {"title":"Numeric","ids":{"tmdb":42}},
                {"title":"Bare"}
            ]"#,
        )
        .unwrap();
        assert_eq!(
            parse(raw),
            vec![
                Trending {
                    tmdb_id: 97546,
                    title: "Ted Lasso".to_string(),
                    watched: Some(1792)
                },
                Trending {
                    tmdb_id: 42,
                    title: "Numeric".to_string(),
                    watched: None
                },
            ]
        );
    }
}
