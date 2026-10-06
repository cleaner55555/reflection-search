//! Zajednički tipovi pretrage: upit, rezultat, greške.
//!
//! Svi crate-ovi zavise od ovog — ovde nema mreže ni IO.

use serde::{Deserialize, Serialize};

/// Normalizovan upit pretrage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Query {
    /// Sirov tekst koji je korisnik uneo.
    pub text: String,
}

impl Query {
    /// Pravi upit; prazan/blank tekst je greška.
    pub fn new(text: impl Into<String>) -> Result<Self, CoreError> {
        let text = text.into();
        if text.trim().is_empty() {
            return Err(CoreError::EmptyQuery);
        }
        Ok(Self { text })
    }
}

/// Jedan normalizovan rezultat, bez obzira na izvor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchResult {
    /// Naslov rezultata.
    pub title: String,
    /// Kanonski URL.
    pub url: String,
    /// Kratak opis/snippet.
    pub snippet: String,
    /// Ime izvora (npr. "brave", "wikipedia", "own-index").
    pub source: String,
}

/// Greške jezgra — mapiranju se u HTTP statuse na API granici.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CoreError {
    /// Upit bez teksta.
    #[error("query must not be empty")]
    EmptyQuery,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blank_query_is_rejected() {
        assert_eq!(Query::new("   "), Err(CoreError::EmptyQuery));
        assert_eq!(Query::new(""), Err(CoreError::EmptyQuery));
    }

    #[test]
    fn valid_query_is_kept_verbatim() {
        let q = Query::new("rust tantivy").expect("valid query");
        assert_eq!(q.text, "rust tantivy");
    }
}
