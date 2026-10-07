import { footer, header } from './layout';

function tier(name: string, price: string, features: string[], cta: string, href: string): string {
  return `
  <div class="rounded-xl border border-slate-200 bg-white p-6 dark:border-slate-800 dark:bg-slate-900">
    <h2 class="text-xl font-bold">${name}</h2>
    <p class="mt-1 text-3xl font-bold">${price}</p>
    <ul class="mt-4 space-y-2 text-sm text-slate-500 dark:text-slate-400">
      ${features.map((f) => `<li>✓ ${f}</li>`).join('')}
    </ul>
    <a href="${href}" class="mt-5 inline-block rounded-lg bg-sky-600 px-5 py-2.5 font-semibold text-white hover:bg-sky-500">${cta}</a>
  </div>`;
}

export function pricing(): string {
  return `
  ${header('#/pricing')}
  <main class="mx-auto max-w-4xl px-5 py-10">
    <h1 class="text-4xl font-bold tracking-tight">Jednostavne cene.</h1>
    <p class="mt-2 text-[17px] text-slate-500 dark:text-slate-400">Pretraga je free zauvek. Plaća se samo ono što košta: AI i pro alatke.</p>
    <div class="mt-8 grid gap-4 md:grid-cols-3">
      ${tier('Free', '$0', ['Dnevni limit pretraga', 'Sve kolone + academic lens', 'Extension, bez naloga'], 'Probaj pretragu', '#/')}
      ${tier('Pro', '$5/mes', ['AI sažeci i asistent u paketu', 'Čuvanje rasporeda na serveru', 'Prioritetni izvori'], 'Nalog', '#/account')}
      ${tier('AI krediti', 'pay-as-you-go', ['Stvarni cost_usd po pozivu', 'Keš pogodak besplatan', 'Bez kredita — 402, bez iznenađenja'], 'Dopuni', '#/account')}
    </div>
    <div class="mt-4 rounded-xl border border-slate-200 bg-white p-6 dark:border-slate-800 dark:bg-slate-900">
      <h2 class="text-xl font-bold">Founding 100</h2>
      <p class="mt-1 text-sm text-slate-500 dark:text-slate-400">Prvih 100: jednokratna uplata, lifetime Pro. Posle toga samo pretplata.</p>
    </div>
  </main>
  ${footer()}`;
}
