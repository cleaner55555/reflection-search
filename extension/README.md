# Extension — Reflection Search

Free pretraga kao plugin, bez API ključeva na klijentu. Zarada ostaje na AI sloju servera.

## Struktura (jedno jezgro, dva manifesta)
- `manifest-chrome.json` — MV3 (Chrome/Edge/Opera/Brave)
- `manifest-firefox.json` — MV2 (Firefox)
- `background.js` — omnibox keyword `rs` → otvara `search.html?q=`
- `popup.html/js` — brza pretraga + podešavanje servera (default `http://127.0.0.1:3000`)
- `search.html/js` — TweetDeck kolone (DnD reorder, localStorage, add/remove)

## Instalacija (dev)
- Chromium: `chrome://extensions` → Developer mode → Load unpacked → kopija foldera sa `manifest-chrome.json` preimenovanim u `manifest.json`
- Firefox: `about:debugging` → This Firefox → Load Temporary Add-on → `manifest-firefox.json` preimenovan u `manifest.json`

## Privatnost
- 0 eksternih zahteva osim konfigurisanog servera (nema CDN, fontova, trackera)
- Dozvole: `storage` + host servera; `*://*/*` se traži tek kad korisnik upiše custom server
- Omnibox upit ide samo na tvoj server
