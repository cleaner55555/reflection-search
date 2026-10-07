# PRIVACY — Reflection Search (radni naziv)

Šta se čuva, šta se šalje, koliko dugo. Bez pravnog jezika.

## Bez naloga (anonimna pretraga)
- Ne čuvamo ništa trajno: nema cookie-ja, nema fingerprinta, nema analytics.
- Rate-limit je in-memory brojač po IP (60/min), briše se restartom servera.
- Keš pretraga je in-memory (ključ = upit+limit+kolone), TTL 10 min, bez vezivanja za korisnika.

## Sa nalogom (kvota + AI)
- `users.db` (SQLite): email, argon2id hash, dnevna potrošnja po danu, AI kreditno stanje (USD). Ništa drugo.
- AI pozivi loguju: korisnik, model, `cost_usd`, preostalo stanje. Tekst pitanja/odgovora se ne loguje.
- AI je prepaid: bez kredita 402, trošak se skida po pozivu, keš pogodak je besplatan.

## Šta se šalje trećim stranama (nužno za meta-pretragu)
| Izvor | Šta ide | Ključ |
|---|---|---|
| Brave API | tekst upita (sa serverskog IP-ja) | `BRAVE_API_KEY`, samo server ga vidi |
| Wikipedia | tekst upita (sa serverskog IP-ja) | bez ključa |
| OpenAlex | tekst upita (sa serverskog IP-ja) | bez ključa |
| LLM (OpenAI-kompatibilan) | upit + rezultati kao kontekst (samo AI rute) | `AI_API_KEY` |

## Mreža i hosting
- `searchd` sluša HTTP na 127.0.0.1:3000 — za javno obavezan HTTPS reverse proxy (Caddy/nginx), inače provajder vidi sve.
- Frontend: 0 eksternih zahteva (nema CDN, fontova, trackera).
- Odgovori nose: CSP, HSTS, `nosniff`, `no-referrer`, `DENY` frame, `no-store`, `x-request-id` (brojač po procesu, ne perzistira se).
- Logovi: metod + putanja + latencija + request_id na INFO; bez tela upita, bez headera (nema tokena u logovima).

## Brisanje
- Nalog: obrisati red iz `users.db` = sve nestaje (nema backup politike na prototipu).
- Keševi: restart servera prazni sve.

## Zavisnosti (cargo audit)
- Čisto osim 1 tranzitivnog upozorenja: `lru 0.12.5` (kroz tantivy) — soundness rupa u `IterMut`, bez uticaja na naše korišćenje, fix čeka tantivy upgrade. Bez RCE/infoleak nalaza.
