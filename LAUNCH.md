# LAUNCH — Reflection Search (radni naziv)

Launch checklist (Faza 6 iz PLAN.md). Radi se TEK kad prethodi u planu.

## Preduslovi
- [ ] Konačno ime + domen (predlozi: Reflect, Refind, Seekly, Lumen — proveriti dostupnost)
- [ ] VPS (~$5/mes) + deploy `searchd` (systemd ili Docker)
- [ ] `BRAVE_API_KEY` (Besplatna kvota $5/mes ~1000 pretraga za start)
- [ ] `JWT_SECRET` (32+ bajta) + `USERS_DB_PATH` na trajnom disku
- [ ] HTTPS preko Caddy/nginx

## Pre lansiranja
- [ ] Coverage ≥ 80% (tarpaulin), clippy čist, CI zelen
- [ ] Seed indeksa (3.5): crawler → small-web seed lista
- [ ] Landing strana: free zauvek + nenametljive reklame + AI doplata
- [ ] PRICING.md: odlučiti founding cenu za prvih 100

## Founding ponuda (prvih 100)
- [ ] Lifetime "Founder" nalog — jednokratna cena, sve buduće pro funkcije
- [ ] Posle 100: samo pretplata, bez izuzetaka (PRICING.md pravila)
- [ ] Naplata: Polar (MoR, radi iz Srbije, isplate preko Stripe Connect)

## Kanali (0 marketing budžet — Kagi model)
- [ ] Hacker News "Show HN" (jedan dobar post vredi više od reklama)
- [ ] Reddit: r/selfhosted, r/rust, r/privacy
- [ ] Nišne zajednice: developeri, istraživači (kolone = pro alat)

## Merenje posle lansiranja
- [ ] Registracije/dan, konverzija u founding
- [ ] Trošak po upitu vs prihod (kvartalna provera iz PRICING.md)
- [ ] Feedback forum ili GitHub issues za korisnike
