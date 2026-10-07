//! `searchd` — HTTP API pretraživača.
//!
//! Rute: `GET /` (frontend), `GET /health`, `GET /search?q=&limit=`.

mod ads;
mod billing;
mod cache;
mod pages;

use axum::{
    extract::{ConnectInfo, Query as AxumQuery, State},
    http::{HeaderMap, HeaderValue, StatusCode},
    middleware::{self, Next},
    response::{Html, IntoResponse, Json, Response},
    routing::{get, post},
    Extension, Router,
};
use search_core::{Query, SearchResult};
use serde::{Deserialize, Serialize};
use sources::{BraveSource, OpenAlexSource, OwnIndexSource, Source, WikipediaSource};
use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tokio::sync::Mutex;
use tower_http::{
    request_id::{MakeRequestId, PropagateRequestIdLayer, RequestId, SetRequestIdLayer},
    trace::{DefaultOnResponse, TraceLayer},
};

/// Verzija servisa (iz workspace `Cargo.toml`).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Prozor rate limita.
pub const RATE_WINDOW: Duration = Duration::from_secs(60);
/// Dozvoljeno pretraga po IP u prozoru.
pub const RATE_LIMIT: u32 = 60;
/// Podrazumevan broj rezultata.
pub const DEFAULT_LIMIT: usize = 10;
/// Najviše rezultata po pozivu.
pub const MAX_LIMIT: usize = 50;

/// Deljeno stanje servera.
#[derive(Clone)]
pub struct AppState {
    sources: Vec<Arc<dyn Source>>,
    limiter: Limiter,
    cache: Arc<cache::Cache<CachedBody>>,
    /// `None` kad nema `JWT_SECRET` — auth rute vraćaju 503.
    store: Option<Arc<auth::UserStore>>,
    /// `None` kad nema `AI_API_KEY` — AI rute vraćaju 503.
    ai: Option<Arc<ai::AiClient>>,
    /// Keš AI odgovora po korisniku+upitu — isti upit = 0 troška.
    ai_cache: Arc<cache::Cache<ai::AiAnswer>>,
    /// Viđeni Polar webhook-idjevi (dedupe, Polar šalje duplikate).
    billing_seen: Arc<cache::Cache<bool>>,
}

/// Keširano telo odgovora (bez headera).
#[derive(Debug, Clone, Serialize)]
pub struct CachedBody {
    results: Vec<SearchResult>,
    total: usize,
    template: Vec<String>,
    columns: Option<HashMap<String, Vec<SearchResult>>>,
    sponsored: Option<ads::SponsoredAd>,
}

impl AppState {
    /// Pravi stanje: Wikipedia + OpenAlex uvek, Brave samo uz `BRAVE_API_KEY`.
    /// Sopstveni indeks: sa diska uz `INDEX_DIR`, inače kreće prazan.
    #[must_use]
    pub fn from_env() -> Self {
        let mut sources: Vec<Arc<dyn Source>> = vec![
            Arc::new(WikipediaSource::new()),
            Arc::new(OpenAlexSource::new()),
        ];
        let own = match std::env::var("INDEX_DIR") {
            Ok(dir) => OwnIndexSource::open_dir(std::path::Path::new(&dir)),
            Err(_) => OwnIndexSource::with_docs(&[]),
        };
        match own {
            Ok(own) => sources.push(Arc::new(own)),
            Err(e) => tracing::error!(error = %e, "own-index disabled"),
        }
        if let Some(brave) = BraveSource::from_env() {
            tracing::info!("brave source enabled");
            sources.insert(0, Arc::new(brave));
        } else {
            tracing::warn!("BRAVE_API_KEY missing — brave source disabled");
        }
        Self {
            sources,
            limiter: Limiter::new(RATE_WINDOW, RATE_LIMIT),
            cache: Arc::new(cache::Cache::new(cache::DEFAULT_TTL, cache::MAX_ENTRIES)),
            store: Self::open_store(),
            ai: Self::open_ai(),
            ai_cache: Arc::new(cache::Cache::new(cache::DEFAULT_TTL, cache::MAX_ENTRIES)),
            billing_seen: Arc::new(cache::Cache::new(
                Duration::from_secs(3600),
                cache::MAX_ENTRIES,
            )),
        }
    }

    /// Otvara AI klijent iz env (`AI_API_KEY` + `AI_BASE_URL` + `AI_MODEL`).
    /// Bez ključa AI je ugašen — server i dalje radi bez sažetaka.
    fn open_ai() -> Option<Arc<ai::AiClient>> {
        match ai::AiClient::from_env() {
            Some(client) => {
                tracing::info!(model = client.model(), "ai enabled");
                Some(Arc::new(client))
            }
            None => {
                tracing::warn!("AI_API_KEY missing — ai routes disabled");
                None
            }
        }
    }

    /// Otvara user prodavnicu iz env (`JWT_SECRET` + `USERS_DB_PATH`).
    /// Bez ispravne tajne auth je ugašen — server i dalje radi anonimno.
    fn open_store() -> Option<Arc<auth::UserStore>> {
        let secret = std::env::var("JWT_SECRET").ok()?;
        let path = std::env::var("USERS_DB_PATH").unwrap_or_else(|_| "users.db".to_string());
        match auth::UserStore::open(&path, secret.as_bytes()) {
            Ok(store) => {
                tracing::info!("auth enabled");
                Some(Arc::new(store))
            }
            Err(e) => {
                tracing::error!(error = %e, "auth disabled");
                None
            }
        }
    }
}

/// Fiksni prozor rate limita po IP adresi.
#[derive(Debug, Clone)]
pub struct Limiter {
    inner: Arc<Mutex<HashMap<IpAddr, (Instant, u32)>>>,
    window: Duration,
    limit: u32,
}

impl Limiter {
    /// Pravi limiter sa prozorom i limitom.
    #[must_use]
    pub fn new(window: Duration, limit: u32) -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            window,
            limit,
        }
    }

    /// Proverava da li IP sme da prođe; broji pokušaj samo ako sme.
    pub async fn check(&self, ip: IpAddr) -> bool {
        let mut map = self.inner.lock().await;
        let now = Instant::now();
        let allowed = match map.get(&ip) {
            Some((start, count)) if now.duration_since(*start) < self.window => *count < self.limit,
            _ => true,
        };
        if allowed {
            let (start, count) = match map.get(&ip).copied() {
                Some((start, count)) if now.duration_since(start) < self.window => (start, count),
                _ => (now, 0),
            };
            map.insert(ip, (start, count + 1));
        }
        allowed
    }
}

/// Rate-limit middleware — preko limita vraća 429 bez diranja izvora.
/// Bez `ConnectInfo` (testovi) propušta dalje bez brojanja.
async fn rate_limit(
    connect: Option<ConnectInfo<SocketAddr>>,
    Extension(limiter): Extension<Limiter>,
    request: axum::http::Request<axum::body::Body>,
    next: Next,
) -> Response {
    let pass = match connect {
        Some(ConnectInfo(addr)) => limiter.check(addr.ip()).await,
        None => true,
    };
    if pass {
        next.run(request).await
    } else {
        (
            StatusCode::TOO_MANY_REQUESTS,
            "rate limit: 60 pretraga po minutu",
        )
            .into_response()
    }
}

/// Odgovor health provere.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Health {
    /// Uvek `"ok"` dok servis živi.
    pub status: &'static str,
    /// Verzija binarnika.
    pub version: &'static str,
}

async fn health() -> Json<Health> {
    Json(Health {
        status: "ok",
        version: VERSION,
    })
}

/// Parametri pretrage.
#[derive(Debug, Deserialize)]
pub struct SearchParams {
    /// Tekst upita (obavezan, ne sme prazan).
    pub q: String,
    /// Broj rezultata (default 10, max 50).
    pub limit: Option<usize>,
    /// Kolone (pro režim): imena izvora odvojena zarezom, max 4.
    /// Bez parametra vraća klasičnu jednu listu.
    pub columns: Option<String>,
}

/// Odgovor pretrage.
#[derive(Debug, Clone, Serialize)]
pub struct SearchResponse {
    /// Rezultati posle merge/dedupe/rankinga (prazno uz kolone).
    pub results: Vec<SearchResult>,
    /// Ukupno vraćenih (posle limita, bez kolona).
    pub total: usize,
    /// Predloženi šablon kolona za detektovani intent (imena izvora).
    pub template: Vec<String>,
    /// Kolone po imenu izvora (samo uz `columns` parametar).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub columns: Option<HashMap<String, Vec<SearchResult>>>,
    /// Sponzorisani slot (nikad deo organskog poretka; null bez inventara).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sponsored: Option<ads::SponsoredAd>,
}

/// Najviše kolona po pozivu.
pub const MAX_COLUMNS: usize = 4;

/// Predloženi šablon kolona za intent (samo postojeći izvori).
fn template_for(intent: ranking::Intent, sources: &[String]) -> Vec<String> {
    let want: &[&str] = match intent {
        ranking::Intent::News => &["all", "wikipedia"],
        ranking::Intent::Shopping => &["all"],
        ranking::Intent::Tech => &["all", "wikipedia"],
        ranking::Intent::Academic => &["all", "openalex"],
        ranking::Intent::General => &["all"],
    };
    let mut out: Vec<String> = want
        .iter()
        .filter(|w| **w == "all" || sources.iter().any(|s| s == *w))
        .map(|w| (*w).to_string())
        .collect();
    if out.is_empty() {
        out.push("all".to_string());
    }
    out
}

/// Vrsta greške → HTTP status mapiranje (bez internih detalja klijentu).
fn bad_request(msg: &str) -> (StatusCode, String) {
    (StatusCode::BAD_REQUEST, msg.to_string())
}

/// Bearer token iz `Authorization` headera (bez "Bearer " prefiksa).
fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers
        .get("authorization")?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
}

/// Kvota ulogovanog korisnika: validan token troši 1 dnevno.
/// Bez tokena prolazi anonimno (IP rate-limit i dalje važi).
/// Potrošena kvota ili tehnička greška skladišta → 429/500.
fn check_quota(state: &AppState, headers: &HeaderMap) -> Result<(), (StatusCode, String)> {
    let Some(token) = bearer(headers) else {
        return Ok(());
    };
    let Some(store) = &state.store else {
        return Err((StatusCode::SERVICE_UNAVAILABLE, "auth disabled".to_string()));
    };
    let user = store
        .verify(token)
        .map_err(|_| (StatusCode::UNAUTHORIZED, "invalid token".to_string()))?;
    match store.spend(&user) {
        Ok(true) => Ok(()),
        Ok(false) => Err((
            StatusCode::TOO_MANY_REQUESTS,
            "daily quota exceeded".to_string(),
        )),
        Err(_) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            "quota check failed".to_string(),
        )),
    }
}

/// Telo registracije/logina.
#[derive(Debug, Deserialize)]
pub struct Credentials {
    /// Email korisnika.
    pub email: String,
    /// Lozinka (min 10 znakova).
    pub password: String,
}

/// Odgovor sa tokenom.
#[derive(Debug, Serialize)]
pub struct TokenResponse {
    /// HS256 JWT za `Authorization: Bearer`.
    pub token: String,
}

fn auth_error(e: auth::AuthError) -> (StatusCode, String) {
    use auth::AuthError as E;
    match e {
        E::BadCredentials => (StatusCode::UNAUTHORIZED, e.to_string()),
        E::Taken => (StatusCode::CONFLICT, e.to_string()),
        E::BadInput => (StatusCode::BAD_REQUEST, e.to_string()),
        E::Quota => (StatusCode::TOO_MANY_REQUESTS, e.to_string()),
        E::NoCredits => (StatusCode::PAYMENT_REQUIRED, e.to_string()),
        E::BadToken => (StatusCode::UNAUTHORIZED, e.to_string()),
        E::Storage => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal error".to_string(),
        ),
    }
}

fn store_or_503(state: &AppState) -> Result<Arc<auth::UserStore>, (StatusCode, String)> {
    state
        .store
        .clone()
        .ok_or((StatusCode::SERVICE_UNAVAILABLE, "auth disabled".to_string()))
}

async fn register(
    State(state): State<AppState>,
    Json(creds): Json<Credentials>,
) -> Result<StatusCode, (StatusCode, String)> {
    let store = store_or_503(&state)?;
    store
        .register(&creds.email, &creds.password)
        .map_err(auth_error)?;
    Ok(StatusCode::CREATED)
}

async fn login(
    State(state): State<AppState>,
    Json(creds): Json<Credentials>,
) -> Result<Json<TokenResponse>, (StatusCode, String)> {
    let store = store_or_503(&state)?;
    let token = store
        .login(&creds.email, &creds.password)
        .map_err(auth_error)?;
    Ok(Json(TokenResponse { token }))
}

/// Polar webhook: `order.paid` → uplata kredita po emailu kupca.
/// Bez `POLAR_WEBHOOK_SECRET` ruta je ugašena (503). Uvek 200 posle
/// uspešne verifikacije (Polar traži brz odgovor; duplikati se ignorišu).
async fn billing_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Result<StatusCode, (StatusCode, String)> {
    let secret = std::env::var("POLAR_WEBHOOK_SECRET")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .ok_or((
            StatusCode::SERVICE_UNAVAILABLE,
            "billing disabled".to_string(),
        ))?;
    let header = |name: &str| {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string()
    };
    let event_type = billing::verify(
        &secret,
        &header("webhook-id"),
        &header("webhook-timestamp"),
        &header("webhook-signature"),
        &body,
    )
    .map_err(|_| (StatusCode::FORBIDDEN, "bad signature".to_string()))?;
    let id = header("webhook-id");
    if state.billing_seen.get(&id).await.is_some() {
        return Ok(StatusCode::OK);
    }
    state.billing_seen.put(id, true).await;
    if event_type == "order.paid" {
        let event: serde_json::Value =
            serde_json::from_slice(&body).map_err(|_| bad_request("bad payload"))?;
        if let (Some(email), Some(credits)) = (
            billing::customer_email(&event),
            billing::credits_for(&event),
        ) {
            if let Some(store) = &state.store {
                match store.user_id_by_email(&email) {
                    Ok(uid) => {
                        let balance = store.grant_credits(&uid, credits).unwrap_or(f64::NAN);
                        tracing::info!(%email, credits, balance, "credits granted");
                    }
                    Err(_) => tracing::warn!(%email, "grant skipped"),
                }
            }
        }
    }
    Ok(StatusCode::OK)
}

/// Telo sažetka (5.1).
#[derive(Debug, Deserialize)]
pub struct SummarizeBody {
    /// Upit čije se rezultate sažimaju.
    pub query: String,
}

/// Telo pitanja nad rezultatima (5.2).
#[derive(Debug, Deserialize)]
pub struct AskBody {
    /// Upit čiji se rezultati koriste kao kontekst.
    pub query: String,
    /// Pitanje korisnika.
    pub question: String,
}

/// AI ruta zahteva prijavu (plaćeni sloj): bez tokena 401,
/// bez `AI_API_KEY` 503. Ne troši search kvotu — AI se naplaćuje odvojeno.
async fn ai_guard(state: &AppState, headers: &HeaderMap) -> Result<String, (StatusCode, String)> {
    let Some(token) = bearer(headers) else {
        return Err((
            StatusCode::UNAUTHORIZED,
            "ai is a paid layer — login required".to_string(),
        ));
    };
    let store = store_or_503(state)?;
    store
        .verify(token)
        .map_err(|_| (StatusCode::UNAUTHORIZED, "invalid token".to_string()))
}

fn ai_client_or_503(state: &AppState) -> Result<Arc<ai::AiClient>, (StatusCode, String)> {
    state
        .ai
        .clone()
        .ok_or((StatusCode::SERVICE_UNAVAILABLE, "ai disabled".to_string()))
}

/// Prepaid gate: korisnik mora imati bar procenjeni trošak pre LLM poziva.
/// Keš pogodak ne troši ništa i ne proverava stanje.
fn charge_gate(
    state: &AppState,
    user: &str,
    input_chars: usize,
    client: &ai::AiClient,
) -> Result<(), (StatusCode, String)> {
    let store = store_or_503(state)?;
    let balance = store
        .credits(user)
        .map_err(|_| (StatusCode::SERVICE_UNAVAILABLE, "auth disabled".to_string()))?;
    if balance < client.quote(input_chars) {
        return Err((
            StatusCode::PAYMENT_REQUIRED,
            "insufficient credits".to_string(),
        ));
    }
    Ok(())
}

/// Knjiži stvarni trošak posle uspešnog poziva; vraća novo stanje za log.
fn charge_spend(state: &AppState, user: &str, cost: f64) -> f64 {
    let Ok(store) = store_or_503(state) else {
        return f64::NAN;
    };
    store.spend_credits(user, cost).unwrap_or(f64::NAN)
}

/// Gruba veličina ulaza za procenu (upit + rezultati + šablon prompta).
fn input_chars(query: &str, question: &str, results: &[SearchResult]) -> usize {
    query.len()
        + question.len()
        + results
            .iter()
            .map(|r| r.title.len() + r.snippet.len() + r.url.len())
            .sum::<usize>()
        + 300
}

fn ai_response(answer: ai::AiAnswer, hit: bool) -> Response {
    let mut res = Json(answer).into_response();
    res.headers_mut().insert(
        "x-cache",
        if hit {
            HeaderValue::from_static("HIT")
        } else {
            HeaderValue::from_static("MISS")
        },
    );
    res
}

async fn summarize(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<SummarizeBody>,
) -> Result<Response, (StatusCode, String)> {
    let query = Query::new(&body.query).map_err(|_| bad_request("query must not be empty"))?;
    let user = ai_guard(&state, &headers).await?;
    let client = ai_client_or_503(&state)?;
    let key = format!("sum|{user}|{}", query.text.trim().to_lowercase());
    if let Some(hit) = state.ai_cache.get(&key).await {
        return Ok(ai_response(hit, true));
    }
    let results = ranking::aggregate(&state.sources, &query).await;
    charge_gate(
        &state,
        &user,
        input_chars(&query.text, "", &results),
        &client,
    )?;
    let answer = client
        .summarize(&query.text, &results)
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, e.to_string()))?;
    let balance = charge_spend(&state, &user, answer.cost_usd);
    tracing::info!(user = %user, cost_usd = answer.cost_usd, balance_usd = balance, model = %answer.model, "summarize billed");
    state.ai_cache.put(key, answer.clone()).await;
    Ok(ai_response(answer, false))
}

async fn ask(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<AskBody>,
) -> Result<Response, (StatusCode, String)> {
    let query = Query::new(&body.query).map_err(|_| bad_request("query must not be empty"))?;
    if body.question.trim().is_empty() {
        return Err(bad_request("question must not be empty"));
    }
    let user = ai_guard(&state, &headers).await?;
    let client = ai_client_or_503(&state)?;
    let key = format!(
        "ask|{user}|{}|{}",
        query.text.trim().to_lowercase(),
        body.question.trim().to_lowercase()
    );
    if let Some(hit) = state.ai_cache.get(&key).await {
        return Ok(ai_response(hit, true));
    }
    let results = ranking::aggregate(&state.sources, &query).await;
    charge_gate(
        &state,
        &user,
        input_chars(&query.text, &body.question, &results),
        &client,
    )?;
    let answer = client
        .ask(&body.question, &query.text, &results)
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, e.to_string()))?;
    let balance = charge_spend(&state, &user, answer.cost_usd);
    tracing::info!(user = %user, cost_usd = answer.cost_usd, balance_usd = balance, model = %answer.model, "ask billed");
    state.ai_cache.put(key, answer.clone()).await;
    Ok(ai_response(answer, false))
}

async fn search(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumQuery(params): AxumQuery<SearchParams>,
) -> Result<Response, (StatusCode, String)> {
    let query = Query::new(&params.q).map_err(|_| bad_request("q must not be empty"))?;
    check_quota(&state, &headers)?;
    let limit = params.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let key = cache::Cache::<CachedBody>::key(&query.text, limit, params.columns.as_deref());
    if let Some(hit) = state.cache.get(&key).await {
        return Ok(cached_response(hit, true));
    }
    let mut results = ranking::aggregate(&state.sources, &query).await;
    results.truncate(limit);
    let source_names: Vec<String> = {
        let mut names: Vec<String> = state.sources.iter().map(|s| s.name().to_string()).collect();
        names.sort();
        names.dedup();
        names
    };
    let template = template_for(ranking::detect(&query.text), &source_names);
    let body = match params.columns {
        Some(raw) => {
            let mut wanted: Vec<String> = raw
                .split(',')
                .map(|c| c.trim().to_lowercase())
                .filter(|c| !c.is_empty())
                .collect();
            wanted.truncate(MAX_COLUMNS);
            if wanted.is_empty() {
                return Err(bad_request("columns must not be empty"));
            }
            let mut map = HashMap::new();
            for name in wanted {
                if name == "all" {
                    map.insert(name, results.clone());
                } else if source_names.contains(&name) {
                    map.insert(
                        name.clone(),
                        results
                            .iter()
                            .filter(|r| r.source == name)
                            .cloned()
                            .collect(),
                    );
                } else {
                    return Err(bad_request("unknown column source"));
                }
            }
            CachedBody {
                results: Vec::new(),
                total: 0,
                template,
                columns: Some(map),
                sponsored: ads::get_ad(&query.text),
            }
        }
        None => {
            let total = results.len();
            CachedBody {
                results,
                total,
                template,
                columns: None,
                sponsored: ads::get_ad(&query.text),
            }
        }
    };
    state.cache.put(key, body.clone()).await;
    Ok(cached_response(body, false))
}

fn cached_response(body: CachedBody, hit: bool) -> Response {
    let mut res = Json(SearchResponse {
        results: body.results,
        total: body.total,
        template: body.template,
        columns: body.columns,
        sponsored: body.sponsored,
    })
    .into_response();
    res.headers_mut().insert(
        "x-cache",
        if hit {
            HeaderValue::from_static("HIT")
        } else {
            HeaderValue::from_static("MISS")
        },
    );
    res
}

/// Minimalan frontend: jedna lista, search box, fetch ka `/search`.
/// Naslovna: Kagi raspored (centar), default lista, kolone na dugme, AI u toku.
const INDEX_HTML: &str = r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>Reflection Search</title>
<meta name='theme-color' content='#020617'><style>html{color-scheme:dark}html[data-theme=light]{color-scheme:light}:focus-visible{outline:2px solid var(--acc);outline-offset:2px}button{touch-action:manipulation}@media (prefers-reduced-motion:reduce){.card{transition:none}}:root{--bg:#020617;--panel:#0f172a;--panel2:rgba(30,41,59,.7);--line:#1e293b;--line2:#334155;--txt:#f1f5f9;--mut:#94a3b8;--dim:#64748b;--link:#38bdf8;--acc:#0284c7;--acc-h:#0ea5e9}html[data-theme=light]{--bg:#faf9f7;--panel:#fff;--panel2:#f4f1ea;--line:#e8e4dc;--line2:#c9c4ba;--txt:#232946;--mut:#5b5b7a;--dim:#9a97ad;--link:#205fce;--acc:#205fce;--acc-h:#1a4fb0}*{box-sizing:border-box}body{margin:0;background:var(--bg);color:var(--txt);font-family:system-ui,sans-serif;min-height:100vh}#topbar{position:sticky;top:0;z-index:10;background:var(--bg);border-bottom:1px solid var(--line);padding:10px 16px;display:flex;gap:8px;align-items:center;flex-wrap:wrap}.brand{font-weight:700;font-size:17px;cursor:pointer;white-space:nowrap}form{display:flex;gap:8px}input,select{background:var(--panel);border:1px solid var(--line2);color:var(--txt);border-radius:8px;padding:9px 14px;outline:none;font-size:15px}input:focus{border-color:var(--acc)}button{background:var(--panel);border:1px solid var(--line2);color:var(--txt);border-radius:8px;padding:9px 14px;cursor:pointer;font-size:15px}button:hover{filter:brightness(1.15)}button.primary{background:var(--acc);border-color:var(--acc);color:#fff;font-weight:600}button.primary:hover{background:var(--acc-h)}#home{min-height:88vh;display:flex;align-items:center;justify-content:center}.hero{max-width:620px;width:100%;text-align:center;padding:0 20px}.logo{font-size:46px;margin:0 0 4px}.tag{color:var(--mut);font-size:17px;margin:0 0 8px}#fh{margin:26px 0 10px}#fh input{flex:1;font-size:17px;padding:13px 18px;border-radius:12px}.chips{margin:6px 0 18px;display:flex;gap:8px;justify-content:center;flex-wrap:wrap}.chips button{font-size:13px;padding:6px 12px;border-radius:20px}.aihome{display:flex;gap:8px;margin:0 0 26px}.aihome input{flex:1;font-size:14px}nav.home{display:flex;gap:18px;justify-content:center;flex-wrap:wrap}nav.home a{color:var(--mut);font-size:14px;text-decoration:none}nav.home a:hover{color:var(--txt)}.colbar{padding:10px 16px 0;display:flex;gap:8px;align-items:center;flex-wrap:wrap}#ad{padding:8px 16px 0}#aibox{margin:12px 16px 0;background:var(--panel);border:1px solid var(--line);border-radius:8px;padding:12px 14px;display:none;max-width:760px}#aibox.show{display:block}#aibox h2{font-size:15px;margin:0 0 6px}#aibox p{font-size:14px;margin:6px 0}#aibox .cost{color:var(--dim);font-size:12px}main{display:flex;gap:12px;overflow-x:auto;padding:16px;align-items:flex-start}main::-webkit-scrollbar,ul::-webkit-scrollbar{height:8px;width:6px}main::-webkit-scrollbar-thumb,ul::-webkit-scrollbar-thumb{background:var(--line2);border-radius:4px}.col{width:360px;min-width:360px;max-height:calc(100vh - 150px);background:var(--panel);border:1px solid var(--line);border-radius:10px;display:flex;flex-direction:column;overflow:hidden}.colhead{padding:10px 12px;background:var(--panel2);border-bottom:1px solid var(--line);display:flex;align-items:center;gap:10px;cursor:move}.colhead h2{font-size:15px;margin:0;flex:1}.badge{background:var(--panel);border:1px solid var(--line2);color:var(--mut);font-size:12px;border-radius:12px;padding:1px 9px}.grip{color:var(--dim);background:none;border:none;padding:6px 8px;font-size:16px;cursor:grab}.x{color:var(--dim);background:none;border:none;width:32px;height:32px;font-size:16px;border-radius:6px}.x:hover{color:#f87171;background:var(--panel)}.col ul{overflow-y:auto;padding:10px;margin:0;list-style:none;display:flex;flex-direction:column;gap:10px}#list{max-width:760px;margin:0 auto;padding:16px;list-style:none;display:flex;flex-direction:column;gap:12px}.card{background:var(--panel);border:1px solid var(--line);border-radius:8px;padding:12px 14px;transition:transform .12s ease}.card:hover{transform:translateY(-1px)}.card a{color:var(--link);font-size:15px;font-weight:600;text-decoration:none}.card a:hover{text-decoration:underline}.card p{color:var(--mut);font-size:13px;margin:6px 0}.card span{color:var(--dim);font-size:11px}.empty{color:var(--dim);font-size:13px;padding:0 4px}aside{border:1px dashed var(--line2);border-radius:6px;padding:8px;font-size:12px;color:var(--mut);margin:0 16px}.dragging{opacity:.4}</style></head>
<body>
<header id="topbar" style="display:none">
<span class="brand" id="home-link">Reflection Search</span>
<form id="f"><input id="q" aria-label="upit za pretragu" style="flex:1;min-width:140px" placeholder="pretraga..."><button class="primary">Traži</button><button id="aibtn" type="button" title="pitaj AI o upitu">Pitaj AI</button></form>
<button id="modebtn" aria-label="prebaci lista kolone">Kolone</button>
<button id="theme" title="svetla/tamna tema" aria-label="promeni temu">◐</button>
</header>
<section id="home">
<div class="hero">
<h1 class="logo">Reflection Search</h1>
<p class="tag">Free pretraga bez praćenja. Bez naloga.</p>
<form id="fh"><input id="hq" aria-label="upit za pretragu" placeholder="pretraži web..." autofocus><button class="primary">Traži</button><button id="haskbtn" type="button" class="ghost">Pitaj AI</button></form>
<div class="chips"><button data-q="rust">rust</button><button data-q="transformer paper">transformer paper</button><button data-q="breaking news today">vesti</button></div>
<p class="tag" style="font-size:13px">AI je prepaid — token sa <a href="/account">naloga</a>.</p>
<nav class="home"><a href="/welcome">O projektu</a><a href="/pricing">Cene</a><a href="/account">Nalog</a><a href="/docs">Dokumentacija</a></nav>
</div>
</section>
<section id="app" style="display:none">
<div id="ad"></div>
<div class="colbar" id="colbar" style="display:none"><select id="src" aria-label="izvor za novu kolonu"><option value="all">all</option><option value="brave">brave</option><option value="wikipedia">wikipedia</option><option value="openalex">openalex</option><option value="own-index">own-index</option></select><button id="add">+ Kolona</button><input id="tok" aria-label="AI token" style="max-width:220px" placeholder="token (sa /account)" type="password" autocomplete="off" spellcheck="false"></div>
<div id="aibox"><h2>AI odgovor</h2><p id="aitext"></p><p class="cost" id="aicost"></p></div>
<main id="cols"></main>
<ul id="list"></ul>
</section>
<script>
try{if(localStorage.getItem('rs-theme')==='light')document.documentElement.setAttribute('data-theme','light')}catch(e){}
let cols=[];try{cols=JSON.parse(localStorage.getItem('rs-cols')||'["all"]')}catch(e){cols=['all']}
let mode='list';try{mode=localStorage.getItem('rs-mode')||'list'}catch(e){}
function save(){try{localStorage.setItem('rs-cols',JSON.stringify(cols));localStorage.setItem('rs-mode',mode)}catch(e){}}
function esc(s){return String(s==null?'':s).replace(/&/g,'&amp;').replace(/</g,'&lt;')}
function dot(src){const m={all:'#38bdf8',brave:'#f59e0b',wikipedia:'#10b981',openalex:'#fb7185','own-index':'#64748b'};return '<span style="display:inline-block;width:8px;height:8px;border-radius:50%;background:'+(m[src]||'#64748b')+'"></span> '}
function render(){
const listMode=mode==='list';
document.getElementById('cols').style.display=listMode?'none':'flex';
document.getElementById('list').style.display=listMode?'flex':'none';
document.getElementById('colbar').style.display=listMode?'none':'flex';
document.getElementById('modebtn').textContent=listMode?'Kolone':'Lista';
if(listMode)return;
const m=document.getElementById('cols');m.innerHTML='';
cols.forEach((c,i)=>{
const d=document.createElement('section');
d.className='col';
d.draggable=true;d.dataset.i=i;
d.innerHTML='<div class="colhead" tabindex="0" data-kb="'+i+'"><span class="grip" aria-hidden="true">⠿</span><h2>'+dot(c)+esc(c)+' <span class="badge" id="cnt-'+i+'"></span></h2><button class="x" data-x="'+i+'" title="ukloni kolonu" aria-label="ukloni kolonu '+esc(c)+'">✕</button></div><ul id="col-'+i+'"><li class="empty">—</li></ul>';
m.appendChild(d);
});
m.querySelectorAll('[data-x]').forEach(b=>{b.onclick=()=>{cols.splice(+b.dataset.x,1);if(!cols.length)cols=['all'];save();render();search()}});
m.querySelectorAll('section').forEach(s=>{
s.ondragstart=e=>{e.dataTransfer.setData('text/plain',s.dataset.i);s.classList.add('dragging')};
s.ondragend=()=>s.classList.remove('dragging');
s.ondragover=e=>e.preventDefault();
s.ondrop=e=>{e.preventDefault();const from=+e.dataTransfer.getData('text/plain');const to=+s.dataset.i;if(from===to)return;const mv=cols.splice(from,1);cols.splice(to,0,mv[0]);save();render();search()};
});
m.querySelectorAll('[data-kb]').forEach(h=>{h.onkeydown=e=>{if(!e.altKey)return;const i=+h.dataset.kb;const j=i+((e.key==='ArrowRight')?1:(e.key==='ArrowLeft'?-1:0));if(e.key!=='ArrowRight'&&e.key!=='ArrowLeft')return;if(j<0||j>=cols.length)return;e.preventDefault();const mv=cols.splice(i,1);cols.splice(j,0,mv[0]);save();render();search()}});
}
function item(x){return '<li class="card"><a target="_blank" rel="noopener" href="'+esc(x.url)+'">'+esc(x.title)+'</a><p>'+esc(x.snippet||'')+'</p><span>['+esc(x.source)+']</span></li>'}
async function search(){
const qv=document.getElementById('q').value.trim();if(!qv)return;
showApp();
if(mode==='list'){
const res=await fetch('/search?limit=20&q='+encodeURIComponent(qv));
const j=await res.json();
showAd(j);
document.getElementById('list').innerHTML=(j.results||[]).map(item).join('')||'<li class="empty">nema rezultata</li>';
return;
}
const res=await fetch('/search?limit=10&q='+encodeURIComponent(qv)+'&columns='+encodeURIComponent(cols.join(',')));
const j=await res.json();
showAd(j);
cols.forEach((c,i)=>{
const ul=document.getElementById('col-'+i);if(!ul)return;
const rows=(j.columns&&j.columns[c])||[];
ul.innerHTML=rows.length?rows.map(item).join(''):'<li class="empty">nema rezultata</li>';
const b=document.getElementById('cnt-'+i);if(b)b.textContent=rows.length;
});
}
function showAd(j){document.getElementById('ad').innerHTML=j.sponsored?'<aside>Sponsored: <a target="_blank" rel="noopener" href="'+esc(j.sponsored.url)+'">'+esc(j.sponsored.title)+'</a></aside>':''}
function showApp(){document.getElementById('home').style.display='none';document.getElementById('app').style.display='block';document.getElementById('topbar').style.display='flex';const qv=document.getElementById('hq').value;if(qv)document.getElementById('q').value=qv;}
function goHome(){document.getElementById('app').style.display='none';document.getElementById('topbar').style.display='none';document.getElementById('home').style.display='flex';}
function token(){const t=document.getElementById('tok').value.trim();if(t){try{localStorage.setItem('rs-token',t)}catch(e){}return t}try{return localStorage.getItem('rs-token')||''}catch(e){return ''}}
async function aiCall(path,body){
const t=token();const box=document.getElementById('aibox');box.className='show';
if(!t){document.getElementById('aitext').textContent='Treba token sa /account (AI je prepaid).';document.getElementById('aicost').textContent='';return}
let res;try{res=await fetch(path,{method:'POST',headers:{'Content-Type':'application/json',Authorization:'Bearer '+t},body:JSON.stringify(body)})}catch(e){document.getElementById('aitext').textContent='Server nedostupan.';document.getElementById('aicost').textContent='';return}
if(res.status===402){document.getElementById('aitext').textContent='Nema kredita — dopuni na /pricing.';document.getElementById('aicost').textContent='';return}
if(!res.ok){document.getElementById('aitext').textContent='Greška: '+res.status;document.getElementById('aicost').textContent='';return}
const j=await res.json();document.getElementById('aitext').textContent=j.text||'';document.getElementById('aicost').textContent='model '+(j.model||'')+' · trošak $'+(Number(j.cost_usd)||0).toFixed(6);
}
function askFlow(){
const qv=(document.getElementById('hq').value||document.getElementById('q').value).trim();
if(!qv)return;
document.getElementById('q').value=qv;showApp();
aiCall('/ask',{query:qv,question:qv});
search();
}
document.getElementById('fh').onsubmit=e=>{e.preventDefault();document.getElementById('q').value=document.getElementById('hq').value;showApp();search()};
document.getElementById('f').onsubmit=e=>{e.preventDefault();search()};
document.getElementById('haskbtn').onclick=askFlow;
document.getElementById('aibtn').onclick=()=>{const qv=document.getElementById('q').value.trim();if(!qv)return;aiCall('/ask',{query:qv,question:qv});};
document.getElementById('home-link').onclick=goHome;
document.querySelectorAll('.chips button').forEach(b=>{b.onclick=()=>{document.getElementById('hq').value=b.dataset.q;document.getElementById('q').value=b.dataset.q;showApp();search()}});
document.getElementById('modebtn').onclick=()=>{mode=(mode==='list')?'cols':'list';save();render();const qv=document.getElementById('q').value.trim();if(qv)search()};
document.getElementById('add').onclick=()=>{const v=document.getElementById('src').value;if(!cols.includes(v)){cols.push(v);save();render();search()}};
try{const st=localStorage.getItem('rs-token');if(st)document.getElementById('tok').value=st}catch(e){}
document.getElementById('theme').onclick=()=>{const h=document.documentElement;const light=h.getAttribute('data-theme')==='light';if(light){h.removeAttribute('data-theme')}else{h.setAttribute('data-theme','light')}try{localStorage.setItem('rs-theme',light?'dark':'light')}catch(e){}};
render();
</script>
</body></html>"#;

/// Favicon iz Vite builda (root dist fajl, van `/assets`).
async fn favicon() -> impl IntoResponse {
    match std::fs::read("web/dist/favicon.svg") {
        Ok(body) => ([("content-type", "image/svg+xml")], body).into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn index() -> Html<String> {
    // Vite build kad postoji (pravi frontend), inače ugrađeni prototip.
    // Time CI bez node-a i dalje prolazi, a dist je u .gitignore.
    Html(std::fs::read_to_string("web/dist/index.html").unwrap_or_else(|_| INDEX_HTML.to_string()))
}

/// Stanje kredita ulogovanog korisnika (za account stranu).
async fn api_credits(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let Some(token) = bearer(&headers) else {
        return Err((StatusCode::UNAUTHORIZED, "login required".to_string()));
    };
    let store = store_or_503(&state)?;
    let user = store
        .verify(token)
        .map_err(|_| (StatusCode::UNAUTHORIZED, "invalid token".to_string()))?;
    let credits = store
        .credits(&user)
        .map_err(|_| (StatusCode::SERVICE_UNAVAILABLE, "auth disabled".to_string()))?;
    Ok(Json(serde_json::json!({"credits": credits})))
}

/// Sigurnosni headeri na svaki odgovor. CSP je labav za inline
/// script/style jer je frontend inline; stroži kad se izdvoji u fajl.
async fn security_headers(mut res: Response) -> Response {
    let h = res.headers_mut();
    h.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    h.insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    h.insert("x-frame-options", HeaderValue::from_static("DENY"));
    h.insert(
        "content-security-policy",
        HeaderValue::from_static(
            "default-src 'self'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; connect-src 'self'; img-src 'self' data:; object-src 'none'; base-uri 'self'",
        ),
    );
    h.insert(
        "strict-transport-security",
        HeaderValue::from_static("max-age=63072000; includeSubDomains"),
    );
    h.insert("cache-control", HeaderValue::from_static("no-store"));
    res
}

/// Brojač ID-jeva zahteva (po procesu, ne perzistira se — nije tracking).
#[derive(Clone, Default)]
struct RequestIdMaker {
    counter: Arc<AtomicUsize>,
}

impl MakeRequestId for RequestIdMaker {
    fn make_request_id<B>(&mut self, _req: &axum::http::Request<B>) -> Option<RequestId> {
        self.counter
            .fetch_add(1, Ordering::SeqCst)
            .to_string()
            .parse()
            .ok()
            .map(RequestId::new)
    }
}

/// Sklapa ruter — izdvojeno radi testiranja bez mreže.
pub fn router(state: AppState) -> Router {
    let request_id = axum::http::HeaderName::from_static("x-request-id");
    Router::new()
        .route("/", get(index))
        .route("/health", get(health))
        .route("/search", get(search))
        .route("/auth/register", post(register))
        .route("/auth/login", post(login))
        .route("/summarize", post(summarize))
        .route("/ask", post(ask))
        .route("/billing/webhook", post(billing_webhook))
        .route("/favicon.svg", get(favicon))
        .route("/api/credits", get(api_credits))
        .nest_service(
            "/assets",
            tower_http::services::ServeDir::new("web/dist/assets"),
        )
        .route("/welcome", get(|| async { Html(pages::welcome()) }))
        .route("/pricing", get(|| async { Html(pages::pricing()) }))
        .route("/account", get(|| async { Html(pages::account()) }))
        .route("/docs", get(|| async { Html(pages::docs()) }))
        .route_layer(middleware::from_fn(rate_limit))
        .layer(Extension(state.limiter.clone()))
        .layer(middleware::map_response(security_headers))
        .layer(PropagateRequestIdLayer::new(request_id.clone()))
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(|req: &axum::http::Request<axum::body::Body>| {
                    let id = req
                        .headers()
                        .get("x-request-id")
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or("-");
                    tracing::info_span!(
                        "http",
                        method = %req.method(),
                        path = req.uri().path(),
                        request_id = id,
                    )
                })
                .on_response(DefaultOnResponse::new().level(tracing::Level::INFO)),
        )
        .layer(SetRequestIdLayer::new(
            request_id,
            RequestIdMaker::default(),
        ))
        .with_state(state)
}

/// Test stanje — prazno (bez mrežnih izvora).
#[cfg(test)]
fn test_state() -> AppState {
    AppState {
        sources: vec![],
        limiter: Limiter::new(RATE_WINDOW, RATE_LIMIT),
        cache: Arc::new(cache::Cache::new(cache::DEFAULT_TTL, cache::MAX_ENTRIES)),
        store: None,
        ai: None,
        ai_cache: Arc::new(cache::Cache::new(cache::DEFAULT_TTL, cache::MAX_ENTRIES)),
        billing_seen: Arc::new(cache::Cache::new(
            Duration::from_secs(3600),
            cache::MAX_ENTRIES,
        )),
    }
}

/// Test stanje sa in-memory prodavnicom (auth testovi).
#[cfg(test)]
fn auth_state() -> AppState {
    let store =
        auth::UserStore::open(":memory:", b"test-secret-32-bytes-long-xxxxxx").expect("store");
    AppState {
        sources: vec![],
        limiter: Limiter::new(RATE_WINDOW, RATE_LIMIT),
        cache: Arc::new(cache::Cache::new(cache::DEFAULT_TTL, cache::MAX_ENTRIES)),
        store: Some(Arc::new(store)),
        ai: None,
        ai_cache: Arc::new(cache::Cache::new(cache::DEFAULT_TTL, cache::MAX_ENTRIES)),
        billing_seen: Arc::new(cache::Cache::new(
            Duration::from_secs(3600),
            cache::MAX_ENTRIES,
        )),
    }
}

/// Test stanje sa stub izvorima (kolone testovi).
#[cfg(test)]
fn stub_state() -> AppState {
    struct Stub(&'static str, Vec<SearchResult>);
    #[async_trait::async_trait]
    impl Source for Stub {
        fn name(&self) -> &'static str {
            self.0
        }
        async fn search(&self, _q: &Query) -> anyhow::Result<Vec<SearchResult>> {
            Ok(self.1.clone())
        }
    }
    fn item(source: &'static str, url: &str) -> SearchResult {
        SearchResult {
            title: "t".into(),
            url: url.into(),
            snippet: "s".into(),
            source: source.into(),
        }
    }
    AppState {
        sources: vec![
            Arc::new(Stub("brave", vec![item("brave", "https://b.com/1")])),
            Arc::new(Stub(
                "wikipedia",
                vec![item("wikipedia", "https://w.com/1")],
            )),
        ],
        limiter: Limiter::new(RATE_WINDOW, RATE_LIMIT),
        cache: Arc::new(cache::Cache::new(cache::DEFAULT_TTL, cache::MAX_ENTRIES)),
        store: None,
        ai: None,
        ai_cache: Arc::new(cache::Cache::new(cache::DEFAULT_TTL, cache::MAX_ENTRIES)),
        billing_seen: Arc::new(cache::Cache::new(
            Duration::from_secs(3600),
            cache::MAX_ENTRIES,
        )),
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().with_env_filter("info").init();
    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    tracing::info!(%addr, version = VERSION, "searchd starting");
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("bind 127.0.0.1:3000");
    axum::serve(
        listener,
        router(AppState::from_env()).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .expect("serve");
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower::ServiceExt;

    fn request(uri: &str) -> axum::http::Request<axum::body::Body> {
        axum::http::Request::builder()
            .uri(uri)
            .header("x-forwarded-for", "9.9.9.9")
            .body(axum::body::Body::empty())
            .expect("request")
    }

    #[tokio::test]
    async fn health_returns_ok_with_version() {
        let res = router(test_state())
            .oneshot(request("/health"))
            .await
            .expect("oneshot");
        assert_eq!(res.status(), StatusCode::OK);
        let body = axum::body::to_bytes(res.into_body(), 4096)
            .await
            .expect("body");
        let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(json["status"], "ok");
        assert_eq!(json["version"], VERSION);
    }

    #[tokio::test]
    async fn security_headers_present() {
        let res = router(test_state())
            .oneshot(request("/health"))
            .await
            .expect("oneshot");
        let h = res.headers();
        assert_eq!(h["x-content-type-options"], "nosniff");
        assert_eq!(h["referrer-policy"], "no-referrer");
        assert_eq!(h["x-frame-options"], "DENY");
        assert!(h.contains_key("content-security-policy"));
        assert!(h.contains_key("strict-transport-security"));
    }

    #[tokio::test]
    async fn frontend_has_no_external_requests() {
        let res = router(test_state())
            .oneshot(request("/"))
            .await
            .expect("oneshot");
        let body = axum::body::to_bytes(res.into_body(), 65536)
            .await
            .expect("body");
        let html = String::from_utf8_lossy(&body);
        for attr in ["src=\"http", "href=\"http", "url(http", "@import"] {
            assert!(!html.contains(attr), "eksterni zahtev u frontendu: {attr}");
        }
    }

    #[tokio::test]
    async fn request_id_propagated() {
        let res = router(test_state())
            .oneshot(request("/health"))
            .await
            .expect("oneshot");
        assert!(res.headers().contains_key("x-request-id"));
    }

    #[tokio::test]
    async fn pages_serve_html() {
        for uri in ["/welcome", "/pricing", "/account", "/docs"] {
            let res = router(test_state())
                .oneshot(request(uri))
                .await
                .expect("oneshot");
            assert_eq!(res.status(), StatusCode::OK, "{uri}");
            let headers = res.headers().clone();
            let body = axum::body::to_bytes(res.into_body(), 65536)
                .await
                .expect("body");
            assert!(headers["content-type"]
                .to_str()
                .expect("ct")
                .contains("text/html"));
            assert!(body.windows(7).any(|w| w == b"Reflect"), "{uri}");
        }
    }

    #[tokio::test]
    async fn api_credits_needs_auth_and_returns_balance() {
        let state = auth_state();
        let denied = router(state.clone())
            .oneshot(request("/api/credits"))
            .await
            .expect("oneshot");
        assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);
        let token = login_token(&state).await;
        let req = axum::http::Request::builder()
            .uri("/api/credits")
            .header("authorization", format!("Bearer {token}"))
            .body(axum::body::Body::empty())
            .expect("request");
        let res = router(state).oneshot(req).await.expect("oneshot");
        assert_eq!(res.status(), StatusCode::OK);
        let body = axum::body::to_bytes(res.into_body(), 4096)
            .await
            .expect("body");
        let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert!((json["credits"].as_f64().expect("credits") - 1.0).abs() < 1e-9);
    }

    #[tokio::test]
    async fn unknown_route_is_404() {
        let res = router(test_state())
            .oneshot(request("/nope"))
            .await
            .expect("oneshot");
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn empty_query_is_400() {
        let res = router(test_state())
            .oneshot(request("/search?q=%20"))
            .await
            .expect("oneshot");
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn search_without_sources_returns_empty_list() {
        let res = router(test_state())
            .oneshot(request("/search?q=rust"))
            .await
            .expect("oneshot");
        assert_eq!(res.status(), StatusCode::OK);
        let body = axum::body::to_bytes(res.into_body(), 4096)
            .await
            .expect("body");
        let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(json["total"], 0);
        assert_eq!(json["results"].as_array().expect("array").len(), 0);
    }

    #[tokio::test]
    async fn index_serves_html() {
        let res = router(test_state())
            .oneshot(request("/"))
            .await
            .expect("oneshot");
        assert_eq!(res.status(), StatusCode::OK);
        let headers = res.headers().clone();
        let body = axum::body::to_bytes(res.into_body(), 65536)
            .await
            .expect("body");
        assert!(headers["content-type"]
            .to_str()
            .expect("ct")
            .contains("text/html"));
        assert!(body.windows(7).any(|w| w == b"Reflect"));
    }

    #[tokio::test]
    async fn limiter_blocks_over_limit() {
        let limiter = Limiter::new(Duration::from_secs(60), 2);
        let ip: IpAddr = "1.2.3.4".parse().expect("ip");
        assert!(limiter.check(ip).await);
        assert!(limiter.check(ip).await);
        assert!(!limiter.check(ip).await);
    }

    #[tokio::test]
    async fn columns_split_by_source() {
        let res = router(stub_state())
            .oneshot(request("/search?q=rust&columns=brave,wikipedia"))
            .await
            .expect("oneshot");
        assert_eq!(res.status(), StatusCode::OK);
        let body = axum::body::to_bytes(res.into_body(), 16384)
            .await
            .expect("body");
        let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(json["columns"]["brave"].as_array().expect("brave").len(), 1);
        assert_eq!(
            json["columns"]["wikipedia"].as_array().expect("wiki").len(),
            1
        );
        assert_eq!(json["results"].as_array().expect("results").len(), 0);
    }

    #[tokio::test]
    async fn unknown_column_is_400() {
        let res = router(stub_state())
            .oneshot(request("/search?q=rust&columns=nope"))
            .await
            .expect("oneshot");
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn template_matches_news_intent() {
        let res = router(stub_state())
            .oneshot(request("/search?q=breaking%20news%20today"))
            .await
            .expect("oneshot");
        let body = axum::body::to_bytes(res.into_body(), 16384)
            .await
            .expect("body");
        let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
        let template = json["template"].as_array().expect("template");
        assert!(template.iter().any(|t| t == "all"));
        assert!(template.iter().any(|t| t == "wikipedia"));
    }

    #[tokio::test]
    async fn missing_q_is_400_or_422() {
        let res = router(test_state())
            .oneshot(request("/search"))
            .await
            .expect("oneshot");
        assert!(
            res.status() == StatusCode::BAD_REQUEST
                || res.status() == StatusCode::UNPROCESSABLE_ENTITY
        );
    }

    #[tokio::test]
    async fn cache_hit_within_same_app() {
        use std::sync::Arc as StdArc;
        let cache = StdArc::new(cache::Cache::new(cache::DEFAULT_TTL, cache::MAX_ENTRIES));
        let mk = || AppState {
            sources: vec![],
            limiter: Limiter::new(RATE_WINDOW, RATE_LIMIT),
            cache: StdArc::clone(&cache),
            store: None,
            ai: None,
            ai_cache: Arc::new(cache::Cache::new(cache::DEFAULT_TTL, cache::MAX_ENTRIES)),
            billing_seen: Arc::new(cache::Cache::new(
                Duration::from_secs(3600),
                cache::MAX_ENTRIES,
            )),
        };
        let r1 = router(mk())
            .oneshot(request("/search?q=rust"))
            .await
            .expect("oneshot");
        assert_eq!(r1.headers()["x-cache"], "MISS");
        let r2 = router(mk())
            .oneshot(request("/search?q=rust"))
            .await
            .expect("oneshot");
        assert_eq!(r2.headers()["x-cache"], "HIT");
    }

    #[tokio::test]
    async fn sponsored_absent_and_organic_intact() {
        let res = router(stub_state())
            .oneshot(request("/search?q=rust"))
            .await
            .expect("oneshot");
        let body = axum::body::to_bytes(res.into_body(), 16384)
            .await
            .expect("body");
        let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert!(
            json.get("sponsored").is_none(),
            "bez inventara nema slot polja"
        );
        assert_eq!(json["results"].as_array().expect("results").len(), 2);
    }

    fn post_json(uri: &str, body: serde_json::Value) -> axum::http::Request<axum::body::Body> {
        axum::http::Request::builder()
            .method("POST")
            .uri(uri)
            .header("content-type", "application/json")
            .body(axum::body::Body::from(body.to_string()))
            .expect("request")
    }

    fn creds(email: &str, password: &str) -> serde_json::Value {
        serde_json::json!({"email": email, "password": password})
    }

    #[tokio::test]
    async fn register_then_login_returns_token() {
        let state = auth_state();
        let reg = router(state.clone())
            .oneshot(post_json(
                "/auth/register",
                creds("u@x.com", "dovoljno-dugo-1"),
            ))
            .await
            .expect("oneshot");
        assert_eq!(reg.status(), StatusCode::CREATED);
        let login = router(state)
            .oneshot(post_json(
                "/auth/login",
                creds("u@x.com", "dovoljno-dugo-1"),
            ))
            .await
            .expect("oneshot");
        assert_eq!(login.status(), StatusCode::OK);
        let body = axum::body::to_bytes(login.into_body(), 4096)
            .await
            .expect("body");
        let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert!(json["token"].as_str().expect("token").len() > 20);
    }

    #[tokio::test]
    async fn duplicate_register_is_409_and_bad_login_401() {
        let state = auth_state();
        let first = router(state.clone())
            .oneshot(post_json(
                "/auth/register",
                creds("u@x.com", "dovoljno-dugo-1"),
            ))
            .await
            .expect("oneshot");
        assert_eq!(first.status(), StatusCode::CREATED);
        let dup = router(state)
            .oneshot(post_json(
                "/auth/register",
                creds("u@x.com", "dovoljno-dugo-1"),
            ))
            .await
            .expect("oneshot");
        assert_eq!(dup.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn bad_login_is_401() {
        let app = router(auth_state());
        let res = app
            .oneshot(post_json(
                "/auth/login",
                creds("nema@ga.com", "bilo-sta-dugacko-2"),
            ))
            .await
            .expect("oneshot");
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn auth_disabled_is_503() {
        let res = router(test_state())
            .oneshot(post_json(
                "/auth/register",
                creds("u@x.com", "dovoljno-dugo-1"),
            ))
            .await
            .expect("oneshot");
        assert_eq!(res.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    const CHAT_JSON: &str = r#"{"choices":[{"message":{"content":"Rust is fast."}}],"usage":{"prompt_tokens":100,"completion_tokens":10}}"#;

    #[cfg(test)]
    async fn stub_llm(body: &'static str) -> String {
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

    /// Stanje sa prodavnicom + stub LLM klijentom (AI testovi).
    #[cfg(test)]
    fn ai_state(llm_base: String) -> AppState {
        let store =
            auth::UserStore::open(":memory:", b"test-secret-32-bytes-long-xxxxxx").expect("store");
        AppState {
            sources: vec![],
            limiter: Limiter::new(RATE_WINDOW, RATE_LIMIT),
            cache: Arc::new(cache::Cache::new(cache::DEFAULT_TTL, cache::MAX_ENTRIES)),
            store: Some(Arc::new(store)),
            ai: Some(Arc::new(ai::AiClient::new("key", llm_base, "test-model"))),
            ai_cache: Arc::new(cache::Cache::new(cache::DEFAULT_TTL, cache::MAX_ENTRIES)),
            billing_seen: Arc::new(cache::Cache::new(
                Duration::from_secs(3600),
                cache::MAX_ENTRIES,
            )),
        }
    }

    #[cfg(test)]
    async fn login_token(state: &AppState) -> String {
        let app = router(state.clone());
        let _ = app
            .oneshot(post_json(
                "/auth/register",
                creds("u@x.com", "dovoljno-dugo-1"),
            ))
            .await
            .expect("register");
        let app = router(state.clone());
        let res = app
            .oneshot(post_json(
                "/auth/login",
                creds("u@x.com", "dovoljno-dugo-1"),
            ))
            .await
            .expect("login");
        let body = axum::body::to_bytes(res.into_body(), 4096)
            .await
            .expect("body");
        let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
        let token = json["token"].as_str().expect("token").to_string();
        // Test nalog dobija kredite (kao da je uplatio) — bez toga AI gate vraća 402.
        let store = state.store.clone().expect("store");
        let user = store.verify(&token).expect("verify");
        store.grant_credits(&user, 1.0).expect("grant");
        token
    }

    #[cfg(test)]
    fn authed_request(
        uri: &str,
        token: &str,
        body: serde_json::Value,
    ) -> axum::http::Request<axum::body::Body> {
        axum::http::Request::builder()
            .method("POST")
            .uri(uri)
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {token}"))
            .body(axum::body::Body::from(body.to_string()))
            .expect("request")
    }

    #[tokio::test]
    async fn ai_requires_auth_is_401() {
        let base = stub_llm(CHAT_JSON).await;
        let app = router(ai_state(base));
        let res = app
            .oneshot(post_json(
                "/summarize",
                serde_json::json!({"query": "rust"}),
            ))
            .await
            .expect("oneshot");
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn ai_disabled_is_503() {
        let state = auth_state();
        let token = login_token(&state).await;
        let app = router(state);
        let res = app
            .oneshot(authed_request(
                "/summarize",
                &token,
                serde_json::json!({"query": "rust"}),
            ))
            .await
            .expect("oneshot");
        assert_eq!(res.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn summarize_miss_then_hit() {
        let base = stub_llm(CHAT_JSON).await;
        let state = ai_state(base);
        let token = login_token(&state).await;
        let app = router(state.clone());
        let res = app
            .oneshot(authed_request(
                "/summarize",
                &token,
                serde_json::json!({"query": "rust"}),
            ))
            .await
            .expect("oneshot");
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(res.headers()["x-cache"], "MISS");
        let body = axum::body::to_bytes(res.into_body(), 16384)
            .await
            .expect("body");
        let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(json["text"], "Rust is fast.");
        assert!(json["cost_usd"].as_f64().expect("cost") > 0.0);
        let app = router(state);
        let res = app
            .oneshot(authed_request(
                "/summarize",
                &token,
                serde_json::json!({"query": "rust"}),
            ))
            .await
            .expect("oneshot");
        assert_eq!(res.headers()["x-cache"], "HIT");
    }

    #[tokio::test]
    async fn ask_happy_path() {
        let base = stub_llm(CHAT_JSON).await;
        let state = ai_state(base);
        let token = login_token(&state).await;
        let app = router(state);
        let res = app
            .oneshot(authed_request(
                "/ask",
                &token,
                serde_json::json!({"query": "rust", "question": "Is it fast?"}),
            ))
            .await
            .expect("oneshot");
        assert_eq!(res.status(), StatusCode::OK);
        let body = axum::body::to_bytes(res.into_body(), 16384)
            .await
            .expect("body");
        let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(json["text"], "Rust is fast.");
    }

    async fn bare_login_token(state: &AppState) -> (String, String) {
        let app = router(state.clone());
        let _ = app
            .oneshot(post_json(
                "/auth/register",
                creds("u@x.com", "dovoljno-dugo-1"),
            ))
            .await
            .expect("register");
        let app = router(state.clone());
        let res = app
            .oneshot(post_json(
                "/auth/login",
                creds("u@x.com", "dovoljno-dugo-1"),
            ))
            .await
            .expect("login");
        let body = axum::body::to_bytes(res.into_body(), 4096)
            .await
            .expect("body");
        let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
        let token = json["token"].as_str().expect("token").to_string();
        let store = state.store.clone().expect("store");
        let user = store.verify(&token).expect("verify");
        (token, user)
    }

    #[cfg(test)]
    fn signed_webhook(
        secret: &str,
        id: &str,
        ts: &str,
        body: &str,
    ) -> axum::http::Request<axum::body::Body> {
        use base64::Engine as _;
        let signed = format!("{id}.{ts}.{body}");
        let mut mac = hmac::Hmac::<sha2::Sha256>::new_from_slice(secret.as_bytes()).expect("mac");
        use hmac::Mac as _;
        mac.update(signed.as_bytes());
        let sig = base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes());
        axum::http::Request::builder()
            .method("POST")
            .uri("/billing/webhook")
            .header("content-type", "application/json")
            .header("webhook-id", id)
            .header("webhook-timestamp", ts)
            .header("webhook-signature", format!("v1,{sig}"))
            .body(axum::body::Body::from(body.to_string()))
            .expect("request")
    }

    #[cfg(test)]
    fn webhook_now() -> String {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time")
            .as_secs()
            .to_string()
    }

    const PAID_BODY: &str = r#"{"type":"order.paid","data":{"amount":500,"metadata":{"credits_usd":5.0},"customer":{"email":"u@x.com"}}}"#;

    #[tokio::test]
    async fn billing_bad_signature_is_403() {
        std::env::set_var("POLAR_WEBHOOK_SECRET", "whsec_test");
        let state = auth_state();
        let app = router(state);
        let res = app
            .oneshot(signed_webhook(
                "whsec_other",
                "msg_1",
                &webhook_now(),
                PAID_BODY,
            ))
            .await
            .expect("oneshot");
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn billing_order_paid_grants_credits_once() {
        std::env::set_var("POLAR_WEBHOOK_SECRET", "whsec_test");
        let state = auth_state();
        let (token, user) = bare_login_token(&state).await;
        let _ = token;
        let store = state.store.clone().expect("store");
        assert!((store.credits(&user).expect("credits") - 0.0).abs() < f64::EPSILON);
        let ts = webhook_now();
        let app = router(state.clone());
        let res = app
            .oneshot(signed_webhook("whsec_test", "msg_2", &ts, PAID_BODY))
            .await
            .expect("oneshot");
        assert_eq!(res.status(), StatusCode::OK);
        assert!((store.credits(&user).expect("credits") - 5.0).abs() < 1e-9);
        let app = router(state);
        let res = app
            .oneshot(signed_webhook("whsec_test", "msg_2", &ts, PAID_BODY))
            .await
            .expect("oneshot");
        assert_eq!(res.status(), StatusCode::OK);
        assert!((store.credits(&user).expect("credits") - 5.0).abs() < 1e-9);
    }

    #[tokio::test]
    async fn ai_no_credits_is_402() {
        let base = stub_llm(CHAT_JSON).await;
        let state = ai_state(base);
        let (token, _) = bare_login_token(&state).await;
        let app = router(state);
        let res = app
            .oneshot(authed_request(
                "/summarize",
                &token,
                serde_json::json!({"query": "rust"}),
            ))
            .await
            .expect("oneshot");
        assert_eq!(res.status(), StatusCode::PAYMENT_REQUIRED);
    }

    #[tokio::test]
    async fn ai_spends_credits_once_then_hit_is_free() {
        let base = stub_llm(CHAT_JSON).await;
        let state = ai_state(base);
        let token = login_token(&state).await;
        let store = state.store.clone().expect("store");
        let user = store.verify(&token).expect("verify");
        let before = store.credits(&user).expect("credits");
        let app = router(state.clone());
        let res = app
            .oneshot(authed_request(
                "/summarize",
                &token,
                serde_json::json!({"query": "rust"}),
            ))
            .await
            .expect("oneshot");
        assert_eq!(res.status(), StatusCode::OK);
        let body = axum::body::to_bytes(res.into_body(), 16384)
            .await
            .expect("body");
        let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
        let cost = json["cost_usd"].as_f64().expect("cost");
        assert!(cost > 0.0);
        let after = store.credits(&user).expect("credits");
        assert!((before - after - cost).abs() < 1e-9);
        let app = router(state);
        let res = app
            .oneshot(authed_request(
                "/summarize",
                &token,
                serde_json::json!({"query": "rust"}),
            ))
            .await
            .expect("oneshot");
        assert_eq!(res.headers()["x-cache"], "HIT");
        let same = store.credits(&user).expect("credits");
        assert!((same - after).abs() < 1e-12);
    }
}
