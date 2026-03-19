# PoliticalDebAIser — Deployment Guide

## Architecture

```
Browser (GitHub/Cloudflare Pages)
  │
  ├── engine.js  ← All LLM calls run here (8 personas + synthesis + tone + source meta)
  │     │
  │     ├── User's API key (stored in localStorage) → direct to Groq/Gemini/HF
  │     └── No user key → proxy to Cloudflare Worker → house keys (server-side)
  │
  └── app.js    ← All UI logic
        │
        └── /scrape → Cloudflare Worker (fetches articles, bypasses CORS)

Cloudflare Worker
  ├── POST /scrape  — fetch + extract article text from any URL
  ├── POST /llm     — proxy LLM calls using server-side house keys
  └── GET  /health  — health check
```

**Security model:**
- House API keys live ONLY in the Worker (set as Cloudflare env vars, never in code)
- User keys live ONLY in their browser localStorage
- No database, no server, no cost beyond API usage

---

## Step 1: Deploy the Cloudflare Worker

### Prerequisites
- [Cloudflare account](https://cloudflare.com) (free)
- [Node.js](https://nodejs.org) installed
- `npm install -g wrangler`

### Deploy

```bash
cd worker/
wrangler login
wrangler deploy
```

Note the Worker URL — it looks like:
`https://political-debaiser-worker.YOUR_SUBDOMAIN.workers.dev`

### Set your house keys (in Cloudflare Dashboard)

Go to: Workers & Pages → political-debaiser-worker → Settings → Variables

Add these (mark as **Secret**):
```
HOUSE_GROQ_KEY_1     = gsk_...      # Your first Groq key
HOUSE_GROQ_KEY_2     = gsk_...      # Second Groq key (optional, for rotation)
HOUSE_GEMINI_KEY_1   = AIza...      # Your Gemini key
HOUSE_HF_KEY_1       = hf_...       # HuggingFace key (optional)
ALLOWED_ORIGIN       = https://your-site.pages.dev   # Set after Pages deploy
```

**Adding more keys for rotation:** Just add `HOUSE_GROQ_KEY_3`, `HOUSE_GROQ_KEY_4` etc.
The Worker automatically round-robins across all keys it finds (up to 10 per provider).

---

## Step 2: Configure the Frontend

Edit `site/index.html` — find this line near the bottom and update it:

```html
<script>
  window.WORKER_URL = 'https://political-debaiser-worker.YOUR_SUBDOMAIN.workers.dev';
</script>
```

Replace `YOUR_SUBDOMAIN` with your actual Cloudflare subdomain.

---

## Step 3: Deploy to Cloudflare Pages (recommended) or GitHub Pages

### Option A: Cloudflare Pages (recommended — same ecosystem, better perf)

```bash
cd site/
wrangler pages deploy . --project-name political-debaiser
```

Or connect your GitHub repo in the Cloudflare Dashboard:
- Pages → Create a project → Connect to Git
- Build command: (leave empty — it's static)
- Build output directory: `site`

### Option B: GitHub Pages

1. Push the `site/` directory contents to a `gh-pages` branch
2. Enable Pages in repo Settings → Pages → Source: `gh-pages` branch

Or use GitHub Actions — add `.github/workflows/deploy.yml`:

```yaml
name: Deploy to GitHub Pages
on:
  push:
    branches: [main]
jobs:
  deploy:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Deploy
        uses: peaceiris/actions-gh-pages@v3
        with:
          github_token: ${{ secrets.GITHUB_TOKEN }}
          publish_dir: ./site
```

---

## Step 4: Update ALLOWED_ORIGIN

Once your Pages site is live, go back to the Worker variables and set:
```
ALLOWED_ORIGIN = https://your-site.pages.dev
```
(Or your custom domain if you set one up.)

---

## Getting Free API Keys

| Provider | Free tier | Sign up |
|----------|-----------|---------|
| **Groq** | ~14,400 req/day on free tier | https://console.groq.com |
| **Gemini** | 1,500 req/day free | https://aistudio.google.com |
| **HuggingFace** | Limited free inference | https://huggingface.co |

Recommended for beta: **Groq** (fastest, most generous free tier).
Add 2-3 Groq keys with different accounts for rotation headroom.

---

## How Key Rotation Works

1. Browser checks if user has their own key for any provider → uses it directly
2. If no user key, calls `Worker /llm` with provider name
3. Worker has `HOUSE_GROQ_KEY_1`, `HOUSE_GROQ_KEY_2` etc. → picks one via round-robin
4. If provider returns 429 (rate limited) → automatically falls back to next provider
5. Provider order: Groq → Gemini → HuggingFace (configurable in `engine.js`)

---

## File Structure

```
debaiser-deploy/
├── site/                    ← Deploy this to Pages
│   ├── index.html           ← Main page (set WORKER_URL here)
│   ├── engine.js            ← LLM engine (all inference logic)
│   ├── app.js               ← UI logic
│   ├── styles.css           ← Styles (unchanged from original)
│   └── fonts/               ← Font files
└── worker/                  ← Deploy this to Cloudflare Workers
    ├── index.js             ← Worker code (scraping + LLM proxy)
    └── wrangler.toml        ← Worker config
```

---

## Keeping the Rust Backend

The Rust backend still works and can be deployed separately (Railway, Fly.io etc.)
if you want server-side analysis as well. The browser frontend and Rust backend
are now independent — use whichever fits your needs.
