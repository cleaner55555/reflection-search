export interface SearchResult {
  title: string;
  url: string;
  snippet: string;
  source: string;
}

export interface SearchResponse {
  results: SearchResult[];
  total: number;
  template: string[];
  columns?: Record<string, SearchResult[]> | null;
  sponsored?: { title: string; url: string; reason: string } | null;
}

export interface AiAnswer {
  text: string;
  model: string;
  cost_usd: number;
}

async function req(path: string, init?: RequestInit) {
  const res = await fetch(path, init);
  if (!res.ok) {
    const err = new Error(`HTTP ${res.status}`) as Error & { status: number };
    err.status = res.status;
    throw err;
  }
  return res.json();
}

export function search(q: string, limit = 20): Promise<SearchResponse> {
  return req(`/search?limit=${limit}&q=${encodeURIComponent(q)}`);
}

export function searchColumns(q: string, columns: string[], limit = 10): Promise<SearchResponse> {
  return req(
    `/search?limit=${limit}&q=${encodeURIComponent(q)}&columns=${encodeURIComponent(columns.join(','))}`,
  );
}

export function summarize(query: string, token: string): Promise<AiAnswer> {
  return req('/summarize', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
    body: JSON.stringify({ query }),
  });
}

export function ask(query: string, question: string, token: string): Promise<AiAnswer> {
  return req('/ask', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
    body: JSON.stringify({ query, question }),
  });
}

export async function register(email: string, password: string): Promise<Response> {
  return fetch('/auth/register', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ email, password }),
  });
}

export async function login(email: string, password: string): Promise<{ token: string }> {
  return req('/auth/login', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ email, password }),
  });
}

export function credits(token: string): Promise<{ credits: number }> {
  return req('/api/credits', { headers: { Authorization: `Bearer ${token}` } });
}
