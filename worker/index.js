/**
 * PoliticalDebAIser — Cloudflare Worker
 *
 * Responsibilities:
 *   1. /scrape   — Fetch + extract article text from any URL (bypasses CORS)
 *   2. /llm      — Proxy LLM calls using server-side "house" API keys (fallback when user has no key)
 *   3. /health   — Simple health check
 *
 * Environment variables (set in Cloudflare dashboard → Workers → Settings → Variables):
 *   HOUSE_GROQ_KEY_1, HOUSE_GROQ_KEY_2, ...    — Your Groq keys (round-robin)
 *   HOUSE_GEMINI_KEY_1, HOUSE_GEMINI_KEY_2, ... — Your Gemini keys
 *   HOUSE_HF_KEY_1, ...                         — HuggingFace keys
 *   ALLOWED_ORIGIN                              — Your Pages domain (e.g. https://debaiser.pages.dev)
 *
 * Security model:
 *   - House keys NEVER leave the Worker — they are used server-side only
 *   - /llm endpoint only proxies requests from the allowed origin
 *   - User-supplied keys are NOT handled here — they go direct from browser to provider
 */

// ── CORS ──────────────────────────────────────────────────────────────────────

function corsHeaders(origin, env) {
  const allowed = env.ALLOWED_ORIGIN || '*';
  return {
    'Access-Control-Allow-Origin': allowed === '*' ? '*' : origin || allowed,
    'Access-Control-Allow-Methods': 'GET, POST, OPTIONS',
    'Access-Control-Allow-Headers': 'Content-Type',
    'Access-Control-Max-Age': '86400',
  };
}

function json(data, status = 200, extraHeaders = {}) {
  return new Response(JSON.stringify(data), {
    status,
    headers: { 'Content-Type': 'application/json', ...extraHeaders },
  });
}

function err(msg, status = 400, extraHeaders = {}) {
  return json({ error: msg }, status, extraHeaders);
}

// ── Key Rotation ──────────────────────────────────────────────────────────────

let roundRobinIndex = 0;

function getHouseKeys(env, provider) {
  const prefix = {
    groq: 'HOUSE_GROQ_KEY_',
    gemini: 'HOUSE_GEMINI_KEY_',
    hf: 'HOUSE_HF_KEY_',
  }[provider];
  if (!prefix) return [];

  const keys = [];
  for (let i = 1; i <= 10; i++) {
    const key = env[`${prefix}${i}`];
    if (key) keys.push(key);
  }
  return keys;
}

function pickKey(keys) {
  if (!keys.length) return null;
  const idx = roundRobinIndex++ % keys.length;
  return keys[idx];
}

// ── Article Scraping ──────────────────────────────────────────────────────────

const BLOCK_TAGS = new Set([
  'script','style','noscript','nav','footer','header','aside','form',
  'button','iframe','svg','figure','figcaption','picture','source',
  'meta','link','head',
]);

const PAYWALL_SIGNALS = [
  'subscriber-only','paywall','premium-content','subscribe to read',
  'subscription required','members only','sign in to read',
  'this content is for subscribers','become a member to read',
];

function stripTags(html) {
  // Very lightweight HTML text extractor — no DOM available in Workers
  // Strips tags, decodes common entities
  return html
    .replace(/<(script|style|noscript)[^>]*>[\s\S]*?<\/\1>/gi, ' ')
    .replace(/<[^>]+>/g, ' ')
    .replace(/&amp;/g, '&').replace(/&lt;/g, '<').replace(/&gt;/g, '>')
    .replace(/&quot;/g, '"').replace(/&#39;/g, "'").replace(/&nbsp;/g, ' ')
    .replace(/\s{3,}/g, '\n\n')
    .trim();
}

function extractTitle(html) {
  // og:title → <title> → first <h1>
  let m = html.match(/<meta[^>]+property=["']og:title["'][^>]+content=["']([^"']+)["']/i)
    || html.match(/<meta[^>]+content=["']([^"']+)["'][^>]+property=["']og:title["']/i);
  if (m) return m[1].trim();

  m = html.match(/<title[^>]*>([^<]+)<\/title>/i);
  if (m) return m[1].trim();

  m = html.match(/<h1[^>]*>([^<]+)<\/h1>/i);
  if (m) return stripTags(m[1]).trim();

  return 'Untitled';
}

function extractMainContent(html) {
  // Heuristic: find the largest block of paragraph text
  // Remove block-level noise first
  let cleaned = html
    .replace(/<(nav|footer|header|aside|form|script|style|noscript)[^>]*>[\s\S]*?<\/\1>/gi, '')
    .replace(/<!--[\s\S]*?-->/g, '');

  // Try article/main tags first
  const articleMatch = cleaned.match(/<article[^>]*>([\s\S]*?)<\/article>/i)
    || cleaned.match(/<main[^>]*>([\s\S]*?)<\/main>/i)
    || cleaned.match(/<div[^>]+(?:class|id)=["'][^"']*(?:article|story|content|post|body|text)[^"']*["'][^>]*>([\s\S]*?)<\/div>/i);

  const source = articleMatch ? articleMatch[1] : cleaned;

  // Extract paragraph text
  const paragraphs = [];
  const pRe = /<p[^>]*>([\s\S]*?)<\/p>/gi;
  let pm;
  while ((pm = pRe.exec(source)) !== null) {
    const text = stripTags(pm[1]).trim();
    if (text.length > 40) paragraphs.push(text);
  }

  if (paragraphs.length > 2) return paragraphs.join('\n\n');

  // Fallback: strip all tags from source
  return stripTags(source);
}

async function scrapeArticle(url) {
  // Validate URL
  let parsed;
  try {
    parsed = new URL(url);
    if (!['http:', 'https:'].includes(parsed.protocol)) {
      return { error: 'Invalid URL — must start with http:// or https://' };
    }
  } catch {
    return { error: 'Invalid URL format' };
  }

  let response;
  try {
    response = await fetch(url, {
      headers: {
        'User-Agent': 'Mozilla/5.0 (compatible; PoliticalDebAIser/1.0)',
        'Accept': 'text/html,application/xhtml+xml,*/*',
        'Accept-Language': 'en-US,en;q=0.9',
      },
      redirect: 'follow',
      cf: { timeout: 15000 },
    });
  } catch (e) {
    return { error: `Failed to fetch article: ${e.message}` };
  }

  if (response.status === 404) return { error: 'Page not found (404)' };
  if (response.status === 403) return { error: 'Access denied (403) — the site may block scrapers' };
  if (!response.ok) return { error: `HTTP error ${response.status}` };

  const ct = response.headers.get('content-type') || '';
  if (!ct.includes('html')) return { error: `Not an HTML page (content-type: ${ct})` };

  const html = await response.text();

  // Paywall detection
  const lhtml = html.toLowerCase();
  for (const signal of PAYWALL_SIGNALS) {
    if (lhtml.includes(signal)) {
      return { error: 'Article is behind a paywall — use the "Text" tab to paste content directly' };
    }
  }

  const title = extractTitle(html);
  let body = extractMainContent(html);

  // Truncate to ~50k chars to avoid overwhelming LLMs
  if (body.length > 50000) body = body.slice(0, 50000) + '\n\n[Article truncated for analysis]';

  if (body.length < 100) {
    return { error: 'Could not extract article text — the page may be JavaScript-rendered' };
  }

  return { title, body, source_url: url };
}

// ── LLM Proxy (house keys) ────────────────────────────────────────────────────

const PROVIDER_CONFIGS = {
  groq: {
    url: 'https://api.groq.com/openai/v1/chat/completions',
    defaultModel: 'llama-3.1-8b-instant',
    envModel: 'GROQ_MODEL',
  },
  gemini: {
    url: 'https://generativelanguage.googleapis.com/v1beta/openai/chat/completions',
    defaultModel: 'gemini-2.0-flash-lite',
    envModel: 'GEMINI_MODEL',
  },
  hf: {
    url: 'https://router.huggingface.co/v1/chat/completions',
    defaultModel: 'meta-llama/Llama-3.1-8B-Instruct',
    envModel: 'HF_MODEL',
  },
};

async function proxyLlm(body, env) {
  const { provider, system_prompt, user_message } = body;

  if (!provider || !system_prompt || !user_message) {
    return { error: 'Missing provider, system_prompt, or user_message' };
  }

  const config = PROVIDER_CONFIGS[provider];
  if (!config) return { error: `Unknown provider: ${provider}` };

  const keys = getHouseKeys(env, provider);
  if (!keys.length) return { error: `No house keys configured for ${provider}` };

  const key = pickKey(keys);
  const model = env[config.envModel] || config.defaultModel;

  const payload = {
    model,
    messages: [
      { role: 'system', content: system_prompt },
      { role: 'user', content: user_message },
    ],
    stream: false,
    temperature: 0,
  };

  let response;
  try {
    response = await fetch(config.url, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        Authorization: `Bearer ${key}`,
      },
      body: JSON.stringify(payload),
    });
  } catch (e) {
    return { error: `LLM request failed: ${e.message}` };
  }

  if (response.status === 429) return { error: 'rate_limited' };
  if (!response.ok) {
    const txt = await response.text().catch(() => '');
    return { error: `Provider error ${response.status}: ${txt.slice(0, 200)}` };
  }

  const data = await response.json();
  const content = data?.choices?.[0]?.message?.content;
  if (!content) return { error: 'Empty response from LLM provider' };

  return { content };
}

// ── Request Router ────────────────────────────────────────────────────────────

export default {
  async fetch(request, env) {
    const origin = request.headers.get('Origin') || '';
    const cors = corsHeaders(origin, env);

    // CORS preflight
    if (request.method === 'OPTIONS') {
      return new Response(null, { status: 204, headers: cors });
    }

    const url = new URL(request.url);

    // Health check
    if (url.pathname === '/health') {
      return json({ status: 'ok', timestamp: Date.now() }, 200, cors);
    }

    // Scrape endpoint
    if (url.pathname === '/scrape' && request.method === 'POST') {
      let body;
      try { body = await request.json(); } catch { return err('Invalid JSON', 400, cors); }
      if (!body?.url) return err('Missing url field', 400, cors);

      const result = await scrapeArticle(body.url);
      if (result.error) return err(result.error, 422, cors);
      return json(result, 200, cors);
    }

    // LLM proxy endpoint (house keys only — user keys go direct from browser)
    if (url.pathname === '/llm' && request.method === 'POST') {
      // Validate origin for key proxy security
      const allowed = env.ALLOWED_ORIGIN;
      if (allowed && allowed !== '*' && origin !== allowed) {
        return err('Forbidden', 403, cors);
      }

      let body;
      try { body = await request.json(); } catch { return err('Invalid JSON', 400, cors); }

      const result = await proxyLlm(body, env);
      if (result.error) return json({ error: result.error }, result.error === 'rate_limited' ? 429 : 502, cors);
      return json({ content: result.content }, 200, cors);
    }

    return err('Not found', 404, cors);
  },
};
