import { ask as apiAsk, search, searchColumns, type SearchResult } from '../api';
import { SOURCE_DOT, esc, getCols, getMode, getToken, setCols, setMode } from '../store';
import { footer, header, resultCard } from './layout';

const SOURCES = ['all', 'brave', 'wikipedia', 'openalex', 'own-index'];

export function app(): string {
  return `
  ${header('#/app')}
  <div id="appbar" class="mx-auto flex max-w-6xl flex-wrap items-center gap-2 px-4 pt-4">
    <form id="app-form" class="flex min-w-52 flex-1 gap-2">
      <input id="app-q" aria-label="upit za pretragu" placeholder="pretraga…"
        class="min-w-0 flex-1 rounded-lg border border-slate-300 bg-white px-3 py-2 text-[15px] outline-none focus:border-sky-500 dark:border-slate-700 dark:bg-slate-900 dark:text-white" />
      <button class="rounded-lg bg-sky-600 px-4 font-semibold text-white hover:bg-sky-500">Traži</button>
      <button id="app-ask" type="button" title="pitaj AI o upitu" class="rounded-lg border border-slate-300 px-3 text-slate-600 hover:bg-slate-100 dark:border-slate-700 dark:text-slate-300 dark:hover:bg-slate-800">Pitaj AI</button>
    </form>
    <button id="mode-btn" aria-label="prebaci lista kolone" class="rounded-lg border border-slate-300 px-3 py-2 text-slate-600 hover:bg-slate-100 dark:border-slate-700 dark:text-slate-300 dark:hover:bg-slate-800">Kolone</button>
  </div>
  <div id="colbar" class="mx-auto hidden max-w-6xl flex-wrap items-center gap-2 px-4 pt-3">
    <select id="col-src" aria-label="izvor za novu kolonu" class="rounded-lg border border-slate-300 bg-white px-2 py-2 text-sm dark:border-slate-700 dark:bg-slate-900">
      ${SOURCES.map((s) => `<option value="${s}">${s}</option>`).join('')}
    </select>
    <button id="col-add" class="rounded-lg border border-slate-300 px-3 py-2 text-sm hover:bg-slate-100 dark:border-slate-700 dark:hover:bg-slate-800">+ Kolona</button>
    <input id="ai-token" aria-label="AI token" type="password" autocomplete="off" spellcheck="false" placeholder="token (sa Naloga)"
      class="max-w-56 rounded-lg border border-slate-300 bg-white px-3 py-2 text-sm dark:border-slate-700 dark:bg-slate-900" />
  </div>
  <div id="ad" class="mx-auto max-w-6xl px-4 pt-2"></div>
  <div id="aibox" class="mx-auto mt-3 hidden max-w-3xl rounded-lg border border-slate-200 bg-white p-4 dark:border-slate-800 dark:bg-slate-900">
    <h2 class="text-[15px] font-semibold">AI odgovor</h2>
    <p id="aitext" class="mt-1 text-sm"></p>
    <p id="aicost" class="mt-1 text-xs text-slate-400"></p>
  </div>
  <main id="cols" class="col-scroll mx-auto hidden max-w-6xl items-start gap-3 overflow-x-auto p-4"></main>
  <ul id="list" class="col-scroll mx-auto flex max-w-3xl list-none flex-col gap-3 p-4"></ul>
  ${footer()}`;
}

export function bindApp(initialQuery: string, initialAsk: boolean): void {
  let cols = getCols();
  let mode = getMode();
  const tokenInput = document.getElementById('ai-token') as HTMLInputElement | null;
  if (tokenInput) tokenInput.value = getToken();

  const colsEl = document.getElementById('cols') as HTMLElement;
  const listEl = document.getElementById('list') as HTMLElement;
  const colbar = document.getElementById('colbar') as HTMLElement;
  const qInput = document.getElementById('app-q') as HTMLInputElement;
  const modeBtn = document.getElementById('mode-btn') as HTMLButtonElement;

  function save(): void {
    setCols(cols);
    setMode(mode);
  }

  function render(): void {
    const listMode = mode === 'list';
    colsEl.style.display = listMode ? 'none' : 'flex';
    listEl.style.display = listMode ? 'flex' : 'none';
    colbar.style.display = listMode ? 'none' : 'flex';
    modeBtn.textContent = listMode ? 'Kolone' : 'Lista';
    if (listMode) return;
    colsEl.innerHTML = '';
    cols.forEach((c, i) => {
      const dot = SOURCE_DOT[c] ?? '#64748b';
      const s = document.createElement('section');
      s.className =
        'flex w-[360px] min-w-[360px] flex-col overflow-hidden rounded-xl border border-slate-200 bg-white dark:border-slate-800 dark:bg-slate-900';
      s.style.maxHeight = 'calc(100vh - 150px)';
      s.draggable = true;
      s.dataset.i = String(i);
      s.innerHTML =
        `<div class="colhead flex cursor-move items-center gap-2.5 border-b border-slate-200 p-2.5 dark:border-slate-800" tabindex="0" data-kb="${i}">` +
        `<span aria-hidden="true" class="text-slate-400">⠿</span><h2 class="flex-1 text-[15px] font-semibold">` +
        `<span style="display:inline-block;width:8px;height:8px;border-radius:50%;background:${dot}"></span> ${esc(c)} ` +
        `<span class="rounded-full border border-slate-300 px-2 py-px text-xs text-slate-500 dark:border-slate-700 dark:text-slate-400" id="cnt-${i}"></span></h2>` +
        `<button class="x h-8 w-8 rounded-md text-base text-slate-400 hover:bg-slate-100 hover:text-red-500 dark:hover:bg-slate-800" data-x="${i}" title="ukloni kolonu" aria-label="ukloni kolonu ${esc(c)}">✕</button></div>` +
        `<ul id="col-${i}" class="col-scroll flex flex-col gap-2.5 overflow-y-auto p-2.5"><li class="px-1 text-sm text-slate-400">—</li></ul>`;
      colsEl.appendChild(s);
    });
    colsEl.querySelectorAll<HTMLButtonElement>('[data-x]').forEach((b) => {
      b.onclick = () => {
        cols.splice(Number(b.dataset.x), 1);
        if (cols.length === 0) cols = ['all'];
        save();
        render();
        void runSearch();
      };
    });
    colsEl.querySelectorAll<HTMLElement>('section').forEach((s) => {
      s.ondragstart = (e) => {
        e.dataTransfer?.setData('text/plain', s.dataset.i ?? '0');
        s.classList.add('opacity-40');
      };
      s.ondragend = () => s.classList.remove('opacity-40');
      s.ondragover = (e) => e.preventDefault();
      s.ondrop = (e) => {
        e.preventDefault();
        const from = Number(e.dataTransfer?.getData('text/plain') ?? '0');
        const to = Number(s.dataset.i ?? '0');
        if (from === to || Number.isNaN(from) || Number.isNaN(to)) return;
        const [mv] = cols.splice(from, 1);
        cols.splice(to, 0, mv);
        save();
        render();
        void runSearch();
      };
    });
    colsEl.querySelectorAll<HTMLElement>('[data-kb]').forEach((h) => {
      h.onkeydown = (e) => {
        if (!e.altKey) return;
        const i = Number(h.dataset.kb ?? '0');
        const j = i + (e.key === 'ArrowRight' ? 1 : e.key === 'ArrowLeft' ? -1 : 0);
        if (e.key !== 'ArrowRight' && e.key !== 'ArrowLeft') return;
        if (j < 0 || j >= cols.length) return;
        e.preventDefault();
        const [mv] = cols.splice(i, 1);
        cols.splice(j, 0, mv);
        save();
        render();
        void runSearch();
      };
    });
  }

  function showAd(j: { sponsored?: { title: string; url: string } | null }): void {
    document.getElementById('ad')!.innerHTML = j.sponsored
      ? `<aside class="rounded-md border border-dashed border-slate-300 p-2 text-xs text-slate-500 dark:border-slate-700 dark:text-slate-400">Sponsored: ` +
        `<a target="_blank" rel="noopener" class="text-sky-600 hover:underline dark:text-sky-400" href="${esc(j.sponsored.url)}">${esc(j.sponsored.title)}</a></aside>`
      : '';
  }

  async function runSearch(): Promise<void> {
    const qv = qInput.value.trim();
    if (!qv) return;
    if (mode === 'list') {
      const j = await search(qv, 20);
      showAd(j);
      listEl.innerHTML =
        j.results.map(resultCard).join('') || '<li class="px-1 text-sm text-slate-400">nema rezultata</li>';
      return;
    }
    const j = await searchColumns(qv, cols, 10);
    showAd(j);
    cols.forEach((c, i) => {
      const ul = document.getElementById(`col-${i}`);
      if (!ul) return;
      const rows: SearchResult[] = (j.columns?.[c] as SearchResult[] | undefined) ?? [];
      ul.innerHTML = rows.length > 0 ? rows.map(resultCard).join('') : '<li class="px-1 text-sm text-slate-400">nema rezultata</li>';
      const badge = document.getElementById(`cnt-${i}`);
      if (badge) badge.textContent = String(rows.length);
    });
  }

  async function runAsk(): Promise<void> {
    const qv = qInput.value.trim();
    if (!qv) return;
    const box = document.getElementById('aibox') as HTMLElement;
    const text = document.getElementById('aitext') as HTMLElement;
    const cost = document.getElementById('aicost') as HTMLElement;
    box.classList.remove('hidden');
    const t = getToken();
    if (!t) {
      text.textContent = 'Treba token sa Naloga (AI je prepaid).';
      cost.textContent = '';
      return;
    }
    try {
      const a = await apiAsk(qv, qv, t);
      text.textContent = a.text;
      cost.textContent = `trošak $${a.cost_usd.toFixed(6)}`;
    } catch (e) {
      const status = (e as Error & { status?: number }).status;
      if (status === 402) {
        text.textContent = 'Nema kredita — dopuni na stranici Cene.';
      } else {
        text.textContent = `Greška: ${status ?? 'mreža'}`;
      }
      cost.textContent = '';
    }
    await runSearch();
  }

  (document.getElementById('app-form') as HTMLFormElement).onsubmit = (e) => {
    e.preventDefault();
    void runSearch();
  };
  (document.getElementById('app-ask') as HTMLButtonElement).onclick = () => void runAsk();
  modeBtn.onclick = () => {
    mode = mode === 'list' ? 'cols' : 'list';
    save();
    render();
    if (qInput.value.trim() !== '') void runSearch();
  };
  const srcSel = document.getElementById('col-src') as HTMLSelectElement | null;
  const addBtn = document.getElementById('col-add') as HTMLButtonElement | null;
  addBtn?.addEventListener('click', () => {
    const v = srcSel?.value ?? '';
    if (v !== '' && !cols.includes(v)) {
      cols.push(v);
      save();
      render();
      void runSearch();
    }
  });
  render();
  if (initialQuery) {
    qInput.value = initialQuery;
    if (initialAsk) void runAsk();
    else void runSearch();
  }
}
