//! OpenAlex izvor — akademski radovi, besplatno, bez ključa.
//!
//! `GET api.openalex.org/works?search=&per-page=` vraća radove
//! (naslov, DOI, godina, autori, citiranost).

use async_trait::async_trait;
use search_core::{Query, SearchResult};

use super::{Source, SOURCE_TIMEOUT};

/// Podrazumevana baza; testovi gađaju lokalni stub.
pub const DEFAULT_BASE_URL: &str = "https://api.openalex.org";

/// Klijent ka OpenAlex API-ju.
pub struct OpenAlexSource {
    client: reqwest::Client,
    base_url: String,
}

impl OpenAlexSource {
    /// Pravi izvor ka pravom API-ju.
    #[must_use]
    pub fn new() -> Self {
        Self::with_base(DEFAULT_BASE_URL)
    }

    /// Pravi izvor sa eksplicitnom bazom (testovi).
    #[must_use]
    pub fn with_base(base_url: impl Into<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: base_url.into(),
        }
    }
}

impl Default for OpenAlexSource {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, serde::Deserialize, Default)]
struct Resp {
    #[serde(default)]
    results: Vec<Work>,
}

#[derive(Debug, serde::Deserialize, Default)]
struct Work {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    doi: Option<String>,
    #[serde(default)]
    id: String,
    #[serde(default)]
    publication_year: Option<u32>,
    #[serde(default)]
    cited_by_count: u32,
    #[serde(default)]
    authorships: Vec<Authorship>,
}

#[derive(Debug, serde::Deserialize)]
struct Authorship {
    author: Author,
}

#[derive(Debug, serde::Deserialize)]
struct Author {
    #[serde(default)]
    display_name: String,
}

fn snippet(w: &Work) -> String {
    let mut parts = Vec::new();
    if let Some(y) = w.publication_year {
        parts.push(y.to_string());
    }
    let names: Vec<&str> = w
        .authorships
        .iter()
        .map(|a| a.author.display_name.as_str())
        .filter(|n| !n.is_empty())
        .take(3)
        .collect();
    if !names.is_empty() {
        let mut s = names.join(", ");
        if w.authorships.len() > 3 {
            s.push_str(" et al.");
        }
        parts.push(s);
    }
    if w.cited_by_count > 0 {
        parts.push(format!("cited by {}", w.cited_by_count));
    }
    parts.join(" · ")
}

#[async_trait]
impl Source for OpenAlexSource {
    fn name(&self) -> &'static str {
        "openalex"
    }

    async fn search(&self, query: &Query) -> anyhow::Result<Vec<SearchResult>> {
        let url = format!(
            "{}/works?search={}&per-page=10&select=id,doi,title,publication_year,authorships,cited_by_count",
            self.base_url,
            super::brave::urlencode(&query.text)
        );
        let res = tokio::time::timeout(
            SOURCE_TIMEOUT,
            self.client
                .get(url)
                .header("User-Agent", "reflection-search/0.1 (contact: local-dev)")
                .send(),
        )
        .await
        .map_err(|_| anyhow::anyhow!("openalex: timeout"))?
        .map_err(|e| anyhow::anyhow!("openalex: request: {e}"))?;
        if !res.status().is_success() {
            return Err(anyhow::anyhow!("openalex: status {}", res.status()));
        }
        let body: Resp = res
            .json()
            .await
            .map_err(|e| anyhow::anyhow!("openalex: parse: {e}"))?;
        Ok(body
            .results
            .into_iter()
            .filter_map(|w| {
                let url = w.doi.clone().filter(|d| !d.is_empty()).or_else(|| {
                    if w.id.is_empty() {
                        None
                    } else {
                        Some(w.id.clone())
                    }
                })?;
                Some(SearchResult {
                    title: w.title.clone().unwrap_or_else(|| url.clone()),
                    url,
                    snippet: snippet(&w),
                    source: "openalex".into(),
                })
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn stub_server(body: &'static str) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        tokio::spawn(async move {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let mut buf = vec![0u8; 8192];
            let _ = sock.read(&mut buf).await;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = sock.write_all(resp.as_bytes()).await;
        });
        format!("http://{addr}")
    }

    const BODY: &str = r#"{"results":[{"id":"https://openalex.org/W1","doi":"https://doi.org/10.1/abc","title":"Attention Is All You Need","publication_year":2017,"cited_by_count":100000,"authorships":[{"author":{"display_name":"Vaswani"}},{"author":{"display_name":"Shazeer"}}]},{"id":"https://openalex.org/W2","title":"No DOI Paper","authorships":[]}]}"#;

    #[tokio::test]
    async fn parses_works() {
        let v: Resp = serde_json::from_str(BODY).expect("direct");
        assert_eq!(v.results.len(), 2);
        let base = stub_server(BODY).await;
        let src = OpenAlexSource::with_base(base);
        let out = src
            .search(&Query::new("transformer").expect("q"))
            .await
            .expect("search");
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].title, "Attention Is All You Need");
        assert_eq!(out[0].url, "https://doi.org/10.1/abc");
        assert!(out[0].snippet.contains("2017"));
        assert!(out[0].snippet.contains("cited by 100000"));
        assert_eq!(out[0].source, "openalex");
        assert_eq!(out[1].url, "https://openalex.org/W2");
    }

    #[tokio::test]
    async fn empty_results_means_empty() {
        let base = stub_server(r#"{"results":[]}"#).await;
        let src = OpenAlexSource::with_base(base);
        let out = src
            .search(&Query::new("xyzzy").expect("q"))
            .await
            .expect("search");
        assert!(out.is_empty());
    }
}
