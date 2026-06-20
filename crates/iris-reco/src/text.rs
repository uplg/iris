//! Builds the text blob that gets embedded for a catalogue item.
//!
//! Everything here is already-resolved strings — genre *names* (resolved upstream
//! from the cached TMDB taxonomy, never hardcoded here), top cast, keywords — so
//! this crate carries no TMDB-specific knowledge. Richer text means a sharper
//! content signal (cf. Moreira et al., arXiv:1907.07629).

/// The fields that feed an item's embedding text.
#[derive(Debug, Default, Clone)]
pub struct ItemText<'a> {
    pub title: &'a str,
    pub overview: Option<&'a str>,
    /// Genre names (already resolved id → name by the caller).
    pub genres: &'a [String],
    /// Top-billed cast names.
    pub cast: &'a [String],
    /// TMDB keyword names.
    pub keywords: &'a [String],
}

/// Assemble the embedding text: `"Title. Overview genre… cast… keyword…"`.
/// Empty fields are skipped; the result is never padded with stray whitespace.
#[must_use]
pub fn build(item: &ItemText<'_>) -> String {
    let mut s = String::new();
    s.push_str(item.title.trim());

    if let Some(ov) = item.overview {
        let ov = ov.trim();
        if !ov.is_empty() {
            s.push_str(". ");
            s.push_str(ov);
        }
    }

    for tokens in [item.genres, item.cast, item.keywords] {
        for token in tokens {
            let t = token.trim();
            if !t.is_empty() {
                s.push(' ');
                s.push_str(t);
            }
        }
    }

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_full_text() {
        let genres = vec!["Animation".to_owned(), "Family".to_owned()];
        let cast = vec!["Some Actor".to_owned()];
        let keywords = vec!["coming of age".to_owned()];
        let it = ItemText {
            title: "Luca",
            overview: Some("A boy spends a summer on the Italian Riviera."),
            genres: &genres,
            cast: &cast,
            keywords: &keywords,
        };
        assert_eq!(
            build(&it),
            "Luca. A boy spends a summer on the Italian Riviera. \
             Animation Family Some Actor coming of age"
        );
    }

    #[test]
    fn skips_empty_fields() {
        let it = ItemText {
            title: "  Title  ",
            overview: Some("   "),
            ..Default::default()
        };
        assert_eq!(build(&it), "Title");
    }
}
