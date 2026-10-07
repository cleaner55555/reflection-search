# web/ — pravi frontend (Vite + TS + Tailwind)

Hash rute u jednom `index.html`: `#/` landing, `#/app`, `#/pricing`, `#/account`, `#/docs`.
API na istom originu (dev proxy ka `:3000`, prod isti `searchd`).

```sh
npm install
npm run dev    # :5173, API proxy na searchd
npm run build  # web/dist — searchd servira / + /assets, fallback je ugrađeni prototip
```

Pravila: 0 eksternih zahteva (nema CDN/fontova), system fontovi, dark default + light toggle (`rs-theme`), localStorage za kolone/mod/token.
