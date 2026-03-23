# PoliticalDebAIser

> **BETA** — This project is under active development.

Multi-perspective political news analysis tool. Paste a URL or article text, and 8 political personas analyze it from different viewpoints. A debiaser engine then synthesizes consensus points, disagreements, bias drivers, and a truth-seeking summary.

Built with Rust/Axum, powered by local or cloud LLM inference.

## Features

- **8 Political Personas** — Progressive Activist, Liberal Social Democrat, Centrist Technocrat, Libertarian Civil Liberties, Conservative Fiscal, National Security Hawk, Environmentalist Green, Populist Anti-Elite
- **Debiaser Synthesis** — Consensus points, disagreements, bias drivers, and balanced truth-seeking summary
- **Political Spectrum** — Liberty-Order axis (-3 to +3) with animated visualization
- **2D Axis Grid** — Economic vs Social axes with persona dot plotting
- **Disagreement Meter** — Color-coded bar based on standard deviation of stance scores
- **Persona Clustering** — Gap-based grouping of personas by agreement
- **Fact Checking** — Per-claim assessment (supported/contested/unsupported/unclear)
- **Tone & Framing Analysis** — Detects framing biases in the original article
- **Source Credibility** — Metadata extraction and credibility signals
- **Multi-Provider LLM** — Groq, Gemini, HuggingFace, and Ollama with round-robin load balancing
- **Analysis History** — Store, retrieve, and share analyses via short URLs
- **Rate Limiting** — Per-IP rate limiting on analysis endpoints
- **Security Hardened** — OWASP Top 10 compliant, SSRF protection, CSP, HSTS, XSS prevention
- **Docker Support** — Multi-stage Dockerfile with docker-compose
- **CI/CD** — GitHub Actions pipeline (fmt, clippy, build, test, security audit, coverage, Docker validation)

## Screenshots

<!-- TODO: Add screenshots -->
![Analysis View](docs/screenshots/analysis-view.png)
![Spectrum Visualization](docs/screenshots/spectrum.png)
![2D Axis Grid](docs/screenshots/axis-grid.png)

## Quick Start

### Prerequisites

- [Rust](https://rustup.rs/) 1.85+ (edition 2024)
- [Ollama](https://ollama.ai/) running locally with a model pulled (e.g. `ollama pull llama3.2`)

### Build & Run

```bash
cargo build
cargo run
```

Open [http://localhost:3000](http://localhost:3000) in your browser.

### Docker

```bash
docker compose up --build
```

This starts the web app on port 3000 and an Ollama instance on port 11434, connected via Docker networking.

### Run Tests

```bash
cargo test           # Run all tests
cargo clippy         # Lint
cargo fmt --check    # Check formatting
```

## Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `OLLAMA_URL` | `http://localhost:11434` | Ollama API endpoint |
| `OLLAMA_MODEL` | `llama3.2` | Ollama model name |
| `LLM_PROVIDERS` | — | Comma-separated provider list (e.g. `groq,gemini,ollama`) |
| `GROQ_API_KEY` | — | API key for Groq provider |
| `GEMINI_API_KEY` | — | API key for Gemini provider |
| `HF_API_KEY` | — | API key for HuggingFace provider |
| `CONFIG_AUTH_TOKEN` | — | Bearer token for `/config` and `/history` DELETE endpoints |
| `LOG_FORMAT` | `text` | Set to `json` for structured JSON logging |
| `RUST_LOG` | `info` | Log level filter (e.g. `debug`, `info,tower_http=trace`) |
| `CORS_ORIGIN` | `http://localhost:3000` | Allowed CORS origin |
| `CACHE_SIZE` | `100` | Max cached articles |
| `STORE_SIZE` | `1000` | Max stored analysis results |
| `RESPONSE_CACHE_SIZE` | `200` | Max cached analysis responses |
| `CACHE_TTL_SECS` | `3600` | Response cache TTL in seconds |
| `RATE_LIMIT_RPM` | `60` | Per-IP rate limit (requests per minute) on analysis endpoints |

Create a `.env` file in the project root to set these, or pass them as environment variables.

## API Endpoints

| Method | Path | Description |
|--------|------|-------------|
| `POST` | `/analyze` | Analyze article by URL. Body: `{"url": "..."}` |
| `POST` | `/analyze-text` | Analyze pasted text. Body: `{"text": "...", "title": "..."}` |
| `GET` | `/health` | Health check — version, uptime, provider status, history count |
| `GET` | `/metrics` | Metrics — total requests, analyses, cache count, uptime |
| `GET` | `/history` | List stored analyses |
| `POST` | `/history` | Store an analysis result. Returns `{id, share_url}` |
| `GET` | `/history/{id}` | Retrieve a stored analysis |
| `GET` | `/history/search?q=` | Search history by title (case-insensitive substring) |
| `DELETE` | `/history/{id}` | Delete a stored analysis (requires auth) |
| `GET` | `/config` | Get runtime configuration (requires auth) |
| `POST` | `/config` | Update runtime configuration (requires auth) |

## Architecture

```
src/
├── main.rs          # Axum server, middleware, security headers
├── lib.rs           # Module exports
├── models.rs        # Types: PersonaId, PersonaOutput, AnalysisResult, AppState
├── archetypes.rs    # 8 persona prompts, parallel analysis, debiaser synthesis
├── routes.rs        # API handlers, SSRF protection, error handling
├── scraper.rs       # HTML extraction, readability scoring, archive.ph fallback
├── llm.rs           # Multi-provider LLM client (Groq, Gemini, HuggingFace, Ollama)
└── summarizer.rs    # Article summarization for token optimization

static/
├── index.html       # Single-page frontend
├── app.js           # Client-side JS
└── styles.css       # Dark theme UI (ijneb.dev design language)

tests/
├── integration_tests.rs     # Integration tests
├── e2e_tests.rs             # E2E tests (security + feature coverage)
├── consistency_tests.rs     # Cross-provider consistency tests
└── llm_provider_tests.rs    # Provider-specific tests
```

## Tech Stack

- **Language:** Rust (2024 edition)
- **Web Framework:** [Axum](https://github.com/tokio-rs/axum) 0.8
- **Async Runtime:** [Tokio](https://tokio.rs/)
- **LLM Inference:** [Ollama](https://ollama.ai/) (local), [Groq](https://groq.com/), [Gemini](https://ai.google.dev/), [HuggingFace](https://huggingface.co/)
- **HTTP Client:** reqwest
- **Rate Limiting:** tower-governor
- **Frontend:** Vanilla HTML/CSS/JS (no framework)
- **Containerization:** Docker + docker-compose
- **CI/CD:** GitHub Actions

## CI/CD Pipeline

The GitHub Actions pipeline runs on every push and PR to master:

| Job | Steps | Purpose |
|-----|-------|---------|
| **ci** | `cargo fmt --check`, `cargo clippy`, `cargo build`, `cargo test`, `cargo audit` | Code quality, correctness, and dependency CVE scanning |
| **coverage** | `cargo tarpaulin` | Test coverage reporting (uploaded as artifact) |
| **docker** | Docker Buildx build | Validates container builds successfully |

## Contributing

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/my-feature`)
3. Ensure all tests pass (`cargo test`)
4. Ensure no lint warnings (`cargo clippy -- -D warnings`)
5. Ensure formatting is correct (`cargo fmt --check`)
6. Commit your changes
7. Open a pull request

## License

This project is licensed under the MIT License. See [LICENSE](LICENSE) for details.
