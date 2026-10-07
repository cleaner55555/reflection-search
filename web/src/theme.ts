const KEY = 'rs-theme';

export type Theme = 'dark' | 'light';

export function current(): Theme {
  try {
    return localStorage.getItem(KEY) === 'light' ? 'light' : 'dark';
  } catch {
    return 'dark';
  }
}

export function apply(theme: Theme): void {
  if (theme === 'light') {
    document.documentElement.setAttribute('data-theme', 'light');
  } else {
    document.documentElement.removeAttribute('data-theme');
    document.documentElement.setAttribute('data-theme', 'dark');
  }
  try {
    localStorage.setItem(KEY, theme);
  } catch {
    /* neblokirajuće */
  }
}

export function toggle(): Theme {
  const next: Theme = current() === 'light' ? 'dark' : 'light';
  apply(next);
  return next;
}

export function init(): void {
  apply(current());
}
