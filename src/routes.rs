// =============================================================================
// Architecture: History Storage, Compare Mode, and Export (v3)
// =============================================================================
//
// ## Analysis Pipeline
//
// Both /analyze (URL) and /analyze-text (pasted text) return an AnalysisResult:
//   { title, source_url?, personas, debiaser, tone_analysis?, source_meta? }
//
// The debiaser synthesis is built into the analysis pipeline — no separate
// /synthesize endpoint is needed.
//
// ## History Storage
//
// Analysis results can be stored server-side for sharing via short URLs.
// The flow is:
//
//   1. Client runs /analyze or /analyze-text → gets AnalysisResult
//   2. Client POSTs the result to /history → gets back { id, share_url }
//   3. Anyone with the ID can GET /history/:id to retrieve the full analysis
//
// Server storage is in-memory (HashMap behind Arc<RwLock>). For persistence,
// swap AnalysisStore with a database-backed implementation later.
//
// ## Compare Mode
//
// Compare mode is purely frontend. The client holds two AnalysisResult objects
// (from localStorage, from /history/:id, or from fresh analyses) and renders
// them side-by-side.
//
// ## Export Formats
//
// JSON export: the AnalysisResult struct (Serialize + Deserialize for
// round-tripping through JSON export/import).
//
// =============================================================================

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Json, Response};

use crate::archetypes;
use crate::llm;
use crate::models::{
    AnalysisRequest, AnalysisResult, AppState, ConfigRequest, ConfigResponse, ErrorResponse,
    HistoryListItem, SourceMeta, StoreHistoryRequest, StoreHistoryResponse, StoredAnalysis,
    TextAnalysisRequest, generate_short_id,
};
use crate::scraper::{ScrapeError, extract_from_text, extract_source_meta, scrape_article};

/// Structured API error that renders as JSON with an appropriate status code.
pub struct ApiError {
    status: StatusCode,
    error: String,
    details: Option<String>,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = Json(ErrorResponse {
            error: self.error,
            details: self.details,
        });
        (self.status, body).into_response()
    }
}

/// Map a ScrapeError to a structured ApiError with the right HTTP status.
fn scrape_err(e: ScrapeError) -> ApiError {
    match &e {
        ScrapeError::InvalidUrl(_) => ApiError {
            status: StatusCode::BAD_REQUEST,
            error: "Invalid URL".to_string(),
            details: Some(e.to_string()),
        },
        ScrapeError::Timeout(_) => ApiError {
            status: StatusCode::GATEWAY_TIMEOUT,
            error: "Article fetch timed out".to_string(),
            details: Some(e.to_string()),
        },
        ScrapeError::FetchFailed(_) => ApiError {
            status: StatusCode::BAD_REQUEST,
            error: "Failed to fetch article".to_string(),
            details: Some(e.to_string()),
        },
        ScrapeError::NotFound => ApiError {
            status: StatusCode::BAD_REQUEST,
            error: "Page not found".to_string(),
            details: Some("The URL returned a 404 — the article may have been removed or the URL is incorrect".to_string()),
        },
        ScrapeError::Paywall => ApiError {
            status: StatusCode::BAD_REQUEST,
            error: "Article behind paywall".to_string(),
            details: Some("The article appears to be behind a paywall and its full content cannot be accessed".to_string()),
        },
        ScrapeError::NotHtml(_) => ApiError {
            status: StatusCode::BAD_REQUEST,
            error: "Not an HTML page".to_string(),
            details: Some(e.to_string()),
        },
        ScrapeError::EmptyContent => ApiError {
            status: StatusCode::BAD_REQUEST,
            error: "Empty article content".to_string(),
            details: Some("The page was fetched but no article text could be extracted".to_string()),
        },
    }
}

/// Map an anyhow analysis error to a structured ApiError,
/// distinguishing Ollama connection issues from other failures.
fn analysis_err(e: anyhow::Error) -> ApiError {
    let msg = e.to_string();
    if msg.contains("connection refused") || msg.contains("Connection refused") {
        ApiError {
            status: StatusCode::BAD_GATEWAY,
            error: "Ollama is unavailable".to_string(),
            details: Some("Could not connect to Ollama. Is it running?".to_string()),
        }
    } else if msg.contains("timed out") || msg.contains("Timeout") {
        ApiError {
            status: StatusCode::GATEWAY_TIMEOUT,
            error: "Analysis request timed out".to_string(),
            details: Some(
                "The LLM provider did not respond in time. Please try again.".to_string(),
            ),
        }
    } else {
        tracing::error!("Analysis error: {msg}");
        ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            error: "Analysis failed".to_string(),
            details: Some(
                "An internal error occurred during analysis. Check server logs for details."
                    .to_string(),
            ),
        }
    }
}

/// GET /health — enhanced health check for Docker and monitoring.
pub async fn health(State(state): State<AppState>) -> Json<serde_json::Value> {
    let uptime_secs = state.uptime_secs();
    let history_count = state.store.read().await.len();

    // Report which LLM providers are configured (names only, never values)
    let mut providers = Vec::new();
    if std::env::var("OLLAMA_URL").is_ok() || std::env::var("OLLAMA_MODEL").is_ok() {
        providers.push("ollama");
    }
    if std::env::var("GROQ_API_KEY").is_ok() {
        providers.push("groq");
    }
    if std::env::var("GEMINI_API_KEY").is_ok() {
        providers.push("gemini");
    }
    if std::env::var("HF_API_KEY").is_ok() {
        providers.push("huggingface");
    }

    Json(serde_json::json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
        "uptime_secs": uptime_secs,
        "history_count": history_count,
        "providers": providers,
    }))
}

/// GET /metrics — basic operational statistics.
pub async fn metrics(State(state): State<AppState>) -> Json<serde_json::Value> {
    let uptime_secs = state.uptime_secs();
    let total_requests = state
        .total_requests
        .load(std::sync::atomic::Ordering::Relaxed);
    let total_analyses = state
        .total_analyses
        .load(std::sync::atomic::Ordering::Relaxed);
    let history_count = state.store.read().await.len();
    let cache_count = state.cache.read().await.len();

    Json(serde_json::json!({
        "uptime_secs": uptime_secs,
        "total_requests": total_requests,
        "total_analyses": total_analyses,
        "history_count": history_count,
        "cache_count": cache_count,
    }))
}

/// GET / — serves the main page from static/index.html (baked in at compile time).
pub async fn index() -> Html<&'static str> {
    Html(include_str!("../static/index.html"))
}

/// POST /analyze — accepts a URL and returns multi-perspective analysis.
pub async fn analyze(
    State(state): State<AppState>,
    Json(payload): Json<AnalysisRequest>,
) -> Result<Json<AnalysisResult>, ApiError> {
    let article = scrape_article(&payload.url, &state.cache)
        .await
        .map_err(scrape_err)?;

    let mut result =
        archetypes::analyze_full(&article.body_text, &article.title, Some(&payload.url))
            .await
            .map_err(analysis_err)?;

    // If LLM-based source credibility failed, fall back to domain-based metadata
    if result.source_meta.is_none() {
        let scraped = extract_source_meta(&payload.url);
        result.source_meta = Some(SourceMeta {
            publication: scraped.publication,
            known_bias: scraped.known_bias,
            ownership_type: scraped.media_type,
        });
    }

    state.inc_analyses();
    Ok(Json(result))
}

/// POST /analyze-text — accepts raw article text (no URL scraping) and runs
/// the same analysis pipeline.
pub async fn analyze_text(
    State(state): State<AppState>,
    Json(payload): Json<TextAnalysisRequest>,
) -> Result<Json<AnalysisResult>, ApiError> {
    if payload.text.trim().is_empty() {
        return Err(ApiError {
            status: StatusCode::BAD_REQUEST,
            error: "Empty text".to_string(),
            details: Some("The text field must not be empty".to_string()),
        });
    }

    // Limit text length to prevent abuse — 100K chars is generous for any article
    const MAX_TEXT_LENGTH: usize = 100_000;
    if payload.text.len() > MAX_TEXT_LENGTH {
        return Err(ApiError {
            status: StatusCode::BAD_REQUEST,
            error: "Text too long".to_string(),
            details: Some(format!("Text must be under {MAX_TEXT_LENGTH} characters")),
        });
    }

    let article = extract_from_text(&payload.text, payload.title.as_deref());

    let result = archetypes::analyze_full(&article.body_text, &article.title, None)
        .await
        .map_err(analysis_err)?;

    state.inc_analyses();
    Ok(Json(result))
}

/// POST /history — store a completed analysis, return { id, share_url }.
pub async fn store_analysis(
    State(state): State<AppState>,
    Json(payload): Json<StoreHistoryRequest>,
) -> Result<(StatusCode, Json<StoreHistoryResponse>), ApiError> {
    let now = chrono_now();
    let id = generate_short_id(&payload.source_url, &now);

    let stored = StoredAnalysis {
        id: id.clone(),
        created_at: now,
        source_url: payload.source_url,
        response: payload.result,
    };

    {
        let mut store = state.store.write().await;
        store.put(id.clone(), stored);
    }

    Ok((
        StatusCode::CREATED,
        Json(StoreHistoryResponse {
            id: id.clone(),
            share_url: format!("/history/{id}"),
        }),
    ))
}

/// GET /history — list all stored analyses (summary only).
pub async fn list_history(State(state): State<AppState>) -> Json<Vec<HistoryListItem>> {
    let store = state.store.read().await;
    let mut items: Vec<HistoryListItem> = store
        .iter()
        .map(|(_, s)| HistoryListItem {
            id: s.id.clone(),
            article_title: s.response.title.clone(),
            source_url: s.source_url.clone(),
            created_at: s.created_at.clone(),
        })
        .collect();
    // Sort by created_at descending (newest first)
    items.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Json(items)
}

/// DELETE /history/:id — remove a stored analysis.
/// Requires bearer token authentication via CONFIG_AUTH_TOKEN env var.
pub async fn delete_history(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    check_config_auth(&headers)?;

    let mut store = state.store.write().await;
    if store.pop(&id).is_some() {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError {
            status: StatusCode::NOT_FOUND,
            error: "Analysis not found".to_string(),
            details: Some(format!("No stored analysis with ID '{id}'")),
        })
    }
}

/// GET /history/:id — retrieve a previously stored analysis by its short ID.
pub async fn get_analysis(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<StoredAnalysis>, ApiError> {
    let mut store = state.store.write().await;
    store.get(&id).cloned().map(Json).ok_or_else(|| ApiError {
        status: StatusCode::NOT_FOUND,
        error: "Analysis not found".to_string(),
        details: Some(format!("No stored analysis with ID '{id}'")),
    })
}

/// Check bearer token authentication against CONFIG_AUTH_TOKEN env var.
/// Returns Ok(()) if authenticated, or an ApiError if not.
fn check_config_auth(headers: &HeaderMap) -> Result<(), ApiError> {
    let expected = match std::env::var("CONFIG_AUTH_TOKEN") {
        Ok(token) if !token.is_empty() => token,
        _ => {
            return Err(ApiError {
                status: StatusCode::FORBIDDEN,
                error: "Configuration locked".to_string(),
                details: Some("CONFIG_AUTH_TOKEN is not configured on the server".to_string()),
            });
        }
    };

    let auth_header = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    if let Some(token) = auth_header.strip_prefix("Bearer ")
        && token == expected
    {
        return Ok(());
    }

    Err(ApiError {
        status: StatusCode::UNAUTHORIZED,
        error: "Unauthorized".to_string(),
        details: Some("Invalid or missing bearer token".to_string()),
    })
}

/// POST /config — store runtime LLM API keys (not persisted to disk).
/// Only accepts known key names to prevent arbitrary data injection.
/// Requires bearer token authentication via CONFIG_AUTH_TOKEN env var.
pub async fn set_config(
    headers: HeaderMap,
    Json(payload): Json<ConfigRequest>,
) -> Result<StatusCode, ApiError> {
    check_config_auth(&headers)?;

    let mut keys = std::collections::HashMap::new();

    if let Some(key) = payload.groq_api_key {
        keys.insert("GROQ_API_KEY".to_string(), key);
    }
    if let Some(key) = payload.gemini_api_key {
        keys.insert("GEMINI_API_KEY".to_string(), key);
    }
    if let Some(key) = payload.hf_api_key {
        keys.insert("HF_API_KEY".to_string(), key);
    }

    llm::set_runtime_keys(keys).await;
    Ok(StatusCode::NO_CONTENT)
}

/// GET /config — check which API keys are currently configured (no values exposed).
pub async fn get_config() -> Json<ConfigResponse> {
    let names = llm::get_runtime_key_names().await;
    Json(ConfigResponse {
        groq_configured: names.contains(&"GROQ_API_KEY".to_string())
            || std::env::var("GROQ_API_KEY").is_ok(),
        gemini_configured: names.contains(&"GEMINI_API_KEY".to_string())
            || std::env::var("GEMINI_API_KEY").is_ok(),
        hf_configured: names.contains(&"HF_API_KEY".to_string())
            || std::env::var("HF_API_KEY").is_ok(),
    })
}

/// Simple ISO-8601-ish timestamp without pulling in chrono.
fn chrono_now() -> String {
    use std::time::SystemTime;
    let d = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = d.as_secs();
    format!("{secs}")
}
