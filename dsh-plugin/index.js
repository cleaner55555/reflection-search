import { defineTool } from '@deepseek-ai/dsh-tools';

export const name = 'reflection-search';
export const inject = ['tools'];

const SERVER = (process.env.REFLECTION_SEARCH_URL || 'http://127.0.0.1:3000').replace(/\/$/, '');
const TOKEN = (process.env.REFLECTION_SEARCH_TOKEN || '').trim();

function authHeaders() {
  return TOKEN ? { Authorization: 'Bearer ' + TOKEN } : {};
}

async function postJson(path, body) {
  const res = await fetch(SERVER + path, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json', ...authHeaders() },
    body: JSON.stringify(body),
  });
  if (res.status === 401)
    return 'Reflection AI needs login: set REFLECTION_SEARCH_TOKEN from your account (AI is prepaid).';
  if (res.status === 402)
    return 'Out of AI credits: top up at your Reflection Search account, then retry.';
  if (res.status === 503)
    return 'Reflection AI is disabled on this server.';
  if (!res.ok) return 'Reflection AI failed with status ' + res.status + '.';
  const j = await res.json();
  return j.text || 'Empty answer.';
}

export function apply(ctx) {
  ctx.tools.register(
    defineTool({
      name: 'reflection_search',
      description:
        'Free private meta-search (Brave + Wikipedia + OpenAlex + own index). Use for current/factual queries. No API key needed for search.',
      parameters: {
        query: { type: 'string', required: true, description: 'Search query' },
        limit: { type: 'number', required: false, description: 'Max results, default 10, max 50' },
        columns: {
          type: 'string',
          required: false,
          description: 'Comma sources, e.g. "brave,wikipedia,openalex". Omit for merged list.',
        },
      },
      output: {
        schema: { type: 'string' },
        render: (_args, value) => [{ type: 'text', text: value }],
      },
      async execute(args) {
        const q = String(args.query || '').trim();
        if (!q) return 'Empty query.';
        const limit = Math.min(Math.max(Number(args.limit) || 10, 1), 50);
        let url = SERVER + '/search?limit=' + limit + '&q=' + encodeURIComponent(q);
        if (args.columns) url += '&columns=' + encodeURIComponent(String(args.columns));
        const res = await fetch(url);
        if (!res.ok) return 'Search failed with status ' + res.status + '.';
        const j = await res.json();
        const rows = j.columns
          ? Object.entries(j.columns).flatMap(([src, list]) =>
              list.map((r) => ({ ...r, source: src }))
            )
          : j.results || [];
        if (!rows.length) return 'No results.';
        return rows
          .slice(0, limit)
          .map((r, i) => (i + 1) + '. ' + r.title + '\n' + r.url + '\n' + (r.snippet || ''))
          .join('\n\n');
      },
    })
  );

  ctx.tools.register(
    defineTool({
      name: 'reflection_ask',
      description:
        'Ask a question answered ONLY from live Reflection Search results, with sources. Prepaid: needs REFLECTION_SEARCH_TOKEN with AI credits; every call bills the token owner at cost plus margin.',
      parameters: {
        query: { type: 'string', required: true, description: 'Search query for context' },
        question: { type: 'string', required: true, description: 'Question to answer' },
      },
      output: {
        schema: { type: 'string' },
        render: (_args, value) => [{ type: 'text', text: value }],
      },
      async execute(args) {
        const q = String(args.query || '').trim();
        const question = String(args.question || '').trim();
        if (!q || !question) return 'Empty query or question.';
        return postJson('/ask', { query: q, question });
      },
    })
  );

  ctx.tools.register(
    defineTool({
      name: 'reflection_summarize',
      description:
        'Summarize live Reflection Search results for a query in 3-5 sentences. Prepaid: needs REFLECTION_SEARCH_TOKEN with AI credits; every call bills the token owner at cost plus margin.',
      parameters: {
        query: { type: 'string', required: true, description: 'Search query to summarize' },
      },
      output: {
        schema: { type: 'string' },
        render: (_args, value) => [{ type: 'text', text: value }],
      },
      async execute(args) {
        const q = String(args.query || '').trim();
        if (!q) return 'Empty query.';
        return postJson('/summarize', { query: q });
      },
    })
  );
}
