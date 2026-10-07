import { footer, header } from './layout';

export function landing(): string {
  return `
  ${header('#/')}
  <main class="mx-auto flex min-h-[85vh] w-full max-w-2xl flex-col items-center justify-center px-5 text-center">
    <h1 class="text-5xl font-bold tracking-tight text-slate-900 dark:text-white">Reflection Search</h1>
    <p class="mt-2 text-[17px] text-slate-500 dark:text-slate-400">Free pretraga bez praćenja. Bez naloga.</p>
    <form id="home-form" class="mt-7 flex w-full gap-2">
      <input id="home-q" aria-label="upit za pretragu" placeholder="pretraži web…" autofocus
        class="min-w-0 flex-1 rounded-xl border border-slate-300 bg-white px-5 py-3.5 text-[17px] text-slate-900 outline-none focus:border-sky-500 dark:border-slate-700 dark:bg-slate-900 dark:text-white" />
      <button class="rounded-xl bg-sky-600 px-6 font-semibold text-white hover:bg-sky-500">Traži</button>
      <button id="home-ask" type="button" class="rounded-xl border border-slate-300 px-4 text-slate-600 hover:bg-slate-100 dark:border-slate-700 dark:text-slate-300 dark:hover:bg-slate-800">Pitaj AI</button>
    </form>
    <div class="mt-3 flex flex-wrap justify-center gap-2">
      <button data-q="rust" class="chip rounded-full border border-slate-300 px-3 py-1 text-[13px] text-slate-500 hover:bg-slate-100 dark:border-slate-700 dark:text-slate-400 dark:hover:bg-slate-800">rust</button>
      <button data-q="transformer paper" class="chip rounded-full border border-slate-300 px-3 py-1 text-[13px] text-slate-500 hover:bg-slate-100 dark:border-slate-700 dark:text-slate-400 dark:hover:bg-slate-800">transformer paper</button>
      <button data-q="breaking news today" class="chip rounded-full border border-slate-300 px-3 py-1 text-[13px] text-slate-500 hover:bg-slate-100 dark:border-slate-700 dark:text-slate-400 dark:hover:bg-slate-800">vesti</button>
    </div>
    <p class="mt-2 text-[13px] text-slate-400 dark:text-slate-500">AI je prepaid — token sa <a href="#/account" class="text-sky-600 hover:underline dark:text-sky-400">naloga</a>.</p>
    <nav class="mt-8 flex flex-wrap justify-center gap-5 text-sm">
      <a href="#/pricing" class="text-slate-500 hover:text-slate-900 dark:text-slate-400 dark:hover:text-white">Cene</a>
      <a href="#/account" class="text-slate-500 hover:text-slate-900 dark:text-slate-400 dark:hover:text-white">Nalog</a>
      <a href="#/docs" class="text-slate-500 hover:text-slate-900 dark:text-slate-400 dark:hover:text-white">Dokumentacija</a>
    </nav>
  </main>
  ${footer()}`;
}

export function bindLanding(goApp: (q: string, ask: boolean) => void): void {
  const form = document.getElementById('home-form') as HTMLFormElement | null;
  const input = document.getElementById('home-q') as HTMLInputElement | null;
  form?.addEventListener('submit', (e) => {
    e.preventDefault();
    const q = input?.value.trim() ?? '';
    if (q) goApp(q, false);
  });
  document.querySelectorAll<HTMLButtonElement>('.chip').forEach((b) => {
    b.onclick = () => {
      const q = b.dataset.q ?? '';
      if (q) goApp(q, false);
    };
  });
  document.getElementById('home-ask')?.addEventListener('click', () => {
    const q = input?.value.trim() ?? '';
    if (q) goApp(q, true);
  });
}
