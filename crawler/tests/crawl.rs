//! Integracioni test crawlera nad lokalnim stub serverom (100 strana).

use crawler::{CrawlConfig, Crawler};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Stub: `/robots.txt` dozvoljava sve osim `/secret/`;
/// `/p{i}` vraća stranu sa linkovima ka sledećima + script đubre.
async fn stub() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr").to_string();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                let Ok(n) = sock.read(&mut buf).await else {
                    return;
                };
                let req = String::from_utf8_lossy(&buf[..n]);
                let path = req.split_whitespace().nth(1).unwrap_or("/").to_string();
                let body = if path == "/robots.txt" {
                    "User-agent: *\nDisallow: /secret/\n".to_string()
                } else if let Some(num) = path.strip_prefix("/p") {
                    let i: usize = num.parse().unwrap_or(0);
                    let next: Vec<String> = ((i + 1)..=(i + 3))
                        .map(|j| format!("<a href=\"/p{j}\">n{j}</a>"))
                        .collect();
                    format!(
                        "<html><head><title>Page {i}</title><script>var junk=1;</script></head>\
                         <body><h1>Heading {i}</h1><p>Body text {i}</p>{}</body></html>",
                        next.join("")
                    )
                } else {
                    "<html><head><title>Root</title></head><body><a href=\"/p0\">start</a></body></html>"
                        .to_string()
                };
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = sock.write_all(resp.as_bytes()).await;
            });
        }
    });
    format!("http://{addr}")
}

#[tokio::test]
async fn crawls_100_pages_respecting_robots() {
    let base = stub().await;
    let mut crawler = Crawler::new(CrawlConfig {
        max_pages: 100,
        max_per_domain: 1000,
        domain_delay: Duration::from_millis(0),
    });
    let pages = crawler
        .crawl(&[format!("{base}/"), format!("{base}/secret/x")])
        .await;
    assert_eq!(
        pages.len(),
        100,
        "očekivano 100 strana, dobijeno {}",
        pages.len()
    );
    let first = pages.iter().find(|p| p.url.ends_with("/p0")).expect("p0");
    assert_eq!(first.title, "Page 0");
    assert!(first.text.contains("Heading 0") && first.text.contains("Body text 0"));
    assert!(
        !first.text.contains("junk"),
        "script sadržaj mora biti izbačen"
    );
    assert!(
        pages.iter().all(|p| !p.url.contains("/secret/")),
        "robots zabrana se poštuje"
    );
}
