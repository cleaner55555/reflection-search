import { credits, login, register } from '../api';
import { getToken, setToken } from '../store';
import { footer, header } from './layout';

export function account(): string {
  return `
  ${header('#/account')}
  <main class="mx-auto max-w-xl px-5 py-10">
    <h1 class="text-4xl font-bold tracking-tight">Tvoj nalog.</h1>
    <p class="mt-2 text-slate-500 dark:text-slate-400">Uloguj se da vidiš kredite i token za agente.</p>
    <div class="mt-6 rounded-xl border border-slate-200 bg-white p-6 dark:border-slate-800 dark:bg-slate-900">
      <h2 class="text-lg font-bold">Prijava</h2>
      <input id="acc-email" type="email" autocomplete="email" aria-label="email" placeholder="email"
        class="mt-3 w-full rounded-lg border border-slate-300 bg-white px-3 py-2.5 outline-none focus:border-sky-500 dark:border-slate-700 dark:bg-slate-900" />
      <input id="acc-pass" type="password" aria-label="lozinka" placeholder="lozinka (min 10 znakova)"
        class="mt-2 w-full rounded-lg border border-slate-300 bg-white px-3 py-2.5 outline-none focus:border-sky-500 dark:border-slate-700 dark:bg-slate-900" />
      <div class="mt-3 flex gap-2">
        <button id="acc-login" class="rounded-lg bg-sky-600 px-4 py-2.5 font-semibold text-white hover:bg-sky-500">Prijavi se</button>
        <button id="acc-reg" class="rounded-lg border border-slate-300 px-4 py-2.5 hover:bg-slate-100 dark:border-slate-700 dark:hover:bg-slate-800">Registruj se</button>
      </div>
      <p id="acc-msg" class="mt-2 text-sm text-red-500"></p>
    </div>
    <div id="acc-dash" class="mt-4 hidden rounded-xl border border-slate-200 bg-white p-6 dark:border-slate-800 dark:bg-slate-900">
      <h2 class="text-lg font-bold">Stanje</h2>
      <p class="mt-1 text-3xl font-bold"><span id="acc-bal">0</span> $</p>
      <p class="mt-3 text-sm text-slate-500">Token za dsh-plugin i API:</p>
      <p><code id="acc-tok" class="break-all rounded bg-slate-100 px-2 py-1 text-[13px] dark:bg-slate-800"></code></p>
      <p class="mt-3 text-sm"><a href="#/pricing" class="text-sky-600 hover:underline dark:text-sky-400">Dopuni kredite</a> · <a href="#/docs" class="text-sky-600 hover:underline dark:text-sky-400">Dokumentacija</a></p>
    </div>
  </main>
  ${footer()}`;
}

export function bindAccount(): void {
  const msg = document.getElementById('acc-msg') as HTMLElement;
  const dash = document.getElementById('acc-dash') as HTMLElement;

  async function show(token: string): Promise<void> {
    setToken(token);
    (document.getElementById('acc-tok') as HTMLElement).textContent = token;
    try {
      const b = await credits(token);
      (document.getElementById('acc-bal') as HTMLElement).textContent = b.credits.toFixed(4);
    } catch {
      (document.getElementById('acc-bal') as HTMLElement).textContent = '?';
    }
    dash.classList.remove('hidden');
    msg.textContent = '';
  }

  const existing = getToken();
  if (existing !== '') void show(existing);

  const email = () => (document.getElementById('acc-email') as HTMLInputElement).value;
  const pass = () => (document.getElementById('acc-pass') as HTMLInputElement).value;

  (document.getElementById('acc-login') as HTMLButtonElement).onclick = async () => {
    try {
      const { token } = await login(email(), pass());
      await show(token);
    } catch (e) {
      msg.textContent = `Greška: ${(e as Error & { status?: number }).status ?? 'mreža'}`;
    }
  };
  (document.getElementById('acc-reg') as HTMLButtonElement).onclick = async () => {
    const res = await register(email(), pass());
    if (res.status === 201 || res.status === 409) {
      try {
        const { token } = await login(email(), pass());
        await show(token);
      } catch (e) {
        msg.textContent = `Greška: ${(e as Error & { status?: number }).status ?? 'mreža'}`;
      }
    } else {
      msg.textContent = `Greška: ${res.status}`;
    }
  };
}
