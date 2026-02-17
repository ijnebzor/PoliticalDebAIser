// =============================================================================
// Architecture: History Storage, Compare Mode, and Export (v3)
// =============================================================================
//
// ## Analysis Pipeline
//
// Both /analyze (URL) and /analyze-text (pasted text) return an AnalysisResult:
//   { title, source_url?, personas: [PersonaOutput], debiaser: DebiasedSummary }
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
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Json, Response};

use crate::archetypes;
use crate::models::{
    AnalysisRequest, AnalysisResult, AppState, ErrorResponse, HistoryListItem,
    StoredAnalysis, StoreHistoryRequest, StoreHistoryResponse,
    TextAnalysisRequest, generate_short_id,
};
use crate::scraper::{extract_from_text, scrape_article, ScrapeError};

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
            error: "Ollama request timed out".to_string(),
            details: Some(msg),
        }
    } else {
        tracing::error!("Analysis error: {msg}");
        ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            error: "Analysis failed".to_string(),
            details: Some("An internal error occurred during analysis. Check server logs for details.".to_string()),
        }
    }
}

/// GET /health — lightweight health check for Docker and monitoring.
pub async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({"status": "ok"}))
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
    let article = scrape_article(&payload.url, &state.cache).await.map_err(scrape_err)?;

    let personas = archetypes::analyze_all_personas(&article.body_text)
        .await
        .map_err(analysis_err)?;

    let debiaser = archetypes::synthesize_debiased(&personas)
        .await
        .map_err(analysis_err)?;

    Ok(Json(AnalysisResult {
        title: article.title,
        source_url: Some(payload.url),
        personas,
        debiaser,
    }))
}

/// POST /analyze-text — accepts raw article text (no URL scraping) and runs
/// the same analysis pipeline.
pub async fn analyze_text(
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

    let personas = archetypes::analyze_all_personas(&article.body_text)
        .await
        .map_err(analysis_err)?;

    let debiaser = archetypes::synthesize_debiased(&personas)
        .await
        .map_err(analysis_err)?;

    Ok(Json(AnalysisResult {
        title: article.title,
        source_url: None,
        personas,
        debiaser,
    }))
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
        store.insert(id.clone(), stored);
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
pub async fn list_history(
    State(state): State<AppState>,
) -> Json<Vec<HistoryListItem>> {
    let store = state.store.read().await;
    let mut items: Vec<HistoryListItem> = store
        .values()
        .map(|s| HistoryListItem {
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
pub async fn delete_history(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let mut store = state.store.write().await;
    if store.remove(&id).is_some() {
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
    let store = state.store.read().await;
    store.get(&id).cloned().map(Json).ok_or_else(|| ApiError {
        status: StatusCode::NOT_FOUND,
        error: "Analysis not found".to_string(),
        details: Some(format!("No stored analysis with ID '{id}'")),
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
