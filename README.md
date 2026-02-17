# PoliticalDebAIser

Analyze any news article through 5 political archetype lenses to discover balanced, multi-perspective insight. PoliticalDebAIser scrapes article content and uses a local Ollama LLM to generate analysis from Conservative, Democrat, Socialist, Dictatorship, and Anarchist viewpoints — then synthesizes a balanced overview.

## Setup

### Prerequisites

- [Rust](https://rustup.rs/) 1.85+ (edition 2024)
- [Ollama](https://ollama.ai/) running locally with a model pulled (e.g. `ollama pull llama3.2`)

### Environment

Optionally create a `.env` file in the project root to customize Ollama settings:

```
OLLAMA_URL=http://localhost:11434
OLLAMA_MODEL=llama3.2
```

Both variables have sensible defaults (`http://localhost:11434` and `llama3.2`).

### Build & Run

```bash
cargo build
cargo run
```

Then open [http://localhost:3000](http://localhost:3000) in your browser.

### Docker

```bash
docker compose up --build
```

This starts both the web app (port 3000) and an Ollama instance (port 11434). The app connects to Ollama automatically via Docker networking.

### Test

```bash
cargo test
```

## Architecture

```
src/
├── main.rs          # Axum server setup, routing, static file serving
├── models.rs        # Request/response types, ArchetypeKind enum (5 variants)
├── archetypes.rs    # Ollama LLM integration, parallel analysis, retry logic, JSON extraction
├── scraper.rs       # HTML fetching, content extraction, caching, paywall detection, truncation
└── routes.rs        # Route handlers: GET /, GET /health, POST /analyze, POST /synthesize

static/
├── index.html       # Single-page frontend
├── app.js           # Client-side JS (form handling, card rendering, API calls)
└── styles.css       # Dark-theme UI with archetype-colored cards

tests/
└── integration_tests.rs  # HTTP-level route handler tests
```

### API Endpoints

| Method | Path         | Description                                      |
|--------|--------------|--------------------------------------------------|
| GET    | `/`          | Serves the main HTML page                        |
| GET    | `/health`    | Health check — returns `{"status": "ok"}`        |
| POST   | `/analyze`   | Accepts `{"url": "..."}`, returns multi-perspective analysis with synthesis and commonalities |
| POST   | `/synthesize`| Accepts `{"analyses": [...]}`, returns balanced synthesis with cross-spectrum commonalities   |
| GET    | `/static/*`  | Serves static assets (CSS, JS)                   |

### Political Archetypes

1. **Conservative** — Limited government, free markets, traditional values
2. **Democrat** — Social justice, regulated capitalism, civil rights
3. **Socialist** — Worker ownership, class solidarity, wealth redistribution
4. **Dictatorship** — Centralized authority, national strength, social order
5. **Anarchist** — Abolition of hierarchy, mutual aid, direct action

### Key Features

- **Parallel analysis** — All 5 archetype analyses run concurrently via `tokio::spawn`
- **Article caching** — Scraped articles are cached in-memory to avoid redundant fetches
- **Paywall detection** — Detects common paywall indicators and returns a clear error
- **Content truncation** — Articles over 50,000 characters are truncated to avoid overwhelming the LLM
- **Retry logic** — Ollama requests retry up to 3 times on connection errors or 5xx responses
- **JSON extraction** — Strips markdown code fences from LLM responses for reliable parsing
- **Neutral summaries** — Generates an objective article summary alongside the archetype analyses
- **Cross-spectrum commonalities** — Identifies points where multiple political perspectives agree
- **Structured errors** — All API errors return JSON with `error` and `details` fields plus appropriate HTTP status codes
- **Docker support** — Multi-stage Dockerfile + docker-compose with Ollama service
