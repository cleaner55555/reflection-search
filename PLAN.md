# Reflection Search (radni naziv) — PROFESIONALNI PLAN

> Status: `[-]` u toku, `[x]` gotovo, `[ ]` na čekanju. Ažurira se posle svakog koraka.
> Standard kvaliteta: **kod za 10** (vidi §8), **coverage ≥ 80%** pre merge-a svake faze (§9).

## 1. Vizija i koncept
Besplatan meta-pretraživač, engleski prvo, ostali jezici po potražnji. Podrazumevano jedna lista rezultata kao svi ostali; kolone (TweetDeck stil, 1–4, intent šabloni) kao pro režim. Prihod: nenametljive kontekstualne reklame (strogo odvojene i označene, bez praćenja) + kasnije naplata AI sloja. Trošak starta ~0 (free API kvote + jeftin VPS).

## 2. Diferencijacija (zašto ne još jedan Kagi-klon)
1. **Free zauvek** — Kagi $10/mes, nema trajni free plan.
2. **Commerce vertikala** — žive cene, istorija cena, kuponi, provera prodavca. Niko ne poseduje poštenu kupovinu (Google Shopping = oglasi, Kagi slab).
3. **Ad-score filter** — domeni sa ad/tracker gustinom preko praga padaju/ispadaju iz rezultata. Efekat blokera bez blokiranja (sajt ne može da blokira, ali može da ne prikazuje).
4. **Kolone + intent šabloni** — vesti/kupovina/tech/opšte; korisnik slaže, čuva raspored.
5. **AI opt-in i odvojeno** — Kagi ga uračunava u cenu i nameće; ovde doplata.
6. **Obrnute reklame (opciono, faza 4)** — deo prihoda ide izdavačima koje korisnik klikne. Marketing koji se sam širi.

## 3. Arhitektura
```
┌──────────┐   ┌─────────────────────────────────────────┐
│ Frontend │──▶│ searchd (Axum, jedan binarnik)           │
└──────────┘   │  /search → agregator → ranking → filter │
               │  izvori: Brave*, Marginalia, Wikipedia, │
               │  svoj Tantivy indeks, keš (Moka/Redis)  │
               └─────────────────────────────────────────┘
```
- Jezik: Rust 1.75+, edition 2021. Web: Axum. Indeks: Tantivy. HTTP klijent: reqwest (rustls).
- Crate-ovi: `core` (tipovi, trait `Source`, greške), `sources` (Brave, Wikipedia, ...), `ranking` (merge/dedupe/ad-score), `server` (Axum API + static frontend).
- Frontend faza 1: minimalan HTML/JS (jedna lista). Kolone faza 2.
- Konfig: env (`SEARCH_API_KEYS_*`), nikad tajne u repou. `.env.example` obavezan.

## 4. Koraci — FAZA 0: Temelj i alatke
- [x] **0.1 Scaffold workspace** — `Cargo.toml` workspace (`search_core`, `sources`, `ranking`, `server`), `.gitignore`, `README.md`, `.env.example`. AC: `cargo build` prolazi. Gotovo: build zelen. Napomena: crate se zove `search_core` (ime `core` senči std `core` u async_trait makrou).
- [x] **0.2 Quality gateovi** — `clippy -D warnings`, `cargo fmt --check`, tarpaulin prag 80%, CI workflow (build+test+clippy+fmt). AC: CI zelen na praznom projektu. Gotovo: sve zeleno lokalno.
- [x] **0.3 Health endpoint** — `GET /health` → `{"status":"ok","version"}` + integration test. AC: test prolazi. Gotovo: 6/6 testova (health, 404, query validacija, merge/dedupe).

## 5. Koraci — FAZA 1: Agregator (MVP pretraga)
- [x] **1.1 `search_core` tipovi** — `Query`, `SearchResult {title, url, snippet, source, score}`, `Source` trait (async, timeout 8s, nikad panic), `AppError` (thiserror). AC: unit testovi tipova + grešaka.
- [x] **1.2 Brave izvor** — `GET api.search.brave.com` sa ključem iz env, mapiranje u `SearchResult`. Bez ključa: izvor se gasi uz warning, ostali rade. AC: mock test (wiremock), live test samo uz ključ.
- [x] **1.3 Wikipedia izvor** — OpenSearch API (free, bez ključa). AC: live test (stabilan API).
- [x] **1.4 Merge + dedupe** — paralelno pozivanje izvora (`tokio::join`), dedupe po normalizovanom URL-u, sort po skoru. AC: test sa 2 mock izvora, merenje latencije < max(latencija izvora)+20%.
- [x] **1.5 `GET /search` + rate-limit + frontend** — Gotovo: živ test (health, Wikipedia rezultati, HTML 200, prazan upit 400). Rate-limit 60/min/IP, frontend jedna lista.
- [x] **1.6 Minimalan frontend** — jedna lista rezultata, search box, bez frameworka. Gotovo: `GET /` vraća HTML 200, provereno curl-om.

## 6. Koraci — FAZA 2: Ranking, filter, kolone
- [x] **2.1 Ad-score** — Gotovo: matrica 20 domena 20/20 (100% ≥ 90%).
- [x] **2.2 Ranking sloj** — Gotovo: RRF + ad penal, deterministički test, 19/19 testova zeleno.
- [x] **2.3 Intent detekcija** — Gotovo: matrica 40 upita 100% (≥85%).
- [x] **2.4 Kolone API** — Gotovo: `columns=` po imenu izvora + `template` po intentu, e2e testovi, živ curl.
- [x] **2.5 Kolone UI** — Gotovo: polje za kolone + side-by-side prikaz, mobile fallback je stack (CSS). Čuvanje rasporeda odloženo za fazu 4 (nalozi).

## 7. Koraci — FAZA 3: Sopstveni indeks i keš
- [x] **3.1 Keš** — Gotovo: TTL+FIFO keš, `X-Cache: HIT/MISS`, testovi + živ curl (MISS→HIT).
- [x] **3.2 Crawler** — Gotovo: robots.txt (naš UA + *), pauza po domenu, UA sa kontaktom, kap 5MB, ekstrakcija (title/tekst/linkovi, bez script/style). Test: 100 strana sa stuba, robots zabrana poštovana.
- [x] **3.3 Tantivy indeks** — Gotovo: schema title/body/url, 1000 dokumenata, p95 <100ms, testovi zeleni.
- [x] **3.4 `OwnIndex` kao izvor** — Gotovo: `own-index` izvor u agregatoru i kolonama (kreće prazan), e2e testovi + živ curl.
- [x] **3.5 Perzistencija + seed** — Gotovo: `SearchIndex::open_dir` (MmapDirectory open-or-create), `OwnIndexSource::open_dir`, `seed` bin (`SEED_URLS` → `INDEX_DIR`), server čita `INDEX_DIR`. Test: roundtrip add→reopen→search.
- [x] **3.6 Tantivy 0.22→0.26 upgrade** — Gotovo: `TopDocs::order_by_score`, `CompactDocValue` konverzija, `get_field` Result; stari `lru` soundness nalaz nestao iz audita.

## 12. Roadmap iz GitHub istraživanja (nakon 3.6)
1. **3.5 kraj**: `open_dir` (open-or-create) + `seed` bin + server čita `INDEX_DIR`.
2. **spider-rs procena**: spike da li menja naš crawler (JS render) — zamena samo ako zatreba JS-heavy sajtovi.
3. **Perplexica focus lensovi**: academic lens gotov (OpenAlex); discussions/calc lensovi kad dobiju izvor bez ključa.
4. **Stract čitanje**: ranking ideje (optics filteri) — istraživanje, ne kod.

## 13. Stranice (dizajn: dark slate brend, serif display + system sans, 0 eksternih)
- [x] **welcome** (`/welcome`), **pricing** (`/pricing`), **account** (`/account`), **docs** (`/docs`) — Gotovo: `pages.rs`, zajednički stil, `/api/credits` za stanje, testovi + live 200.

## 8. Koraci — FAZA 4: Prihod (posle trakcije, ne pre)
- [x] **4.1 Nalozi** — Gotovo: argon2id + JWT (30d), dnevna kvota 1000, Bearer na /search, 49/49 testova + živ curl (201/200/401).
- [x] **4.2 Ad slot** — Gotovo: odvojen `sponsored` slot (null bez inventara), organski poredak netaknut (test), frontend aside.
- [x] **4.3 Founding ponuda** — Gotovo: `PRICING.md` (prvih 100 lifetime, pa pretplata, pravila).

## 9. Koraci — FAZA 5: AI sloj (naplata, kasnije)
- [x] **5.1 Sažeci rezultata** — Gotovo: `ai` crate (OpenAI-kompatibilan, `cost_usd` iz `usage` ili procene chars/4, cene env override), `POST /summarize` (auth obavezan 401, 503 bez ključa, 502 LLM greška), keš per user+query (`X-Cache`), trošak logovan po pozivu. 60/60 testova + živ curl (401/401/502).
- [x] **5.2 Asistent** — Gotovo: `POST /ask` (pitanje+query → odgovor, ista auth/keš granica), živ curl potvrđen. Naplata (Polar) — posle launcha.

## 10. Standard "kod za 10" (važi za svaki korak)
1. Nema `unwrap`/`expect` na produkcionim putevima — samo u testovima.
2. Sve greške tipizirane (`thiserror`), mappirane u HTTP statuse; nema `anyhow` preko API granice.
3. `clippy -- -D warnings` čisto; `cargo fmt --check` čisto.
4. Svaka javna funkcija ima doc-komentar; svaki crate ima `README` ili modulsku dokumentaciju.
5. Timeout na svaki mrežni poziv; retry max 1 sa backoffom; nikad blokirajuće u async.
6. Tajne samo iz env; primer u `.env.example`; CI pada ako nađe tajnu (gitleaks ili grep).
7. Svaki korak ima testove pre/uz kod; coverage gate 80% (tarpaulin, `fail-under = 80`). Izmereno: 81.32% (862/1060).
8. Commit poruke: `feat|fix|docs|test|refactor: ...`; svaki korak = 1+ commit + push.

## 11. Otvoreno
- [ ] Konačno ime (predlozi: Reflect, Refind, Seekly, Lumen).
- [ ] Commerce vertikala: izvori cena.
- [x] Dnevni limit free pretraga — definisano i implementirano: 60/min/IP anonimno + 1000/dan ulogovani.
