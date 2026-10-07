//! Brave Search izvor (`api.search.brave.com`, plaćeni API).
//!
//! Bez `BRAVE_API_KEY` izvor je ugašen (prazna lista + warning) —
//! ostali izvori rade normalno.

use async_trait::async_trait;
use search_core::{Query, SearchResult};

use super::{Source, SOURCE_TIMEOUT};

/// Podrazumevana baza Brave API-ja; testovi gađaju lokalni stub.
pub const DEFAULT_BASE_URL: &str = "https://api.search.brave.com";

/// Klijent ka Brave Search API-ju.
pub struct BraveSource {
    client: reqwest::Client,
    api_key: String,
    base_url: String,
}

impl BraveSource {
    /// Pravi izvor iz env (`BRAVE_API_KEY`); `None` znači ugašen izvor.
    #[must_use]
    pub fn from_env() -> Option<Self> {
        let api_key = std::env::var("BRAVE_API_KEY")
            .ok()
            .filter(|k| !k.trim().is_empty())?;
        Some(Self::new(api_key, DEFAULT_BASE_URL))
    }

    /// Pravi izvor sa eksplicitnom bazom (testovi koriste stub).
    #[must_use]
    pub fn new(api_key: impl Into<String>, base_url: impl Into<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key: api_key.into(),
            base_url: base_url.into(),
        }
    }

    fn url(&self, query: &Query) -> String {
        format!(
            "{}/res/v1/web/search?q={}&count=10",
            self.base_url,
            urlencode(&query.text)
        )
    }
}

#[derive(Debug, serde::Deserialize)]
struct BraveResponse {
    #[serde(default)]
    web: Option<BraveWeb>,
}

#[derive(Debug, serde::Deserialize)]
struct BraveWeb {
    #[serde(default)]
    results: Vec<BraveResult>,
}

#[derive(Debug, serde::Deserialize)]
struct BraveResult {
    #[serde(default)]
    title: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    description: String,
}

/// Minimalni percent-encode za query parametar (bez novih zavisnosti).
pub(crate) fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push_str("%20"),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[async_trait]
impl Source for BraveSource {
    fn name(&self) -> &'static str {
        "brave"
    }

    fn is_paid(&self) -> bool {
        true
    }

    async fn search(&self, query: &Query) -> anyhow::Result<Vec<SearchResult>> {
        let res = tokio::time::timeout(
            SOURCE_TIMEOUT,
            self.client
                .get(self.url(query))
                .header("Accept", "application/json")
                .header("X-Subscription-Token", &self.api_key)
                .send(),
        )
        .await
        .map_err(|_| anyhow::anyhow!("brave: timeout"))?
        .map_err(|e| anyhow::anyhow!("brave: request: {e}"))?;
        if !res.status().is_success() {
            return Err(anyhow::anyhow!("brave: status {}", res.status()));
        }
        let body: BraveResponse = res
            .json()
            .await
            .map_err(|e| anyhow::anyhow!("brave: parse: {e}"))?;
        Ok(body
            .web
            .unwrap_or(BraveWeb { results: vec![] })
            .results
            .into_iter()
            .filter(|r| !r.url.is_empty())
            .map(|r| SearchResult {
                title: r.title,
                url: r.url,
                snippet: r.description,
                source: "brave".into(),
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimalni HTTP stub: vrati fiksni JSON uz 200.
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
            let mut buf = vec![0u8; 4096];
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

    #[tokio::test]
    async fn parses_results_and_skips_empty_url() {
        let base = stub_server(
            r#"{"web":{"results":[
                {"title":"T1","url":"https://a.com/","description":"D1"},
                {"title":"T2","url":"","description":"D2"}
            ]}}"#,
        )
        .await;
        let src = BraveSource::new("key", base);
        let out = src
            .search(&Query::new("rust").expect("q"))
            .await
            .expect("search");
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].title, "T1");
        assert_eq!(out[0].url, "https://a.com/");
        assert_eq!(out[0].snippet, "D1");
        assert_eq!(out[0].source, "brave");
    }

    #[tokio::test]
    async fn empty_web_means_no_results() {
        let base = stub_server(r#"{"web":null}"#).await;
        let src = BraveSource::new("key", base);
        let out = src
            .search(&Query::new("rust").expect("q"))
            .await
            .expect("search");
        assert!(out.is_empty());
    }

    #[test]
    fn urlencode_leaves_safe_chars() {
        assert_eq!(urlencode("rust lang"), "rust%20lang");
        assert_eq!(urlencode("a-b_c.d~e"), "a-b_c.d~e");
    }
}
