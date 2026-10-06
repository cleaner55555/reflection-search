//! Prosti TTL keš odgovora pretrage (bez novih zavisnosti).
//!
//! Ključ: normalizovan upit + limit + kolone. Namenjeno čestim
//! ponovljenim upitima — svaki HIT štedi pozive ka plaćenim API-jima.

use std::{
    collections::{HashMap, VecDeque},
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

/// Podrazumevan TTL keša.
pub const DEFAULT_TTL: Duration = Duration::from_secs(600);
/// Najviše unosa — preko toga izbacuje najstarije (FIFO).
pub const MAX_ENTRIES: usize = 500;

/// Vrednost u kešu sa rokom.
struct Entry<T> {
    value: T,
    expires: Instant,
}

/// TTL keš sa FIFO evikcijom. Thread-safe preko async brave.
pub struct Cache<T: Clone> {
    inner: Mutex<CacheInner<T>>,
    ttl: Duration,
    max_entries: usize,
}

struct CacheInner<T> {
    map: HashMap<String, Entry<T>>,
    order: VecDeque<String>,
}

impl<T: Clone> Cache<T> {
    /// Pravi keš sa TTL-om i kapom.
    pub fn new(ttl: Duration, max_entries: usize) -> Self {
        Self {
            inner: Mutex::new(CacheInner {
                map: HashMap::new(),
                order: VecDeque::new(),
            }),
            ttl,
            max_entries,
        }
    }

    /// Ključ od delova upita.
    #[must_use]
    pub fn key(query: &str, limit: usize, columns: Option<&str>) -> String {
        format!(
            "{}|{limit}|{}",
            query.trim().to_lowercase(),
            columns.unwrap_or("-")
        )
    }

    /// Čita iz keša; isteklo se briše i vraća `None`.
    pub async fn get(&self, key: &str) -> Option<T> {
        let mut inner = self.inner.lock().await;
        let expired = inner
            .map
            .get(key)
            .is_some_and(|e| e.expires <= Instant::now());
        if expired {
            inner.map.remove(key);
            return None;
        }
        inner.map.get(key).map(|e| e.value.clone())
    }

    /// Upisuje u keš; preko kape izbacuje najstarije.
    pub async fn put(&self, key: String, value: T) {
        let mut inner = self.inner.lock().await;
        if inner.map.len() >= self.max_entries && !inner.map.contains_key(&key) {
            if let Some(old) = inner.order.pop_front() {
                inner.map.remove(&old);
            }
        }
        if !inner.map.contains_key(&key) {
            inner.order.push_back(key.clone());
        }
        inner.map.insert(
            key,
            Entry {
                value,
                expires: Instant::now() + self.ttl,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn hit_after_put() {
        let c: Cache<String> = Cache::new(Duration::from_secs(60), 10);
        assert!(c.get("k").await.is_none());
        c.put("k".into(), "v".into()).await;
        assert_eq!(c.get("k").await.as_deref(), Some("v"));
    }

    #[tokio::test]
    async fn expired_entry_is_miss() {
        let c: Cache<String> = Cache::new(Duration::from_millis(5), 10);
        c.put("k".into(), "v".into()).await;
        tokio::time::sleep(Duration::from_millis(15)).await;
        assert!(c.get("k").await.is_none());
    }

    #[tokio::test]
    async fn evicts_oldest_over_cap() {
        let c: Cache<String> = Cache::new(Duration::from_secs(60), 2);
        c.put("a".into(), "1".into()).await;
        c.put("b".into(), "2".into()).await;
        c.put("c".into(), "3".into()).await;
        assert!(c.get("a").await.is_none());
        assert_eq!(c.get("c").await.as_deref(), Some("3"));
    }

    #[test]
    fn key_normalizes_case_and_space() {
        assert_eq!(
            Cache::<String>::key("  Rust ", 10, None),
            Cache::<String>::key("rust", 10, None)
        );
        assert_ne!(
            Cache::<String>::key("rust", 10, None),
            Cache::<String>::key("rust", 10, Some("wiki"))
        );
    }
}
