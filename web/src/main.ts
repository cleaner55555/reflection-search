import './style.css';
import { init as initTheme, toggle } from './theme';
import { account, bindAccount } from './views/account';
import { app, bindApp } from './views/app';
import { docs } from './views/docs';
import { bindLanding, landing } from './views/landing';
import { pricing } from './views/pricing';

initTheme();

function themeBtn(): void {
  document.getElementById('theme-btn')?.addEventListener('click', () => toggle());
}

function route(): void {
  const root = document.getElementById('app') as HTMLElement;
  const hash = location.hash || '#/';
  const qIndex = hash.indexOf('?');
  const path = qIndex < 0 ? hash.slice(1) : hash.slice(1, qIndex);
  const params = new URLSearchParams(qIndex < 0 ? '' : hash.slice(qIndex + 1));
  window.scrollTo(0, 0);
  if (path === '/pricing') {
    root.innerHTML = pricing();
    themeBtn();
  } else if (path === '/account') {
    root.innerHTML = account();
    themeBtn();
    bindAccount();
  } else if (path === '/docs') {
    root.innerHTML = docs();
    themeBtn();
  } else if (path === '/app') {
    root.innerHTML = app();
    themeBtn();
    bindApp(params.get('q') ?? '', params.get('ask') === '1');
  } else {
    root.innerHTML = landing();
    themeBtn();
    bindLanding((q, ask) => {
      location.hash = `#/app?q=${encodeURIComponent(q)}${ask ? '&ask=1' : ''}`;
    });
  }
}

window.addEventListener('hashchange', route);
route();
