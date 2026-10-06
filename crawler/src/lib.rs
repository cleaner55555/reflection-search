//! Pristojan crawler za small-web indeks.
//!
//! Pravila: poštuje `robots.txt` (naš UA + `*`), pauza između
//! zahteva po domenu, identifikujući User-Agent sa kontaktom,
//! samo `text/html`, kap veličine odgovora.

pub mod politeness;
pub mod robots;

use std::collections::{HashMap, HashSet, VecDeque};
use std::time::Duration;

/// Kontakt u User-Agentu — administratori sajtova moraju moći da nas nađu.
pub const CONTACT: &str = "contact: local-dev";
/// User-Agent crawlera.
pub const USER_AGENT: &str = "reflection-search-bot/0.1 (+local-dev; respects robots.txt)";

/// Maksimalna veličina HTML odgovora (5 MB).
pub const MAX_BODY_BYTES: usize = 5 * 1024 * 1024;
/// Pauza između dva zahteva ka istom domenu.
pub const DOMAIN_DELAY: Duration = Duration::from_secs(2);

/// Skinuta i očišćena strana.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    /// Kanonski URL strane.
    pub url: String,
    /// Sadržaj `<title>` (prazan ako ga nema).
    pub title: String,
    /// Vidljiv tekst strane (bez skripti/stilova/navigacije).
    pub text: String,
    /// Apsolutni linkovi ka HTML stranama istog ili drugog domena.
    pub links: Vec<String>,
}

/// Greške crawlera.
#[derive(Debug, thiserror::Error)]
pub enum CrawlError {
    /// Mrežna greška ili timeout.
    #[error("fetch: {0}")]
    Fetch(String),
    /// Nije HTML.
    #[error("not html: {0}")]
    NotHtml(String),
    /// Odgovor preko kape.
    #[error("body too large")]
    TooLarge,
    /// robots.txt zabranjuje.
    #[error("disallowed by robots.txt")]
    Disallowed,
}

/// Konfiguracija puzanja.
#[derive(Debug, Clone)]
pub struct CrawlConfig {
    /// Najviše strana ukupno.
    pub max_pages: usize,
    /// Najviše strana po domenu.
    pub max_per_domain: usize,
    /// Pauza između zahteva ka istom domenu.
    pub domain_delay: Duration,
}

impl Default for CrawlConfig {
    fn default() -> Self {
        Self {
            max_pages: 100,
            max_per_domain: 20,
            domain_delay: DOMAIN_DELAY,
        }
    }
}

/// Pristojan breadth-first crawler.
pub struct Crawler {
    client: reqwest::Client,
    config: CrawlConfig,
    robots_cache: HashMap<String, robots::Rules>,
    last_hit: HashMap<String, tokio::time::Instant>,
    per_domain: HashMap<String, usize>,
}

impl Crawler {
    /// Pravi crawler sa konfiguracijom.
    #[must_use]
    pub fn new(config: CrawlConfig) -> Self {
        let client = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(Duration::from_secs(15))
            .build()
            .expect("client");
        Self {
            client,
            config,
            robots_cache: HashMap::new(),
            last_hit: HashMap::new(),
            per_domain: HashMap::new(),
        }
    }

    /// Puzi od semena; vraća skinute strane.
    pub async fn crawl(&mut self, seeds: &[String]) -> Vec<Page> {
        let mut queue: VecDeque<String> = seeds.iter().cloned().collect();
        let mut seen: HashSet<String> = queue.iter().cloned().collect();
        let mut pages = Vec::new();
        while let Some(url) = queue.pop_front() {
            if pages.len() >= self.config.max_pages {
                break;
            }
            let Ok(parsed) = url::Url::parse(&url) else {
                continue;
            };
            if parsed.scheme() != "http" && parsed.scheme() != "https" {
                continue;
            }
            let domain = host_port(&parsed);
            let count = self.per_domain.get(&domain).copied().unwrap_or(0);
            if count >= self.config.max_per_domain {
                continue;
            }
            if !self.allowed(&parsed).await {
                continue;
            }
            self.polite_wait(&domain).await;
            match self.fetch(&url).await {
                Ok(page) => {
                    self.per_domain.insert(domain, count + 1);
                    for link in &page.links {
                        if seen.insert(link.clone()) {
                            queue.push_back(link.clone());
                        }
                    }
                    pages.push(page);
                }
                Err(e) => tracing::debug!(%url, error = %e, "fetch skipped"),
            }
        }
        pages
    }

    /// Proverava robots.txt (keširano po host:port).
    async fn allowed(&mut self, url: &url::Url) -> bool {
        let domain = host_port(url);
        if !self.robots_cache.contains_key(&domain) {
            let rules = self.load_robots(url).await;
            self.robots_cache.insert(domain.clone(), rules);
        }
        let rules = self.robots_cache.get(&domain).expect("cached");
        rules.allows(url.path())
    }

    /// Skida robots.txt; neuspeh znači "sve dozvoljeno" (standard).
    async fn load_robots(&self, url: &url::Url) -> robots::Rules {
        let host = url.host_str().unwrap_or("");
        let robots_url = match url.port() {
            Some(port) => format!("{}://{host}:{port}/robots.txt", url.scheme()),
            None => format!("{}://{host}/robots.txt", url.scheme()),
        };
        let Ok(res) = self.client.get(robots_url).send().await else {
            return robots::Rules::allow_all();
        };
        if !res.status().is_success() {
            return robots::Rules::allow_all();
        }
        let Ok(text) = res.text().await else {
            return robots::Rules::allow_all();
        };
        robots::Rules::parse(&text)
    }

    /// Pauza između zahteva ka istom domenu.
    async fn polite_wait(&mut self, domain: &str) {
        if let Some(last) = self.last_hit.get(domain) {
            let elapsed = last.elapsed();
            if elapsed < self.config.domain_delay {
                tokio::time::sleep(self.config.domain_delay - elapsed).await;
            }
        }
        self.last_hit
            .insert(domain.to_string(), tokio::time::Instant::now());
    }

    /// Skida i čisti jednu stranu.
    async fn fetch(&self, url: &str) -> Result<Page, CrawlError> {
        let res = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|e| CrawlError::Fetch(e.to_string()))?;
        let ctype = res
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        if !ctype.contains("text/html") && !ctype.is_empty() {
            return Err(CrawlError::NotHtml(ctype));
        }
        let bytes = res
            .bytes()
            .await
            .map_err(|e| CrawlError::Fetch(e.to_string()))?;
        if bytes.len() > MAX_BODY_BYTES {
            return Err(CrawlError::TooLarge);
        }
        let html = String::from_utf8_lossy(&bytes);
        Ok(extract(url, &html))
    }
}

fn host_port(url: &url::Url) -> String {
    match (url.host_str().unwrap_or(""), url.port()) {
        (host, Some(port)) => format!("{host}:{port}"),
        (host, None) => host.to_string(),
    }
}

/// Vadi naslov, tekst i linkove iz HTML-a.
fn extract(url: &str, html: &str) -> Page {
    let doc = scraper::Html::parse_document(html);
    let title = doc
        .select(&scraper::Selector::parse("title").expect("selector"))
        .next()
        .map(|t| t.text().collect::<String>().trim().to_string())
        .unwrap_or_default();
    let skip = ["script", "style", "nav", "header", "footer"];
    let mut text = String::new();
    let body_sel = scraper::Selector::parse("body").expect("selector");
    if let Some(body) = doc.select(&body_sel).next() {
        for node in body.descendants() {
            let Some(t) = node.value().as_text() else {
                continue;
            };
            let inside_skip = node.ancestors().any(|a| {
                a.value()
                    .as_element()
                    .is_some_and(|el| skip.contains(&el.name()))
            });
            if inside_skip {
                continue;
            }
            let s = t.trim();
            if !s.is_empty() {
                if !text.is_empty() {
                    text.push(' ');
                }
                text.push_str(s);
            }
        }
    }
    let base = url::Url::parse(url).ok();
    let mut links = Vec::new();
    let a_sel = scraper::Selector::parse("a[href]").expect("selector");
    for a in doc.select(&a_sel) {
        if let Some(href) = a.value().attr("href") {
            let abs = match &base {
                Some(b) => b.join(href).map(|u| u.to_string()).unwrap_or_default(),
                None => href.to_string(),
            };
            if (abs.starts_with("http://") || abs.starts_with("https://")) && !links.contains(&abs)
            {
                links.push(abs);
            }
        }
    }
    Page {
        url: url.to_string(),
        title,
        text,
        links,
    }
}
