# PoliticalDebAIser — Requirements Document

**Version:** 2.0
**Last Updated:** 2026-02-17
**Project Lead:** Tiny Steve the Procrastinator
**Client:** Friendji

---

## 1. Project Overview

PoliticalDebAIser is a Rust-based web application that analyzes news articles and political content through 5 distinct political archetype lenses. It provides users with diverse ideological perspectives on any given article, highlights commonalities and differences, and offers a balanced synthesis.

---

## 2. Core Requirements

### 2.1 Input
- **R-001:** User provides a URL to a news article or political content
- **R-002:** The application fetches and parses the article content from the URL
- **R-003:** Invalid or unreachable URLs return clear error messages

### 2.2 Political Archetypes
The system analyzes content through 5 political perspectives:

| ID | Archetype | Description |
|----|-----------|-------------|
| **R-010** | Conservative | Limited government, free markets, traditional values, fiscal responsibility |
| **R-011** | Democrat | Progressive governance, social equality, regulated capitalism, civil rights |
| **R-012** | Socialist | Worker ownership, class solidarity, wealth redistribution, public services |
| **R-013** | Dictatorship | Centralized authority, national unity, state-directed planning, social order |
| **R-014** | Anarchist | Abolition of state, voluntary association, mutual aid, decentralization |

### 2.3 Analysis Output
- **R-020:** Each archetype produces a 2-3 sentence summary from its perspective
- **R-021:** Each archetype identifies 3-5 key highlights/talking points
- **R-022:** Each archetype rates the article's alignment to its values (0-100% score)
- **R-023:** The system identifies commonalities shared across perspectives
- **R-024:** A "Synthesize All Perspectives" button generates a balanced, unbiased take

### 2.4 Synthesis
- **R-030:** Synthesis identifies areas of agreement and disagreement across archetypes
- **R-031:** Synthesis highlights key tensions and trade-offs
- **R-032:** Synthesis is non-partisan and does not favor any single perspective
- **R-033:** Synthesis is generated on-demand (user clicks the button)

---

## 3. Technical Requirements

### 3.1 Backend
- **T-001:** Built in Rust using the Axum web framework
- **T-002:** Runs on port 3000 (configurable)
- **T-003:** Serves static frontend files via tower-http
- **T-004:** CORS enabled for development

### 3.2 LLM Integration
- **T-010:** Uses Ollama for local model inference (switchable)
- **T-011:** Ollama URL configurable via OLLAMA_URL env var (default: http://localhost:11434)
- **T-012:** Model configurable via OLLAMA_MODEL env var (default: llama3.2)
- **T-013:** No external API keys required for base operation

### 3.3 Content Scraping
- **T-020:** Fetches article HTML via reqwest with timeout
- **T-021:** Extracts title from `<title>` tag with `<h1>` fallback
- **T-022:** Extracts body text from `<article>`, `<main>`, or `<p>` tags
- **T-023:** Strips HTML, collapses whitespace, returns clean text
- **T-024:** Extracts meta description when available

### 3.4 API Endpoints
- **T-030:** `GET /` — Serves the main HTML page
- **T-031:** `POST /analyze` — Accepts `{"url": "..."}`, returns archetype analyses
- **T-032:** `POST /synthesize` — Accepts analyses, returns balanced synthesis
- **T-033:** `GET /static/*` — Serves CSS, JS, and other static assets

### 3.5 Frontend
- **T-040:** Single-page application (HTML/CSS/JS)
- **T-041:** Dark professional theme
- **T-042:** Responsive design (mobile and desktop)
- **T-043:** 5 color-coded archetype cards in a grid layout
- **T-044:** Animated alignment score bars
- **T-045:** Loading states with skeleton placeholders
- **T-046:** XSS protection via HTML escaping
- **T-047:** Error banners with specific messages

---

## 4. v2 Enhancements (In Progress)

### 4.1 Performance
- **V2-001:** Parallel archetype analysis (all 5 run concurrently)
- **V2-002:** In-memory article caching (avoid re-scraping same URL)
- **V2-003:** Retry logic for transient Ollama failures (up to 2 retries)

### 4.2 Reliability
- **V2-010:** Structured JSON error responses with appropriate HTTP status codes
- **V2-011:** Request timeout on article fetching (30s)
- **V2-012:** JSON extraction fallback (strip markdown code fences from LLM responses)

### 4.3 Deployment
- **V2-020:** Dockerfile with multi-stage build
- **V2-021:** docker-compose.yml with app + Ollama services
- **V2-022:** .dockerignore for clean builds

### 4.4 Frontend Polish
- **V2-030:** Skeleton loading cards with shimmer animation
- **V2-031:** Fade-in animations on card appearance
- **V2-032:** Copy-to-clipboard on synthesis result
- **V2-033:** Retry button on errors
- **V2-034:** Specific error messages (Ollama down, invalid URL, parse failure)

---

## 5. Quality Requirements

- **Q-001:** All code compiles with zero warnings
- **Q-002:** Minimum 36 tests passing (unit + integration)
- **Q-003:** No raw `unwrap()` on network operations
- **Q-004:** Proper error propagation with `anyhow::Context`
- **Q-005:** XSS protection on all user-facing content

---

## 6. Environment Setup

```bash
# Required
- Rust toolchain (cargo)
- Ollama running locally with a model pulled

# Environment variables (.env)
OLLAMA_URL=http://localhost:11434    # Ollama server URL
OLLAMA_MODEL=llama3.2               # Model to use for analysis
```

---

## 7. Change Log

| Date | Version | Changes |
|------|---------|---------|
| 2026-02-17 | 1.0 | Initial project — 5 archetypes, Anthropic Claude API, basic UI |
| 2026-02-17 | 1.1 | Switched from Anthropic API to Ollama local model inference |
| 2026-02-17 | 2.0 (WIP) | Parallel analysis, caching, Docker, frontend polish |
