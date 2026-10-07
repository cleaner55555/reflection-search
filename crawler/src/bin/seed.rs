//! Seed: crawl seed URL-ova u Tantivy indeks na disku.
//!
//! Env: `SEED_URLS` (zarezom), `INDEX_DIR` (default `index-data`).

use crawler::{CrawlConfig, Crawler};
use index::{Doc, SearchIndex};

#[tokio::main]
async fn main() {
    let seeds: Vec<String> = std::env::var("SEED_URLS")
        .unwrap_or_default()
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let dir = std::env::var("INDEX_DIR").unwrap_or_else(|_| "index-data".to_string());
    if seeds.is_empty() {
        eprintln!("SEED_URLS not set (comma-separated URLs)");
        std::process::exit(1);
    }
    let mut crawler = match Crawler::new(CrawlConfig::default()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("crawler: {e}");
            std::process::exit(1);
        }
    };
    let pages = crawler.crawl(&seeds).await;
    let docs: Vec<Doc> = pages
        .iter()
        .filter(|p| !p.text.is_empty())
        .map(|p| Doc {
            title: p.title.clone(),
            body: p.text.clone(),
            url: p.url.clone(),
        })
        .collect();
    let mut idx = match SearchIndex::open_dir(std::path::Path::new(&dir)) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("index: {e}");
            std::process::exit(1);
        }
    };
    let n = docs.len();
    if let Err(e) = idx.add(&docs) {
        eprintln!("add: {e}");
        std::process::exit(1);
    }
    println!("indexed {n} docs into {dir}");
}
