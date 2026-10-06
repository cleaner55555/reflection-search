//! AI sloj (plaćena nadogradnja): sažeci rezultata + Q&A asistent.
//!
//! Bez `AI_API_KEY` klijent ne postoji — server vraća 503, search radi normalno.
//! Model je OpenAI-kompatibilan (`POST {base}/v1/chat/completions`).
//!
//! ## Trošak (AC 5.1)
//! Svaki odgovor nosi `cost_usd` izmeren iz `usage` odgovora API-ja.
//! Kad `usage` fali, tokeni se procenjuju kao `chars/4` (konzervativno).
//! Cene su podrazumevane za jeftin model i služe kao procena — proveriti
//! aktuelni cenovnik provajdera pre naplate; override preko env
//! `LLM_PRICE_IN_PER_M` / `LLM_PRICE_OUT_PER_M` (USD na 1M tokena).

use search_core::SearchResult;
use serde::{Deserialize, Serialize};

/// Podrazumevana baza OpenAI-kompatibilnog API-ja.
pub const DEFAULT_BASE_URL: &str = "https://api.openai.com";
/// Podrazumevani jeftin model za sažetke.
pub const DEFAULT_MODEL: &str = "gpt-4o-mini";
/// Timeout po pozivu.
pub const REQUEST_TIMEOUT_SECS: u64 = 30;
/// Pauza pre jedinog retry-ja (samo transportne greške).
pub const RETRY_BACKOFF_MS: u64 = 500;
/// Najviše ulaznih znakova konteksta po pozivu (štiti trošak).
pub const MAX_CONTEXT_CHARS: usize = 6000;

/// Podrazumevana cena ulaza ($/1M tokena) — procena, proveriti cenovnik.
pub const DEFAULT_PRICE_IN_PER_M: f64 = 0.15;
/// Podrazumevana cena izlaza ($/1M tokena) — procena, proveriti cenovnik.
pub const DEFAULT_PRICE_OUT_PER_M: f64 = 0.60;

/// Greške AI sloja — mapiraju se u HTTP statuse na API granici.
#[derive(Debug, thiserror::Error)]
pub enum AiError {
    /// Transport/timeout (posle retry-ja).
    #[error("ai: request failed")]
    Request,
    /// Provajder vratio ne-uspešan status.
    #[error("ai: bad status")]
    Status,
    /// Nevalidan JSON odgovor.
    #[error("ai: bad response")]
    Parse,
}

/// Odgovor sa cenom — trošak meren, ne nagađan.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AiAnswer {
    /// Tekst sažetka/odgovora.
    pub text: String,
    /// Model koji je odgovorio.
    pub model: String,
    /// Cena poziva u USD (iz `usage` ili procene).
    pub cost_usd: f64,
    /// Ulazni tokeni (izmereni ili procenjeni).
    pub input_tokens: u32,
    /// Izlazni tokeni (izmereni ili procenjeni).
    pub output_tokens: u32,
}

/// Keširana AI vrednost (server je kešira po upitu — isti upit = 0 troška).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CachedAi {
    /// Tekst sažetka/odgovora.
    pub text: String,
    /// Model koji je odgovorio.
    pub model: String,
    /// Cena originalnog poziva u USD.
    pub cost_usd: f64,
}

/// Klijent ka OpenAI-kompatibilnom chat API-ju.
pub struct AiClient {
    client: reqwest::Client,
    api_key: String,
    base_url: String,
    model: String,
    price_in_per_m: f64,
    price_out_per_m: f64,
}

impl AiClient {
    /// Pravi klijent iz env (`AI_API_KEY` obavezan; ostalo ima default).
    /// `None` znači ugašen AI sloj.
    #[must_use]
    pub fn from_env() -> Option<Self> {
        let api_key = std::env::var("AI_API_KEY")
            .ok()
            .filter(|k| !k.trim().is_empty())?;
        Some(Self::new(
            api_key,
            std::env::var("AI_BASE_URL").unwrap_or_else(|_| DEFAULT_BASE_URL.to_string()),
            std::env::var("AI_MODEL").unwrap_or_else(|_| DEFAULT_MODEL.to_string()),
        ))
    }

    /// Pravi klijent sa eksplicitnim parametrima (testovi koriste stub).
    #[must_use]
    pub fn new(
        api_key: impl Into<String>,
        base_url: impl Into<String>,
        model: impl Into<String>,
    ) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key: api_key.into(),
            base_url: base_url.into(),
            model: model.into(),
            price_in_per_m: env_price("LLM_PRICE_IN_PER_M", DEFAULT_PRICE_IN_PER_M),
            price_out_per_m: env_price("LLM_PRICE_OUT_PER_M", DEFAULT_PRICE_OUT_PER_M),
        }
    }

    /// Ime modela (za odgovore i logove).
    #[must_use]
    pub fn model(&self) -> &str {
        &self.model
    }

    /// Sažima rezultate pretrage za upit (5.1).
    pub async fn summarize(
        &self,
        query: &str,
        results: &[SearchResult],
    ) -> Result<AiAnswer, AiError> {
        let context = build_context(results);
        let prompt = format!(
            "Summarize these search results for the query \"{query}\" in 3-5 sentences. Be factual, no markdown headers.\n\n{context}"
        );
        self.complete(prompt).await
    }

    /// Odgovara na pitanje nad rezultatima (5.2).
    pub async fn ask(
        &self,
        question: &str,
        query: &str,
        results: &[SearchResult],
    ) -> Result<AiAnswer, AiError> {
        let context = build_context(results);
        let prompt = format!(
            "Question: {question}\nAnswer using only these search results for \"{query}\". If the results do not contain the answer, say so in one sentence.\n\n{context}"
        );
        self.complete(prompt).await
    }

    async fn complete(&self, prompt: String) -> Result<AiAnswer, AiError> {
        let in_tokens = approx_tokens(&prompt);
        let body = serde_json::json!({
            "model": self.model,
            "temperature": 0.2,
            "max_tokens": 512,
            "messages": [{"role": "user", "content": prompt}],
        });
        let res = match self.post_once(&body).await {
            Ok(res) => Ok(res),
            Err(_) => {
                tokio::time::sleep(std::time::Duration::from_millis(RETRY_BACKOFF_MS)).await;
                self.post_once(&body).await
            }
        };
        let res = res?;
        if !res.status().is_success() {
            return Err(AiError::Status);
        }
        let parsed: ChatResponse = res.json().await.map_err(|_| AiError::Parse)?;
        let text = parsed
            .choices
            .first()
            .map(|c| c.message.content.clone())
            .filter(|t| !t.trim().is_empty())
            .ok_or(AiError::Parse)?;
        let (input_tokens, output_tokens) = match parsed.usage {
            Some(u) => (u.prompt_tokens, u.completion_tokens),
            None => (in_tokens, approx_tokens(&text)),
        };
        let cost_usd = estimate_cost_usd(
            self.price_in_per_m,
            self.price_out_per_m,
            input_tokens,
            output_tokens,
        );
        Ok(AiAnswer {
            text,
            model: self.model.clone(),
            cost_usd,
            input_tokens,
            output_tokens,
        })
    }

    async fn post_once(&self, body: &serde_json::Value) -> Result<reqwest::Response, AiError> {
        tokio::time::sleep(std::time::Duration::from_millis(0)).await;
        tokio::time::timeout(
            std::time::Duration::from_secs(REQUEST_TIMEOUT_SECS),
            self.client
                .post(format!("{}/v1/chat/completions", self.base_url))
                .header("Authorization", format!("Bearer {}", self.api_key))
                .json(body)
                .send(),
        )
        .await
        .map_err(|_| AiError::Request)?
        .map_err(|_| AiError::Request)
    }
}

/// Cena u USD iz tokena i cenovnika (po 1M).
#[must_use]
pub fn estimate_cost_usd(
    price_in_per_m: f64,
    price_out_per_m: f64,
    input_tokens: u32,
    output_tokens: u32,
) -> f64 {
    (f64::from(input_tokens) * price_in_per_m + f64::from(output_tokens) * price_out_per_m)
        / 1_000_000.0
}

/// Gruba procena tokena (~4 znaka po tokenu, min 1).
#[must_use]
pub fn approx_tokens(text: &str) -> u32 {
    (text.len().div_ceil(4)).max(1) as u32
}

/// Gradi kontekst od rezultata, sečen na kap radi troška.
fn build_context(results: &[SearchResult]) -> String {
    let mut out = String::new();
    for r in results.iter().take(10) {
        let line = format!("- {} ({}): {}\n", r.title, r.url, r.snippet);
        if out.len() + line.len() > MAX_CONTEXT_CHARS {
            break;
        }
        out.push_str(&line);
    }
    out
}

fn env_price(name: &str, fallback: f64) -> f64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|v: &f64| *v >= 0.0)
        .unwrap_or(fallback)
}

#[derive(Debug, serde::Deserialize)]
struct ChatResponse {
    #[serde(default)]
    choices: Vec<ChatChoice>,
    #[serde(default)]
    usage: Option<ChatUsage>,
}

#[derive(Debug, serde::Deserialize)]
struct ChatChoice {
    #[serde(default)]
    message: ChatMessage,
}

#[derive(Debug, Default, serde::Deserialize)]
struct ChatMessage {
    #[serde(default)]
    content: String,
}

#[derive(Debug, serde::Deserialize)]
struct ChatUsage {
    #[serde(default)]
    prompt_tokens: u32,
    #[serde(default)]
    completion_tokens: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn stub_server(body: &'static str, status: u16) -> String {
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
            let reason = if status == 200 { "OK" } else { "Error" };
            let resp = format!(
                "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = sock.write_all(resp.as_bytes()).await;
        });
        format!("http://{addr}")
    }

    const CHAT_JSON: &str = r#"{"choices":[{"message":{"content":"Rust is fast."}}],"usage":{"prompt_tokens":100,"completion_tokens":10}}"#;

    fn results() -> Vec<search_core::SearchResult> {
        vec![search_core::SearchResult {
            title: "t".into(),
            url: "https://x.com/".into(),
            snippet: "s".into(),
            source: "w".into(),
        }]
    }

    #[tokio::test]
    async fn summarize_returns_text_and_measured_cost() {
        let base = stub_server(CHAT_JSON, 200).await;
        let c = AiClient::new("key", base, "test-model");
        let q = search_core::Query::new("rust").expect("q");
        let a = c.summarize(&q.text, &results()).await.expect("summarize");
        assert_eq!(a.text, "Rust is fast.");
        assert_eq!(a.model, "test-model");
        assert_eq!(a.input_tokens, 100);
        assert_eq!(a.output_tokens, 10);
        let want = estimate_cost_usd(DEFAULT_PRICE_IN_PER_M, DEFAULT_PRICE_OUT_PER_M, 100, 10);
        assert!((a.cost_usd - want).abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn ask_uses_question_and_results() {
        let base = stub_server(CHAT_JSON, 200).await;
        let c = AiClient::new("key", base, "m");
        let q = search_core::Query::new("rust").expect("q");
        let a = c
            .ask("Is it fast?", &q.text, &results())
            .await
            .expect("ask");
        assert_eq!(a.text, "Rust is fast.");
    }

    #[tokio::test]
    async fn bad_status_is_error() {
        let base = stub_server("{}", 500).await;
        let c = AiClient::new("key", base, "m");
        let q = search_core::Query::new("rust").expect("q");
        assert!(c.summarize(&q.text, &results()).await.is_err());
    }

    #[test]
    fn cost_math_is_exact() {
        assert!((estimate_cost_usd(0.15, 0.60, 1_000_000, 0) - 0.15).abs() < 1e-9);
        assert!((estimate_cost_usd(0.15, 0.60, 0, 1_000_000) - 0.60).abs() < 1e-9);
    }

    #[test]
    fn approx_tokens_min_one() {
        assert_eq!(approx_tokens(""), 1);
        assert_eq!(approx_tokens("abcd"), 1);
        assert_eq!(approx_tokens("abcde"), 2);
    }
}
