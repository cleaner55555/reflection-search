//! Javne stranice: welcome, pricing, account, docs.
//!
//! Zajednički stil: dark slate brend kao app, serif naslovi + system sans,
//! 0 eksternih zahteva (nema CDN, fontova, trackera).

const CSS: &str = "*{box-sizing:border-box}body{margin:0;background:#020617;color:#e2e8f0;font-family:system-ui,sans-serif;line-height:1.6}main{max-width:720px;margin:0 auto;padding:48px 20px}h1,h2{font-family:Georgia,serif;color:#f8fafc}h1{font-size:40px;line-height:1.15;margin:0 0 16px}p.lead{font-size:18px;color:#94a3b8}a{color:#38bdf8}nav{padding:16px 20px;border-bottom:1px solid #1e293b}nav a{margin-right:16px;font-size:14px;text-decoration:none}.cta{display:inline-block;background:#0284c7;color:#fff!important;font-weight:600;border-radius:8px;padding:12px 24px;text-decoration:none;margin:8px 8px 8px 0}.cta:hover{background:#0ea5e9}.ghost{background:transparent;border:1px solid #334155}.card{border:1px solid #1e293b;border-radius:8px;padding:20px;margin:16px 0;background:#0f172a}table{width:100%;border-collapse:collapse;font-size:14px}td,th{border-bottom:1px solid #1e293b;padding:10px 8px;text-align:left}th{color:#94a3b8;font-weight:600}code{background:#1e293b;border-radius:4px;padding:2px 6px;font-size:13px}input{background:#1e293b;border:1px solid #334155;color:#f1f5f9;border-radius:6px;padding:9px 12px;width:100%;margin:4px 0}button{background:#0284c7;border:none;color:#fff;border-radius:6px;padding:10px 16px;cursor:pointer;font-weight:600}button:hover{background:#0ea5e9}.num{font-size:28px;font-weight:700;color:#f8fafc}footer{border-top:1px solid #1e293b;padding:24px 20px;text-align:center;color:#475569;font-size:13px}";

fn shell(title: &str, body: &str) -> String {
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">\
        <meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
        <title>{title} — Reflection Search</title>\
        <style>{CSS}</style></head><body>\
        <nav><a href=\"/welcome\">Početna</a><a href=\"/\">Pretraga</a>\
        <a href=\"/pricing\">Cene</a><a href=\"/account\">Nalog</a>\
        <a href=\"/docs\">Dokumentacija</a></nav>\
        <main>{body}</main>\
        <footer>Reflection Search — free pretraga zauvek. Bez praćenja.</footer>\
        </body></html>"
    )
}

/// Landing: šta, zašto free, 3 razloga, CTA.
#[must_use]
pub fn welcome() -> String {
    shell(
        "Free pretraga bez praćenja",
        "<h1>Pretraga koja radi za tebe, ne za oglašivače.</h1>\
        <p class=\"lead\">Reflection Search je besplatan meta-pretraživač: bez naloga, bez profila, bez reklama koje te prate.</p>\
        <p><a class=\"cta\" href=\"/\">Probaj pretragu</a>\
        <a class=\"cta ghost\" href=\"/pricing\">Cene</a></p>\
        <div class=\"card\"><h2>Kolone kao TweetDeck</h2>\
        <p>Složi izvore jedan pored drugog, prevuci redosled, sačuvaj raspored. Vesti, kod i nauka svako u svojoj koloni.</p></div>\
        <div class=\"card\"><h2>Academic lens</h2>\
        <p>Pitanja o radovima idu direktno na OpenAlex — 250M+ akademskih radova, citiranost u snippetu.</p></div>\
        <div class=\"card\"><h2>AI kao doplata, ne porez</h2>\
        <p>Sažeci i asistent se plaćaju po trošku iz prepaid kredita. Pretraga ostaje free zauvek.</p></div>",
    )
}

/// Cenovnik: Free / Pro / krediti + founding.
#[must_use]
pub fn pricing() -> String {
    shell(
        "Cene",
        "<h1>Jednostavne cene.</h1>\
        <p class=\"lead\">Pretraga je free zauvek. Plaća se samo ono što košta: AI i pro alatke.</p>\
        <div class=\"card\"><h2>Free — $0</h2>\
        <p>Dnevni limit pretraga, sve kolone, academic lens, extension. Bez naloga.</p></div>\
        <div class=\"card\"><h2>Pro — $5/mes</h2>\
        <p>AI sažeci i asistent u paketu, čuvanje rasporeda na serveru, prioritetni izvori.</p></div>\
        <div class=\"card\"><h2>AI krediti — pay-as-you-go</h2>\
        <p>Uplatiš $5, trošiš po stvarnom <code>cost_usd</code> poziva. Bez kredita — 402, bez iznenađenja. Keš pogodak je besplatan.</p></div>\
        <div class=\"card\"><h2>Founding 100</h2>\
        <p>Prvih 100: jednokratna uplata, lifetime Pro. Posle toga samo pretplata.</p></div>",
    )
}

/// Nalog: login, stanje, token, dopuna (podaci stižu JS-om sa API-ja).
#[must_use]
pub fn account() -> String {
    shell(
        "Nalog",
        "<h1>Tvoj nalog.</h1>\
        <p class=\"lead\">Uloguj se da vidiš kredite i token za agente.</p>\
        <div class=\"card\"><h2>Prijava</h2>\
        <input id=\"email\" placeholder=\"email\" autocomplete=\"email\">\
        <input id=\"pass\" type=\"password\" placeholder=\"lozinka (min 10 znakova)\">\
        <p><button id=\"login\">Prijavi se</button> <button id=\"reg\" class=\"ghost\" style=\"background:transparent;border:1px solid #334155\">Registruj se</button></p>\
        <p id=\"msg\"></p></div>\
        <div class=\"card\" id=\"dash\" style=\"display:none\"><h2>Stanje</h2>\
        <p class=\"num\"><span id=\"bal\">0</span> $</p>\
        <p>Token za dsh-plugin i API:</p>\
        <p><code id=\"tok\" style=\"word-break:break-all\"></code></p>\
        <p><a href=\"/pricing\">Dopuni kredite</a> · <a href=\"/docs\">Dokumentacija</a></p></div>\
        <script>\
        let TOK='';\
        async function call(path,body){const r=await fetch(path,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(body)});return r;}\
        async function refresh(){const r=await fetch('/api/credits',{headers:{Authorization:'Bearer '+TOK}});if(!r.ok){document.getElementById('msg').textContent='Greška: '+r.status;return;}const j=await r.json();document.getElementById('bal').textContent=j.credits.toFixed(4);document.getElementById('dash').style.display='block';}\
        async function auth(path){const e=document.getElementById('email').value,p=document.getElementById('pass').value;const r=await call(path,{email:e,password:p});if(path.endsWith('register')&&r.status!==201&&r.status!==409){document.getElementById('msg').textContent='Greška: '+r.status;return;}if(path.endsWith('register')){return auth('/auth/login');}if(!r.ok){document.getElementById('msg').textContent='Greška: '+r.status;return;}const j=await r.json();TOK=j.token;document.getElementById('tok').textContent=TOK;document.getElementById('msg').textContent='';refresh();}\
        document.getElementById('login').onclick=()=>auth('/auth/login');\
        document.getElementById('reg').onclick=()=>auth('/auth/register');\
        </script>",
    )
}

/// Dokumentacija: API tabela, extension, dsh-plugin, env, privatnost.
#[must_use]
pub fn docs() -> String {
    shell(
        "Dokumentacija",
        "<h1>Dokumentacija.</h1>\
        <h2>API</h2>\
        <table><tr><th>Metod</th><th>Ruta</th><th>Auth</th></tr>\
        <tr><td>GET</td><td><code>/search?q=&amp;limit=&amp;columns=</code></td><td>opciono (kvota)</td></tr>\
        <tr><td>POST</td><td><code>/summarize</code> {query}</td><td>obavezno + krediti</td></tr>\
        <tr><td>POST</td><td><code>/ask</code> {query, question}</td><td>obavezno + krediti</td></tr>\
        <tr><td>GET</td><td><code>/api/credits</code></td><td>obavezno</td></tr>\
        <tr><td>POST</td><td><code>/billing/webhook</code></td><td>Polar potpis</td></tr>\
        <tr><td>GET</td><td><code>/health</code></td><td>ne</td></tr></table>\
        <h2>Kolone</h2>\
        <p><code>all, brave, wikipedia, openalex, own-index</code> — max 4 po pozivu. Redosled se čuva u browseru, prevlačenjem.</p>\
        <h2>Extension</h2>\
        <p>Chromium (MV3) + Firefox (MV2) u <code>extension/</code>. Omnibox ključ <code>rs</code>, server se podešava u popupu.</p>\
        <h2>dsh-plugin</h2>\
        <p><code>dsh-plugin/</code>: <code>reflection_search</code> (free), <code>reflection_ask</code> + <code>reflection_summarize</code> (token sa kreditima). Env <code>REFLECTION_SEARCH_URL</code>, <code>REFLECTION_SEARCH_TOKEN</code>.</p>\
        <h2>Pretplata i krediti</h2>\
        <p>AI je prepaid: bez kredita 402. Uplata preko Polar checkouta → <code>order.paid</code> webhook dopunjuje stanje. Cene modela preko <code>AI_PROVIDER</code> (openai/deepseek) + <code>AI_MODEL</code>.</p>\
        <h2>Privatnost</h2>\
        <p>0 eksternih zahteva, nalog ne treba za pretragu, keš i logovi in-memory. Detalji u <code>PRIVACY.md</code> na GitHub-u.</p>",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages_render_without_externals() {
        for (name, html) in [
            ("welcome", welcome()),
            ("pricing", pricing()),
            ("account", account()),
            ("docs", docs()),
        ] {
            assert!(html.contains("Reflection Search"), "{name}");
            for attr in ["src=\"http", "href=\"http", "url(http", "@import"] {
                assert!(!html.contains(attr), "{name}: {attr}");
            }
        }
    }

    #[test]
    fn pricing_mentions_tiers() {
        let html = pricing();
        assert!(html.contains("$5"));
        assert!(html.contains("Founding 100"));
        assert!(html.contains("402"));
    }
}
