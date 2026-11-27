//! Secret entries of the command palette: shown only when the query is exactly one of
//! their keywords, never found by the fuzzy search.

use crate::i18n::{Lang, Msg};

/// A secret palette entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Secret {
    /// The Answer to the Ultimate Question of Life, the Universe, and Everything.
    Answer,
    /// The author's GitHub page.
    Author,
    /// The ASCII drawing of graphene.
    Graphene,
}

impl Secret {
    const ALL: [Self; 3] = [Self::Answer, Self::Author, Self::Graphene];

    /// The query that reveals it.
    fn keyword(self) -> &'static str {
        match self {
            Self::Answer => "42",
            Self::Author => "marmotz",
            Self::Graphene => "graphene",
        }
    }

    /// Label of its palette entry.
    pub fn label(self) -> Msg {
        match self {
            Self::Answer => Msg::SecretAnswer,
            Self::Author => Msg::SecretAuthor,
            Self::Graphene => Msg::SecretGraphene,
        }
    }
}

/// The author's GitHub page, opened by [`Secret::Author`].
pub const AUTHOR_URL: &str = "https://github.com/marmotz";

/// The secret whose keyword is exactly `query`, trimmed and ignoring case.
pub fn find(query: &str) -> Option<Secret> {
    let query = query.trim();
    Secret::ALL
        .into_iter()
        .find(|secret| secret.keyword().eq_ignore_ascii_case(query))
}

/// The page about the Answer in `lang`, opened by [`Secret::Answer`]. A language
/// without its own page opens the English one.
pub fn answer_url(lang: Lang) -> &'static str {
    match lang {
        Lang::EnUs | Lang::EnGb | Lang::Es | Lang::De | Lang::It => {
            "https://en.wikipedia.org/wiki/Phrases_from_The_Hitchhiker%27s_Guide_to_the_Galaxy#The_Answer_to_the_Ultimate_Question_of_Life,_the_Universe,_and_Everything_is_42"
        }
        Lang::Fr | Lang::FrCa => {
            "https://fr.wikipedia.org/wiki/La_grande_question_sur_la_vie,_l%27Univers_et_le_reste"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::LANGS;

    #[test]
    fn find_matches_a_keyword_exactly_trimmed_and_ignoring_case() {
        assert_eq!(find("42"), Some(Secret::Answer));
        assert_eq!(find("marmotz"), Some(Secret::Author));
        assert_eq!(find("graphene"), Some(Secret::Graphene));
        assert_eq!(find("  MarmotZ "), Some(Secret::Author));
        assert_eq!(find("GRAPHENE"), Some(Secret::Graphene));
    }

    #[test]
    fn find_ignores_partial_or_longer_queries() {
        for query in ["", "4", "graph", "422", "marmot", "graphenes", "4 2"] {
            assert_eq!(find(query), None, "{query:?}");
        }
    }

    #[test]
    fn answer_url_is_a_wikipedia_page_in_every_language() {
        for lang in LANGS {
            let url = answer_url(lang);
            assert!(url.starts_with("https://"), "{lang:?}");
            assert!(url.contains("wikipedia.org/wiki/"), "{lang:?}");
        }
        assert!(answer_url(Lang::Fr).starts_with("https://fr."));
        assert!(answer_url(Lang::EnUs).starts_with("https://en."));
    }
}
