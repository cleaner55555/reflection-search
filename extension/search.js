/* TweetDeck kolone nad /search API-jem. Server se čita iz storage-a.
   Bez eksternih zahteva osim konfigurisanog servera. */

const ext = globalThis.browser ?? globalThis.chrome;
const FALLBACK_SERVER = "http://127.0.0.1:3000";

let SERVER = FALLBACK_SERVER;
let cols = [];
try {
  cols = JSON.parse(localStorage.getItem("rs-cols") || '["all"]');
} catch (e) {
  cols = ["all"];
}

function save() {
  try {
    localStorage.setItem("rs-cols", JSON.stringify(cols));
  } catch (e) {
    /* neblokirajuće */
  }
}

function esc(s) {
  return String(s == null ? "" : s).replace(/&/g, "&amp;").replace(/</g, "&lt;");
}

function fail(msg) {
  document.getElementById("err").textContent = msg;
}

function render() {
  const m = document.getElementById("cols");
  m.innerHTML = "";
  cols.forEach((c, i) => {
    const d = document.createElement("section");
    d.className = "col";
    d.draggable = true;
    d.dataset.i = i;
    d.innerHTML =
      '<div class="colhead"><span class="grip">++</span><h2>' +
      esc(c) +
      '</h2><button class="x" data-x="' +
      i +
      '">x</button></div><ul id="col-' +
      i +
      '"><li class="empty">-</li></ul>';
    m.appendChild(d);
  });
  m.querySelectorAll("[data-x]").forEach((b) => {
    b.onclick = () => {
      cols.splice(+b.dataset.x, 1);
      if (!cols.length) cols = ["all"];
      save();
      render();
      search();
    };
  });
  m.querySelectorAll("section").forEach((s) => {
    s.ondragstart = (e) => {
      e.dataTransfer.setData("text/plain", s.dataset.i);
      s.classList.add("dragging");
    };
    s.ondragend = () => s.classList.remove("dragging");
    s.ondragover = (e) => e.preventDefault();
    s.ondrop = (e) => {
      e.preventDefault();
      const from = +e.dataTransfer.getData("text/plain");
      const to = +s.dataset.i;
      if (from === to) return;
      const mv = cols.splice(from, 1);
      cols.splice(to, 0, mv[0]);
      save();
      render();
      search();
    };
  });
}

function item(x) {
  return (
    '<li class="card"><a target="_blank" rel="noopener" href="' +
    esc(x.url) +
    '">' +
    esc(x.title) +
    '</a><p>' +
    esc(x.snippet || "") +
    '</p><span>[' +
    esc(x.source) +
    "]</span></li>"
  );
}

async function search() {
  const qv = document.getElementById("q").value.trim();
  if (!qv) return;
  fail("");
  let res;
  try {
    res = await fetch(
      SERVER +
        "/search?limit=10&q=" +
        encodeURIComponent(qv) +
        "&columns=" +
        encodeURIComponent(cols.join(","))
    );
  } catch (e) {
    fail("Server nedostupan (" + SERVER + "). Proveri popup podešavanje.");
    return;
  }
  if (!res.ok) {
    fail("Server greška: " + res.status);
    return;
  }
  const j = await res.json();
  document.getElementById("ad").innerHTML = j.sponsored
    ? '<aside>Sponsored: <a href="' +
      esc(j.sponsored.url) +
      '">' +
      esc(j.sponsored.title) +
      "</a></aside>"
    : "";
  cols.forEach((c, i) => {
    const ul = document.getElementById("col-" + i);
    if (!ul) return;
    const rows = (j.columns && j.columns[c]) || [];
    ul.innerHTML = rows.length
      ? rows.map(item).join("")
      : '<li class="empty">nema rezultata</li>';
  });
}

async function init() {
  try {
    const o = await ext.storage.local.get({ server: FALLBACK_SERVER });
    SERVER = String(o.server || FALLBACK_SERVER).replace(/\/$/, "");
  } catch (e) {
    SERVER = FALLBACK_SERVER;
  }
  const params = new URLSearchParams(location.search);
  const q = params.get("q");
  if (q) document.getElementById("q").value = q;
  document.getElementById("f").onsubmit = (e) => {
    e.preventDefault();
    search();
  };
  document.getElementById("add").onclick = () => {
    const v = document.getElementById("src").value;
    if (!cols.includes(v)) {
      cols.push(v);
      save();
      render();
      search();
    }
  };
  render();
  if (q) search();
}

document.addEventListener("DOMContentLoaded", init);
