/* Zajednički background za MV3 (Chromium) i MV2 (Firefox).
   Omnibox ključna reč: rs — otvara TweetDeck stranu sa upitom. */

const ext = globalThis.browser ?? globalThis.chrome;
const FALLBACK_SERVER = "http://127.0.0.1:3000";

async function serverBase() {
  try {
    const o = await ext.storage.local.get({ server: FALLBACK_SERVER });
    return String(o.server || FALLBACK_SERVER).replace(/\/$/, "");
  } catch (e) {
    return FALLBACK_SERVER;
  }
}

ext.omnibox.onInputEntered.addListener(async (q) => {
  const text = String(q || "").trim();
  if (!text) return;
  await serverBase();
  ext.tabs.create({
    url: ext.runtime.getURL("search.html") + "?q=" + encodeURIComponent(text),
  });
});
