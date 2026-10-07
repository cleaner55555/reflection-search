/* Popup: brza pretraga + podešavanje servera. Bez eksternih zahteva. */

const ext = globalThis.browser ?? globalThis.chrome;
const FALLBACK_SERVER = "http://127.0.0.1:3000";

function norm(u) {
  return String(u || "").trim().replace(/\/$/, "") || FALLBACK_SERVER;
}

async function init() {
  const q = document.getElementById("q");
  const srv = document.getElementById("srv");
  q.focus();
  try {
    const o = await ext.storage.local.get({ server: FALLBACK_SERVER });
    srv.value = o.server || FALLBACK_SERVER;
  } catch (e) {
    srv.value = FALLBACK_SERVER;
  }
  const go = () => {
    const text = q.value.trim();
    if (!text) return;
    ext.tabs.create({
      url: ext.runtime.getURL("search.html") + "?q=" + encodeURIComponent(text),
    });
    window.close();
  };
  document.getElementById("go").onclick = go;
  q.onkeydown = (e) => {
    if (e.key === "Enter") go();
  };
  document.getElementById("save").onclick = async () => {
    const server = norm(srv.value);
    try {
      await ext.storage.local.set({ server });
      if (ext.permissions && ext.permissions.request) {
        await ext.permissions.request({ origins: [server + "/*"] });
      }
    } catch (e) {
      /* neblokirajuće: permissions API nije svuda dostupan */
    }
    srv.value = server;
  };
}

document.addEventListener("DOMContentLoaded", init);
