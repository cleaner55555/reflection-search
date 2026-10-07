import { footer, header } from './layout';

export function docs(): string {
  return `
  ${header('#/docs')}
  <main class="mx-auto max-w-3xl px-5 py-10">
    <h1 class="text-4xl font-bold tracking-tight">Dokumentacija.</h1>
    <h2 class="mb-2 mt-8 text-xl font-bold">API</h2>
    <table class="w-full text-sm">
      <tr class="border-b border-slate-200 text-left text-slate-500 dark:border-slate-800 dark:text-slate-400"><th class="py-2">Metod</th><th>Ruta</th><th>Auth</th></tr>
      <tr class="border-b border-slate-100 dark:border-slate-800/60"><td><code>GET</code></td><td><code>/search?q=&amp;limit=&amp;columns=</code></td><td>opciono (kvota)</td></tr>
      <tr class="border-b border-slate-100 dark:border-slate-800/60"><td><code>POST</code></td><td><code>/summarize</code> {query}</td><td>obavezno + krediti</td></tr>
      <tr class="border-b border-slate-100 dark:border-slate-800/60"><td><code>POST</code></td><td><code>/ask</code> {query, question}</td><td>obavezno + krediti</td></tr>
      <tr class="border-b border-slate-100 dark:border-slate-800/60"><td><code>GET</code></td><td><code>/api/credits</code></td><td>obavezno</td></tr>
      <tr class="border-b border-slate-100 dark:border-slate-800/60"><td><code>POST</code></td><td><code>/billing/webhook</code></td><td>Polar potpis</td></tr>
      <tr><td><code>GET</code></td><td><code>/health</code></td><td>ne</td></tr>
    </table>
    <h2 class="mb-2 mt-8 text-xl font-bold">Kolone</h2>
    <p class="text-sm text-slate-500 dark:text-slate-400"><code>all, brave, wikipedia, openalex, own-index</code> — max 4 po pozivu. Redosled se čuva u browseru, prevlačenjem ili Alt+strelice.</p>
    <h2 class="mb-2 mt-8 text-xl font-bold">Extension</h2>
    <p class="text-sm text-slate-500 dark:text-slate-400">Chromium (MV3) + Firefox (MV2) u <code>extension/</code> folderu repoa. Omnibox ključ <code>rs</code>, server se podešava u popupu.</p>
    <h2 class="mb-2 mt-8 text-xl font-bold">dsh-plugin</h2>
    <p class="text-sm text-slate-500 dark:text-slate-400"><code>dsh-plugin/</code>: <code>reflection_search</code> (free), <code>reflection_ask</code> + <code>reflection_summarize</code> (token sa kreditima). Env <code>REFLECTION_SEARCH_URL</code>, <code>REFLECTION_SEARCH_TOKEN</code>.</p>
    <h2 class="mb-2 mt-8 text-xl font-bold">Pretplata i krediti</h2>
    <p class="text-sm text-slate-500 dark:text-slate-400">AI je prepaid: bez kredita 402. Uplata preko Polar checkouta → <code>order.paid</code> webhook dopunjuje stanje. Cene modela preko <code>AI_PROVIDER</code> (openai/deepseek) + <code>AI_MODEL</code>.</p>
    <h2 class="mb-2 mt-8 text-xl font-bold">Privatnost</h2>
    <p class="text-sm text-slate-500 dark:text-slate-400">0 eksternih zahteva, nalog ne treba za pretragu, keš i logovi in-memory. Detalji u <code>PRIVACY.md</code> na GitHub-u.</p>
  </main>
  ${footer()}`;
}
