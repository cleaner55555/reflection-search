function read(key: string, fallback: string): string {
  try {
    return localStorage.getItem(key) ?? fallback;
  } catch {
    return fallback;
  }
}

function write(key: string, value: string): void {
  try {
    localStorage.setItem(key, value);
  } catch {
    /* neblokirajuće */
  }
}

export function getCols(): string[] {
  try {
    const v = JSON.parse(read('rs-cols', '["all"]')) as unknown;
    return Array.isArray(v) && v.length > 0 ? (v as string[]) : ['all'];
  } catch {
    return ['all'];
  }
}

export function setCols(cols: string[]): void {
  write('rs-cols', JSON.stringify(cols.length > 0 ? cols : ['all']));
}

export function getMode(): 'list' | 'cols' {
  return read('rs-mode', 'list') === 'cols' ? 'cols' : 'list';
}

export function setMode(mode: 'list' | 'cols'): void {
  write('rs-mode', mode);
}

export function getToken(): string {
  return read('rs-token', '');
}

export function setToken(token: string): void {
  write('rs-token', token);
}

export function esc(s: string | null | undefined): string {
  return String(s ?? '')
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;');
}

export const SOURCE_DOT: Record<string, string> = {
  all: '#38bdf8',
  brave: '#f59e0b',
  wikipedia: '#10b981',
  openalex: '#fb7185',
  'own-index': '#64748b',
};
