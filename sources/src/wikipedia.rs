//! Wikipedia OpenSearch izvor — besplatan, bez ključa.
//!
//! Stabilan javni API: vraća `[upit, [naslovi], [opisi], [urlovi]]`.

use async_trait::async_trait;
use search_core::{Query, SearchResult};

use super::{Source, SOURCE_TIMEOUT};

/// Podrazumevana baza; testovi gađaju lokalni stub.
pub const DEFAULT_BASE_URL: &str = "https://en.wikipedia.org";

/// Klijent ka Wikipedia OpenSearch API-ju.
pub struct WikipediaSource {
    client: reqwest::Client,
    base_url: String,
}

impl WikipediaSource {
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

impl Default for WikipediaSource {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Source for WikipediaSource {
    fn name(&self) -> &'static str {
        "wikipedia"
    }

    async fn search(&self, query: &Query) -> anyhow::Result<Vec<SearchResult>> {
        let url = format!(
            "{}/w/api.php?action=opensearch&search={}&limit=10&format=json",
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
        .map_err(|_| anyhow::anyhow!("wikipedia: timeout"))?
        .map_err(|e| anyhow::anyhow!("wikipedia: request: {e}"))?;
        if !res.status().is_success() {
            return Err(anyhow::anyhow!("wikipedia: status {}", res.status()));
        }
        let body: (String, Vec<String>, Vec<String>, Vec<String>) = res
            .json()
            .await
            .map_err(|e| anyhow::anyhow!("wikipedia: parse: {e}"))?;
        Ok(body
            .1
            .into_iter()
            .zip(body.2)
            .zip(body.3)
            .filter(|((_, _), url)| !url.is_empty())
            .map(|((title, snippet), url)| SearchResult {
                title,
                url,
                snippet,
                source: "wikipedia".into(),
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
    async fn parses_opensearch_tuple() {
        let base = stub_server(r#"["Rust",["Rust (lang)","Crab"],["desc1","desc2"],["https://a.com/1","https://a.com/2"]]"#).await;
        let src = WikipediaSource::with_base(base);
        let out = src
            .search(&Query::new("rust").expect("q"))
            .await
            .expect("search");
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].title, "Rust (lang)");
        assert_eq!(out[0].url, "https://a.com/1");
        assert_eq!(out[0].source, "wikipedia");
    }
}
