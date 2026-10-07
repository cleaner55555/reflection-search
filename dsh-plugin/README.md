# reflection-search-dsh-plugin

DeepSeek Harness (`dsh`) plugin — free private meta-search kao alat za agente.
Topic za discoverability: `dsh-plugin`.

## Šta registruje

- `reflection_search(query, limit?, columns?)` — gađa `GET /search` našeg `searchd`-a,
  vraća numerisanu listu `naslov + url + snippet` sa citiranim izvorima. Free, bez naloga.
- `reflection_ask(query, question)` — odgovor iz živih rezultata (`POST /ask`).
- `reflection_summarize(query)` — sažetak rezultata (`POST /summarize`).

AI alati su prepaid: traže `REFLECTION_SEARCH_TOKEN` (token sa vašeg naloga)
sa AI kreditima — svaki poziv skida `cost_usd` i nama ostaje marža.
Bez tokena vraćaju uputstvo za login, bez kredita uputstvo za dopunu.

## Podešavanje

```sh
REFLECTION_SEARCH_URL=http://127.0.0.1:3000 pnpm dsh web --patch ./cordis.yml
```

`REFLECTION_SEARCH_URL` default: `http://127.0.0.1:3000` (lokalni `searchd`).
Pretraga ne traži nalog; AI rute plugin ne zove (naplata ostaje na serveru).

## Objavljivanje

npm paket + GitHub topic `dsh-plugin` (vidi `package.json` `dsh.bundle`).
