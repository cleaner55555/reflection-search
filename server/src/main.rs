//! `searchd` — HTTP API pretraživača.
//!
//! Rute: `GET /` (frontend), `GET /health`, `GET /search?q=&limit=`.

mod ads;
mod cache;

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
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

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
    /// Sopstveni indeks kreće prazan; puni ga crawler proces (faza 3).
    #[must_use]
    pub fn from_env() -> Self {
        let mut sources: Vec<Arc<dyn Source>> = vec![
            Arc::new(WikipediaSource::new()),
            Arc::new(OpenAlexSource::new()),
        ];
        match OwnIndexSource::with_docs(&[]) {
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
const INDEX_HTML: &str = r#"<!doctype html>
<html lang="en" class="dark"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>Reflection Search</title>
<style>*{box-sizing:border-box}body{margin:0;background:#020617;color:#f1f5f9;font-family:system-ui,sans-serif;min-height:100vh}header{position:sticky;top:0;z-index:10;background:rgba(15,23,42,.95);border-bottom:1px solid #1e293b;padding:12px 16px;display:flex;flex-wrap:wrap;gap:8px;align-items:center}h1{font-size:18px;font-weight:700;margin:0 8px 0 0;white-space:nowrap}form#f{display:flex;gap:8px;flex:1;min-width:200px;max-width:640px}input,select{background:#1e293b;border:1px solid #334155;color:#f1f5f9;border-radius:6px;padding:7px 12px;outline:none}input:focus{border-color:#0284c7}button{background:#1e293b;border:1px solid #334155;color:#f1f5f9;border-radius:6px;padding:7px 12px;cursor:pointer}button:hover{background:#334155}button.primary{background:#0284c7;border-color:#0284c7;font-weight:600}button.primary:hover{background:#0ea5e9}#ad{padding:8px 16px 0}main{display:flex;gap:12px;overflow-x:auto;padding:16px;align-items:flex-start}main::-webkit-scrollbar,ul::-webkit-scrollbar{height:8px;width:6px}main::-webkit-scrollbar-thumb,ul::-webkit-scrollbar-thumb{background:#334155;border-radius:4px}.col{width:340px;min-width:340px;max-height:calc(100vh - 140px);background:#0f172a;border:1px solid #1e293b;border-radius:8px;display:flex;flex-direction:column;overflow:hidden}.colhead{padding:8px 12px;background:rgba(30,41,59,.7);border-bottom:1px solid #334155;display:flex;align-items:center;gap:8px;cursor:move}.colhead h2{font-size:15px;margin:0;flex:1}.grip,.x{color:#64748b;background:none;border:none;padding:0 4px}.x:hover{color:#f87171}.col ul{overflow-y:auto;padding:8px;margin:0;list-style:none;display:flex;flex-direction:column;gap:8px}.card{background:rgba(30,41,59,.6);border-radius:6px;padding:8px}.card:hover{background:#1e293b}.card a{color:#38bdf8;font-size:13px;font-weight:600;text-decoration:none}.card a:hover{text-decoration:underline}.card p{color:#94a3b8;font-size:12px;margin:4px 0}.card span{color:#475569;font-size:11px}.empty{color:#475569;font-size:13px;padding:0 4px}aside{border:1px dashed #475569;border-radius:6px;padding:8px;font-size:12px;color:#94a3b8}.dragging{opacity:.4}</style></head>
<body>
<header>
<h1>Reflection Search</h1>
<form id="f"><input id="q" style="flex:1" placeholder="pretraga..." autofocus><button class="primary">Traži</button></form>
<select id="src"><option value="all">all</option><option value="brave">brave</option><option value="wikipedia">wikipedia</option><option value="openalex">openalex</option><option value="own-index">own-index</option></select>
<button id="add">+ Kolona</button>
</header>
<div id="ad"></div>
<main id="cols"></main>
<script>
let cols=[];try{cols=JSON.parse(localStorage.getItem('rs-cols')||'["all"]')}catch(e){cols=['all']}
function save(){try{localStorage.setItem('rs-cols',JSON.stringify(cols))}catch(e){}}
function esc(s){return String(s==null?'':s).replace(/&/g,'&amp;').replace(/</g,'&lt;')}
function render(){
const m=document.getElementById('cols');m.innerHTML='';
cols.forEach((c,i)=>{
const d=document.createElement('section');
d.className='col';
d.draggable=true;d.dataset.i=i;
d.innerHTML='<div class="colhead"><span class="grip">++</span><h2>'+esc(c)+'</h2><button class="x" data-x="'+i+'">x</button></div><ul id="col-'+i+'"><li class="empty">-</li></ul>';
m.appendChild(d);
});
m.querySelectorAll('[data-x]').forEach(b=>{b.onclick=()=>{cols.splice(+b.dataset.x,1);if(!cols.length)cols=['all'];save();render();search()}});
m.querySelectorAll('section').forEach(s=>{
s.ondragstart=e=>{e.dataTransfer.setData('text/plain',s.dataset.i);s.classList.add('dragging')};
s.ondragend=()=>s.classList.remove('dragging');
s.ondragover=e=>e.preventDefault();
s.ondrop=e=>{e.preventDefault();const from=+e.dataTransfer.getData('text/plain');const to=+s.dataset.i;if(from===to)return;const mv=cols.splice(from,1);cols.splice(to,0,mv[0]);save();render();search()};
});
}
function item(x){return '<li class="card"><a href="'+esc(x.url)+'">'+esc(x.title)+'</a><p>'+esc(x.snippet||'')+'</p><span>['+esc(x.source)+']</span></li>'}
async function search(){
const qv=document.getElementById('q').value.trim();if(!qv)return;
const res=await fetch('/search?limit=10&q='+encodeURIComponent(qv)+'&columns='+encodeURIComponent(cols.join(',')));
const j=await res.json();
document.getElementById('ad').innerHTML=j.sponsored?'<aside>Sponsored: <a href="'+esc(j.sponsored.url)+'">'+esc(j.sponsored.title)+'</a></aside>':'';
cols.forEach((c,i)=>{
const ul=document.getElementById('col-'+i);if(!ul)return;
const rows=(j.columns&&j.columns[c])||[];
ul.innerHTML=rows.length?rows.map(item).join(''):'<li class="empty">nema rezultata</li>';
});
}
document.getElementById('f').onsubmit=e=>{e.preventDefault();search()};
document.getElementById('add').onclick=()=>{const v=document.getElementById('src').value;if(!cols.includes(v)){cols.push(v);save();render();search()}};
render();
</script>
</body></html>"#;

async fn index() -> Html<&'static str> {
    Html(INDEX_HTML)
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

/// Sklapa ruter — izdvojeno radi testiranja bez mreže.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/health", get(health))
        .route("/search", get(search))
        .route("/auth/register", post(register))
        .route("/auth/login", post(login))
        .route("/summarize", post(summarize))
        .route("/ask", post(ask))
        .route_layer(middleware::from_fn(rate_limit))
        .layer(Extension(state.limiter.clone()))
        .layer(middleware::map_response(security_headers))
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
