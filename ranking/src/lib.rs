//! Ranking sloj: spajanje, dedupe i ad-score filter.

pub mod ad_score;
pub mod intent;

pub use ad_score::{is_dirty, DIRTY_THRESHOLD};
pub use intent::{detect, Intent};

use search_core::{Query, SearchResult};
use sources::Source;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

/// Ukupni budžet agregacije — izvori imaju svoje timeoutove, ovo je kap.
pub const AGGREGATE_TIMEOUT: Duration = Duration::from_secs(10);

/// Rezervni timeout po izvoru u agregatoru (izvori imaju svoj kraći).
const SOURCE_TIMEOUT_FALLBACK: Duration = Duration::from_secs(9);

/// Poziva free izvore paralelno; plaćene (`is_paid`) samo kad free nemaju odgovor.
/// Svaki potrošeni Brave poziv se broji — zato fallback, ne paralelno sa svima.
pub async fn aggregate(sources: &[Arc<dyn Source>], query: &Query) -> Vec<SearchResult> {
    let (paid, free): (Vec<_>, Vec<_>) = sources
        .iter()
        .cloned()
        .partition(|s: &Arc<dyn Source>| s.is_paid());
    let mut lists = run_all(&free, query).await;
    if lists.iter().all(|l| l.is_empty()) {
        lists.extend(run_all(&paid, query).await);
    }
    rank(lists)
}

/// Poziva date izvore paralelno, greške loguje i preskače.
async fn run_all(sources: &[Arc<dyn Source>], query: &Query) -> Vec<Vec<SearchResult>> {
    let mut handles = Vec::with_capacity(sources.len());
    for source in sources {
        let (source, query) = (Arc::clone(source), query.clone());
        handles.push(tokio::spawn(async move {
            match tokio::time::timeout(SOURCE_TIMEOUT_FALLBACK, source.search(&query)).await {
                Ok(Ok(list)) => Some(list),
                Ok(Err(e)) => {
                    tracing::warn!(source = source.name(), error = %e, "source failed, skipped");
                    None
                }
                Err(_) => {
                    tracing::warn!(source = source.name(), "source timed out, skipped");
                    None
                }
            }
        }));
    }
    let gather = async {
        let mut lists = Vec::with_capacity(handles.len());
        for handle in handles {
            if let Ok(Some(list)) = handle.await {
                lists.push(list);
            }
        }
        lists
    };
    let lists = tokio::time::timeout(AGGREGATE_TIMEOUT, gather)
        .await
        .unwrap_or_default();
    lists
}

/// Reciprocal-rank konstanta (standardna RRF vrednost).
const RRF_K: f32 = 60.0;

/// Rangira liste: reciprocal-rank fusion po poziciji u izvoru,
/// penal za ad-score, deterministički poredak.
///
/// Skor: `sum(1/(60+pozicija)) * (1 - ad_score/200)`.
/// Isti skor → redosled po URL-u (deterministički).
#[must_use]
pub fn rank(lists: Vec<Vec<SearchResult>>) -> Vec<SearchResult> {
    let mut scored: Vec<(SearchResult, f32, String)> = Vec::new();
    let mut seen = HashSet::new();
    for list in &lists {
        for (pos, r) in list.iter().enumerate() {
            let key = normalize_url(&r.url);
            if !seen.insert(key.clone()) {
                continue;
            }
            let base = 1.0 / (RRF_K + pos as f32);
            let penalty = 1.0 - f32::from(ad_score::ad_score(&r.url)) / 200.0;
            scored.push((r.clone(), base * penalty, key));
        }
    }
    scored.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.2.cmp(&b.2))
    });
    scored.into_iter().map(|(r, _, _)| r).collect()
}

/// Normalizuje URL za dedupe: mala slova, bez trailing `/`, bez fragmenta.
#[must_use]
pub fn normalize_url(url: &str) -> String {
    let lower = url.trim().to_lowercase();
    let no_frag = lower.split('#').next().unwrap_or(&lower);
    no_frag.trim_end_matches('/').to_string()
}

/// Spaja liste iz više izvora, izbacuje duplikate (prvi viđeni pobeđuje).
#[must_use]
pub fn merge(lists: Vec<Vec<SearchResult>>) -> Vec<SearchResult> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for list in lists {
        for r in list {
            if seen.insert(normalize_url(&r.url)) {
                out.push(r);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(url: &str, source: &str) -> SearchResult {
        SearchResult {
            title: "t".into(),
            url: url.into(),
            snippet: "s".into(),
            source: source.into(),
        }
    }

    #[test]
    fn dedupe_keeps_first_seen() {
        let merged = merge(vec![
            vec![r("https://a.com/x", "brave"), r("https://b.com/", "brave")],
            vec![
                r("https://A.com/x#frag", "wiki"),
                r("https://c.com", "wiki"),
            ],
        ]);
        let urls: Vec<_> = merged.iter().map(|x| x.url.as_str()).collect();
        assert_eq!(
            urls,
            vec!["https://a.com/x", "https://b.com/", "https://c.com"]
        );
        assert_eq!(merged[0].source, "brave");
    }

    #[test]
    fn normalize_handles_case_slash_fragment() {
        assert_eq!(normalize_url("HTTPS://A.COM/X/#F"), "https://a.com/x");
    }

    struct Fixed(&'static str, Vec<SearchResult>, bool);
    #[async_trait::async_trait]
    impl sources::Source for Fixed {
        fn name(&self) -> &'static str {
            self.0
        }
        async fn search(&self, _q: &Query) -> anyhow::Result<Vec<SearchResult>> {
            if self.2 {
                return Err(anyhow::anyhow!("boom"));
            }
            Ok(self.1.clone())
        }
    }

    fn fixed(name: &'static str, urls: &[&str], fail: bool) -> Arc<dyn sources::Source> {
        Arc::new(Fixed(
            name,
            urls.iter()
                .map(|u| SearchResult {
                    title: "t".into(),
                    url: (*u).into(),
                    snippet: "s".into(),
                    source: name.into(),
                })
                .collect(),
            fail,
        ))
    }

    #[tokio::test]
    async fn aggregate_merges_and_skips_failures() {
        let sources: Vec<Arc<dyn sources::Source>> = vec![
            fixed("one", &["https://a.com", "https://b.com"], false),
            fixed("two", &["https://b.com", "https://c.com"], false),
            fixed("bad", &[], true),
        ];
        let out = aggregate(&sources, &Query::new("x").expect("q")).await;
        let urls: Vec<_> = out.iter().map(|r| r.url.as_str()).collect();
        assert_eq!(
            urls,
            vec!["https://a.com", "https://b.com", "https://c.com"]
        );
    }

    #[tokio::test]
    async fn aggregate_empty_sources_means_empty() {
        let sources: Vec<Arc<dyn sources::Source>> = vec![];
        let out = aggregate(&sources, &Query::new("x").expect("q")).await;
        assert!(out.is_empty());
    }

    struct Counting {
        name: &'static str,
        results: Vec<SearchResult>,
        paid: bool,
        hits: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    }
    #[async_trait::async_trait]
    impl sources::Source for Counting {
        fn name(&self) -> &'static str {
            self.name
        }
        fn is_paid(&self) -> bool {
            self.paid
        }
        async fn search(&self, _q: &Query) -> anyhow::Result<Vec<SearchResult>> {
            self.hits.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(self.results.clone())
        }
    }

    fn counting(
        name: &'static str,
        urls: &[&str],
        paid: bool,
        hits: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    ) -> Arc<dyn sources::Source> {
        Arc::new(Counting {
            name,
            results: urls
                .iter()
                .map(|u| SearchResult {
                    title: "t".into(),
                    url: (*u).into(),
                    snippet: "s".into(),
                    source: name.into(),
                })
                .collect(),
            paid,
            hits,
        })
    }

    #[tokio::test]
    async fn aggregate_skips_paid_when_free_has_results() {
        use std::sync::atomic::Ordering;
        let paid_hits = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let sources: Vec<Arc<dyn sources::Source>> = vec![
            counting(
                "free",
                &["https://a.com"],
                false,
                std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            ),
            counting(
                "paid",
                &["https://b.com"],
                true,
                std::sync::Arc::clone(&paid_hits),
            ),
        ];
        let out = aggregate(&sources, &Query::new("x").expect("q")).await;
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].url, "https://a.com");
        assert_eq!(paid_hits.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn aggregate_calls_paid_when_free_empty() {
        use std::sync::atomic::Ordering;
        let paid_hits = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let sources: Vec<Arc<dyn sources::Source>> = vec![
            counting(
                "free",
                &[],
                false,
                std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            ),
            counting(
                "paid",
                &["https://b.com"],
                true,
                std::sync::Arc::clone(&paid_hits),
            ),
        ];
        let out = aggregate(&sources, &Query::new("x").expect("q")).await;
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].url, "https://b.com");
        assert_eq!(paid_hits.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn rank_pushes_dirty_down_and_stays_deterministic() {
        let lists = vec![vec![
            r("https://www.dailymail.co.uk/news/x", "brave"),
            r("https://en.wikipedia.org/wiki/Rust", "wiki"),
            r("https://example.com/article", "brave"),
        ]];
        let once: Vec<_> = rank(lists.clone()).iter().map(|x| x.url.clone()).collect();
        let twice: Vec<_> = rank(lists).iter().map(|x| x.url.clone()).collect();
        assert_eq!(once, twice, "redosled mora biti deterministički");
        assert_eq!(once[0], "https://en.wikipedia.org/wiki/Rust");
        assert_eq!(once[2], "https://www.dailymail.co.uk/news/x");
    }
}
