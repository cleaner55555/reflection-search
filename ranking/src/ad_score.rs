//! Ad-score heuristika: procena ad/tracker gustine po URL-u (0–100).
//!
//! Viši skor = prljaviji domen. Koristi se za penal u rankingu (korak 2.2).
//! Prag isključenja je u configu servera; ovde je samo skor.

/// Skor iznad kog domen važi za prljav.
pub const DIRTY_THRESHOLD: u8 = 50;

/// Domeni poznati po agresivnim reklamama/trackers (smanjuju poverenje).
const DIRTY_DOMAINS: &[&str] = &[
    "dailymail.co.uk",
    "thesun.co.uk",
    "forbes.com",
    "businessinsider.com",
    "buzzfeed.com",
    "tmz.com",
    "eonline.com",
    "radaronline.com",
    "nationalenquirer.com",
    "theblaze.com",
];

/// Domeni poznati kao čisti (nekomercijalni, bez trackera).
const CLEAN_DOMAINS: &[&str] = &[
    "wikipedia.org",
    "arxiv.org",
    "github.com",
    "stackoverflow.com",
    "developer.mozilla.org",
    "docs.rs",
    "w3.org",
    "ietf.org",
];

/// Ad/tracking query parametri — svaki dodaje poene.
const TRACKING_PARAMS: &[&str] = &[
    "gclid",
    "fbclid",
    "msclkid",
    "utm_source",
    "utm_medium",
    "utm_campaign",
    "mc_cid",
    "mc_eid",
    "igshid",
    "yclid",
];

/// Izdvaja host iz URL-a (mala slova, bez porta).
fn host_of(url: &str) -> String {
    let after_scheme = url.split("://").nth(1).unwrap_or(url);
    let host_port = after_scheme.split('/').next().unwrap_or(after_scheme);
    host_port
        .split(':')
        .next()
        .unwrap_or(host_port)
        .trim()
        .to_lowercase()
}

/// Vraća ad-score 0–100 za URL.
#[must_use]
pub fn ad_score(url: &str) -> u8 {
    let host = host_of(url);
    if CLEAN_DOMAINS
        .iter()
        .any(|d| host == *d || host.ends_with(&format!(".{d}")))
    {
        return 0;
    }
    let mut score: u8 = 10;
    if DIRTY_DOMAINS
        .iter()
        .any(|d| host == *d || host.ends_with(&format!(".{d}")))
    {
        score = score.saturating_add(60);
    }
    let lower = url.to_lowercase();
    let params = TRACKING_PARAMS
        .iter()
        .filter(|p| lower.contains(*p))
        .count();
    score.saturating_add((params.min(3) * 10) as u8).min(100)
}

/// Da li URL prelazi prag prljavštine.
#[must_use]
pub fn is_dirty(url: &str) -> bool {
    ad_score(url) >= DIRTY_THRESHOLD
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_matrix() {
        let cases = [
            ("https://en.wikipedia.org/wiki/Rust", 0u8, false),
            ("https://arxiv.org/abs/2306.00107", 0, false),
            ("https://github.com/rust-lang/rust", 0, false),
            ("https://docs.rs/axum/latest", 0, false),
            ("https://developer.mozilla.org/en-US/", 0, false),
            ("https://stackoverflow.com/q/123", 0, false),
            ("https://example.com/article", 10, false),
            ("https://example.com/a?utm_source=x", 20, false),
            ("https://example.com/a?utm_source=x&fbclid=y", 30, false),
            (
                "https://example.com/a?utm_source=x&fbclid=y&gclid=z",
                40,
                false,
            ),
            (
                "https://shop.example.com/a?utm_source=x&fbclid=y&gclid=z&msclkid=w",
                40,
                false,
            ),
            ("https://www.dailymail.co.uk/news/x", 70, true),
            ("https://www.thesun.co.uk/news/x", 70, true),
            ("https://www.forbes.com/sites/x", 70, true),
            ("https://www.businessinsider.com/x", 70, true),
            ("https://www.buzzfeed.com/x", 70, true),
            ("https://www.tmz.com/x", 70, true),
            ("https://shop.example.com/?gclid=a", 20, false),
            ("https://sub.docs.rs/crate", 0, false),
            ("https://en.m.wikipedia.org/wiki/X", 0, false),
        ];
        let mut hits = 0;
        for (url, score, dirty) in cases {
            let got_score = ad_score(url);
            let got_dirty = is_dirty(url);
            if got_score == score && got_dirty == dirty {
                hits += 1;
            } else {
                eprintln!("MISS {url}: got ({got_score},{got_dirty}) want ({score},{dirty})");
            }
        }
        assert!(
            hits * 100 / cases.len() >= 90,
            "samo {hits}/{} pogodaka",
            cases.len()
        );
    }
}
