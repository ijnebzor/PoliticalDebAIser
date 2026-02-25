# PoliticalDebAIser — Requirements Document

**Version:** 3.1
**Last Updated:** 2026-02-25
**Project Lead:** Tiny Steve the Procrastinator
**Client:** Friendji

---

## 1. Project Overview

PoliticalDebAIser (a.k.a. "Debiaser") is a Rust-based web application that analyzes news articles and political content through 8 distinct political persona lenses. Each persona provides a stance score on a Liberty-Order axis, fact-checks key claims, flags caveats, and optionally plots on a 2D Economic vs Social axis grid. A "Debiaser" engine synthesizes all persona outputs into consensus points, disagreements, bias drivers, and a truth-seeking summary.

**Reference:** `references/debiaser_webapp_MVPprototype.jsx` — Friendji's POC prototype (React/JSX). The production app layers this design over the existing Rust/Axum + Ollama infrastructure.

---

## 2. Core Requirements

### 2.1 Input
- **R-001:** User provides a URL to a news article or political content
- **R-002:** User can alternatively paste raw article text directly
- **R-003:** The application fetches and parses the article content from the URL
- **R-004:** Invalid or unreachable URLs return clear error messages

### 2.2 Political Personas (replaces Archetypes)
The system analyzes content through 8 political persona lenses:

| ID | Persona | Stance Tendency | Description |
|----|---------|-----------------|-------------|
| **R-010** | Progressive Activist | Liberty (-2.2) | Civil rights, disproportionate impacts, speech chilling effects |
| **R-011** | Liberal Social Democrat | Liberty (-1.2) | Targeted measures with safeguards, proportionality, data minimisation |
| **R-012** | Centrist Technocrat | Centre (0.1) | Measurable outcomes, KPIs, cost-benefit, sunset clauses |
| **R-013** | Libertarian, Civil Liberties | Liberty (-2.6) | Privacy as fundamental liberty, mission creep, power asymmetry |
| **R-014** | Conservative, Fiscal | Order (1.4) | Cost discipline, law-and-order, penalties for misuse |
| **R-015** | National Security Hawk | Order (2.2) | Threat landscape, intelligence gaps, rapid response |
| **R-016** | Environmentalist Green | Liberty (-0.8) | Energy footprint, activism chill, supply-chain risks |
| **R-017** | Populist, Anti-elite | Order (1.0) | Suspicious of elites, corporate capture, equal application |

### 2.3 Per-Persona Analysis Output
- **R-020:** Stance score on Liberty-Order axis (-3 to +3)
- **R-021:** Confidence score (0.0 to 1.0)
- **R-022:** 2-4 sentence summary from the persona's perspective
- **R-023:** Key claims (list of strings)
- **R-024:** Fact checks per claim: claim text, assessment (supported/contested/unsupported/unclear), rationale
- **R-025:** Caveats (list of acknowledged blind spots or biases)
- **R-026:** Optional 2D axes: economic score (-3 to +3) and social score (-3 to +3)

### 2.4 Debiaser Synthesis (replaces Synthesis)
- **R-030:** Consensus points: areas where most/all personas agree
- **R-031:** Disagreements: key areas of conflict between personas
- **R-032:** Likely bias drivers: identified framing biases in the original article
- **R-033:** Truth-seeking summary: balanced multi-paragraph analysis
- **R-034:** Spectrum score: weighted mean of persona stance scores (-3 to +3)
- **R-035:** Spectrum explanation: text explaining the placement

### 2.5 Visualisation
- **R-040:** Political spectrum bar: Liberty (-3) to Order (+3) with animated marker
- **R-041:** Disagreement meter: colour-coded bar based on std deviation of stance scores
- **R-042:** Persona clustering: group personas by agreement on the stance axis (gap-based clustering)
- **R-043:** 2D axis grid (toggle): Economic vs Social axes with persona dots and colour scale
- **R-044:** Cluster blocks: personas grouped visually with gradient banding and span info
- **R-045:** Progress loader: visible loading indicator during analysis

---

## 3. Technical Requirements

### 3.1 Backend
- **T-001:** Built in Rust using the Axum web framework
- **T-002:** Runs on port 3000 (configurable)
- **T-003:** Serves static frontend files via tower-http
- **T-004:** CORS enabled for development
- **T-005:** Combined AppState with article cache and analysis store

### 3.2 LLM Integration
- **T-010:** Uses Ollama for local model inference (switchable)
- **T-011:** Ollama URL configurable via OLLAMA_URL env var (default: http://localhost:11434)
- **T-012:** Model configurable via OLLAMA_MODEL env var (default: llama3.2)
- **T-013:** No external API keys required for base operation
- **T-014:** Structured JSON output from LLM (with fence stripping fallback)
- **T-015:** Parallel persona analysis (all 8 run concurrently)
- **T-016:** Retry logic for transient Ollama failures (up to 2 retries)

### 3.3 Content Scraping
- **T-020:** Fetches article HTML via reqwest with 30s timeout
- **T-021:** Extracts title from og:title, then `<title>`, then `<h1>` (priority order)
- **T-022:** Readability-style content extraction with scoring heuristic
- **T-023:** Strips nav/footer/sidebar/ads/scripts before extraction
- **T-024:** Content node scoring: text density + paragraph count
- **T-025:** Supports plain text input (bypass scraping)

### 3.4 API Endpoints
- **T-030:** `GET /` — Serves the main HTML page
- **T-031:** `POST /analyze` — Accepts `{"url": "..."}`, returns persona analyses + debiaser output
- **T-032:** `POST /analyze-text` — Accepts `{"text": "...", "title": "..."}`, same analysis without scraping
- **T-033:** `POST /synthesize` — Accepts persona analyses, returns debiaser synthesis
- **T-034:** `GET /static/*` — Serves CSS, JS, and other static assets
- **T-035:** `GET /health` — Health check endpoint
- **T-036:** `POST /history` — Store a completed analysis, returns short ID
- **T-037:** `GET /history` — List all stored analyses
- **T-038:** `GET /history/:id` — Retrieve stored analysis by ID
- **T-039:** `DELETE /history/:id` — Remove stored analysis

### 3.5 Frontend
- **T-040:** Single-page application (HTML/CSS/JS, no React — vanilla JS)
- **T-041:** Clean, light professional theme (white/gray, per prototype)
- **T-042:** Responsive design (mobile and desktop)
- **T-043:** 8 persona cards with persona clustering layout
- **T-044:** Animated spectrum bar (Liberty-Order axis)
- **T-045:** Disagreement meter with colour coding (low/medium/high)
- **T-046:** 2D axis grid toggle (Economic vs Social)
- **T-047:** Skeleton loading placeholders during analysis
- **T-048:** XSS protection via HTML escaping
- **T-049:** Error banners with specific messages and retry
- **T-050:** History sidebar with localStorage persistence
- **T-051:** Compare mode (side-by-side articles)
- **T-052:** Export (JSON + text) and Share Link
- **T-053:** URL and text input tabs

### 3.6 Article Summarization
- **T-060:** Articles >4000 chars summarized before persona analysis (configurable via SUMMARY_THRESHOLD env var)
- **T-061:** Summarization uses Ollama with detailed prompt preserving key facts, claims, quotes, and framing
- **T-062:** Summarization threshold configurable via SUMMARY_THRESHOLD env var (default: 4000)

### 3.7 Tone & Framing Analysis
- **T-070:** Analyzes rhetorical devices (list of strings)
- **T-071:** Detects emotional tone (string)
- **T-072:** Identifies framing strategy (string)
- **T-073:** Objectivity score (0.0 to 1.0)
- **T-074:** All analysis uses BEGIN/END ARTICLE delimiters for prompt injection mitigation

### 3.8 Source Credibility
- **T-080:** LLM-based source identification (publication name, known bias, ownership type)
- **T-081:** Scraper-based fallback with 35-publication known database
- **T-082:** Known bias labels from established media bias assessments

---

## 4. Data Structures

### 4.1 PersonaOutput (replaces ArchetypeAnalysis)
```
PersonaOutput {
  id: PersonaId,          // e.g. "progressive_activist"
  title: String,          // e.g. "Progressive Activist"
  stance_score: f64,      // -3.0 to +3.0 (Liberty-Order)
  confidence: f64,        // 0.0 to 1.0
  summary: String,        // 2-4 sentences
  key_claims: Vec<String>,
  fact_checks: Vec<FactCheck>,
  caveats: Vec<String>,
  axes: Option<Axes2D>,   // { economic: f64, social: f64 }
}
```

### 4.2 DebiasedSummary (replaces SynthesisResponse)
```
DebiasedSummary {
  consensus_points: Vec<String>,
  disagreements: Vec<String>,
  likely_bias_drivers: Vec<String>,
  truth_seeking_summary: String,
  spectrum_score: f64,     // -3.0 to +3.0
  spectrum_explain: String,
}
```

### 4.3 ToneAnalysis
```
ToneAnalysis {
  rhetorical_devices: Vec<String>,
  emotional_tone: String,
  framing_strategy: String,
  objectivity_score: f64,   // 0.0 to 1.0
}
```

### 4.4 SourceMeta
```
SourceMeta {
  publication: String,
  known_bias: Option<String>,
  ownership_type: Option<String>,
}
```

### 4.5 AnalysisResult (replaces AnalysisResponse)
```
AnalysisResult {
  title: String,
  source_url: Option<String>,
  personas: Vec<PersonaOutput>,
  debiaser: DebiasedSummary,
  tone_analysis: Option<ToneAnalysis>,
  source_meta: Option<SourceMeta>,
  warnings: Vec<String>,
}
```

---

## 5. Quality Requirements

- **Q-001:** All code compiles with zero warnings
- **Q-002:** Comprehensive test suite (unit + integration)
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
| 2026-02-17 | 2.0 | Parallel analysis, caching, Docker, frontend polish, Stage 1 features |
| 2026-02-18 | 3.0 | Major redesign: 8 personas, stance scoring, fact-checking, 2D axes, debiaser synthesis, per Friendji prototype |
| 2026-02-25 | 3.1 | Stage 3: Article summarization, tone/framing analysis, source credibility, 35-pub database |
