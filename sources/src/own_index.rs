//! Sopstveni indeks kao izvor (`own-index`).
//!
//! Omot oko `index::SearchIndex` iza async brave: pretraga je
//! in-memory i brza, pa je blokirajući lock prihvatljiv.
//! Punjenje indeksa (crawler → indeks) je zaseban korak.

use async_trait::async_trait;
use index::{Doc, SearchIndex};
use search_core::{Query, SearchResult};
use tokio::sync::Mutex;

/// Izvor nad sopstvenim Tantivy indeksom.
pub struct OwnIndexSource {
    index: Mutex<SearchIndex>,
}

impl OwnIndexSource {
    /// Pravi izvor sa početnim dokumentima (prazno = ugašen do punjenja).
    pub fn with_docs(docs: &[Doc]) -> anyhow::Result<Self> {
        let mut index = SearchIndex::open().map_err(|e| anyhow::anyhow!("{e}"))?;
        index.add(docs).map_err(|e| anyhow::anyhow!("{e}"))?;
        Ok(Self {
            index: Mutex::new(index),
        })
    }

    /// Otvara (ili pravi) indeks na disku.
    pub fn open_dir(path: &std::path::Path) -> anyhow::Result<Self> {
        let index = SearchIndex::open_dir(path).map_err(|e| anyhow::anyhow!("{e}"))?;
        Ok(Self {
            index: Mutex::new(index),
        })
    }
}

#[async_trait]
impl super::Source for OwnIndexSource {
    fn name(&self) -> &'static str {
        "own-index"
    }

    async fn search(&self, query: &Query) -> anyhow::Result<Vec<SearchResult>> {
        let index = self.index.lock().await;
        let hits = index
            .search(&query.text)
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        Ok(hits
            .into_iter()
            .map(|h| SearchResult {
                title: if h.title.is_empty() {
                    h.url.clone()
                } else {
                    h.title
                },
                url: h.url,
                snippet: String::new(),
                source: "own-index".into(),
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Source;

    #[tokio::test]
    async fn finds_seeded_doc() {
        let src = OwnIndexSource::with_docs(&[Doc {
            title: "Rust guide".into(),
            body: "systems programming language tutorial".into(),
            url: "https://example.com/rust".into(),
        }])
        .expect("seed");
        let out = src
            .search(&Query::new("systems tutorial").expect("q"))
            .await
            .expect("search");
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].source, "own-index");
    }

    #[tokio::test]
    async fn empty_index_means_empty() {
        let src = OwnIndexSource::with_docs(&[]).expect("seed");
        let out = src
            .search(&Query::new("anything").expect("q"))
            .await
            .expect("search");
        assert!(out.is_empty());
    }

    #[tokio::test]
    async fn open_dir_seeded_and_found() {
        let dir = std::env::temp_dir().join(format!("rs-src-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut idx = index::SearchIndex::open_dir(&dir).expect("create");
        idx.add(&[Doc {
            title: "Guide".into(),
            body: "systems tutorial".into(),
            url: "https://x.com/1".into(),
        }])
        .expect("add");
        drop(idx);
        let src = OwnIndexSource::open_dir(&dir).expect("open");
        let out = src
            .search(&Query::new("systems tutorial").expect("q"))
            .await
            .expect("search");
        assert_eq!(out.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
