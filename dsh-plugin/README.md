# reflection-search-dsh-plugin

DeepSeek Harness (`dsh`) plugin — free private meta-search kao alat za agente.
Topic za discoverability: `dsh-plugin`.

## Šta registruje

- `reflection_search(query, limit?, columns?)` — gađa `GET /search` našeg `searchd`-a,
  vraća numerisanu listu `naslov + url + snippet` sa citiranim izvorima.

## Podešavanje

```sh
REFLECTION_SEARCH_URL=http://127.0.0.1:3000 pnpm dsh web --patch ./cordis.yml
```

`REFLECTION_SEARCH_URL` default: `http://127.0.0.1:3000` (lokalni `searchd`).
Pretraga ne traži nalog; AI rute plugin ne zove (naplata ostaje na serveru).

## Objavljivanje

npm paket + GitHub topic `dsh-plugin` (vidi `package.json` `dsh.bundle`).
