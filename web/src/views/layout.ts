import type { SearchResult } from '../api';
import { esc } from '../store';

export function header(active: string): string {
  const link = (href: string, label: string) =>
    `<a href="${href}" class="px-1 py-1 text-sm text-slate-500 hover:text-slate-900 dark:text-slate-400 dark:hover:text-slate-100 ${active === href ? 'font-semibold text-slate-900 dark:text-white' : ''}">${label}</a>`;
  return `
  <header class="sticky top-0 z-10 border-b border-slate-200 bg-white/95 backdrop-blur dark:border-slate-800 dark:bg-slate-950/95">
    <div class="mx-auto flex max-w-6xl flex-wrap items-center gap-x-4 gap-y-2 px-4 py-3">
      <a href="#/" class="text-[17px] font-bold text-slate-900 dark:text-white">Reflection Search</a>
      <nav class="ml-auto flex items-center gap-4">
        ${link('#/', 'Pretraga')}
        ${link('#/pricing', 'Cene')}
        ${link('#/account', 'Nalog')}
        ${link('#/docs', 'Dokumentacija')}
        <button id="theme-btn" title="svetla/tamna tema" aria-label="promeni temu" class="rounded-md border border-slate-300 px-2 py-1 text-slate-600 hover:bg-slate-100 dark:border-slate-700 dark:text-slate-300 dark:hover:bg-slate-800">◐</button>
      </nav>
    </div>
  </header>`;
}

export function footer(): string {
  return `
  <footer class="border-t border-slate-200 py-6 text-center text-[13px] text-slate-400 dark:border-slate-800 dark:text-slate-600">
    Reflection Search — free pretraga zauvek. Bez praćenja.
  </footer>`;
}

export function resultCard(r: SearchResult): string {
  return `
  <li class="card-lift rounded-lg border border-slate-200 bg-white p-3 hover:border-slate-300 dark:border-slate-800 dark:bg-slate-900 dark:hover:border-slate-700">
    <a target="_blank" rel="noopener" href="${esc(r.url)}" class="text-[15px] font-semibold text-sky-700 hover:underline dark:text-sky-400">${esc(r.title)}</a>
    <p class="mt-1 text-[13px] text-slate-500 dark:text-slate-400">${esc(r.snippet)}</p>
    <span class="text-[11px] text-slate-400 dark:text-slate-600">[${esc(r.source)}]</span>
  </li>`;
}
