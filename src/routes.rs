use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Json, Response};

use crate::archetypes;
use crate::models::{
    AnalysisRequest, AnalysisResponse, ArticleCache, ErrorResponse, SynthesisRequest,
    SynthesisResponse,
};
use crate::scraper::{scrape_article, ScrapeError};

/// Structured API error that renders as JSON with an appropriate status code.
pub(crate) struct ApiError {
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

/// Map an anyhow analysis/synthesis error to a structured ApiError,
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
        ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            error: "Analysis failed".to_string(),
            details: Some(msg),
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
    State(cache): State<ArticleCache>,
    Json(payload): Json<AnalysisRequest>,
) -> Result<Json<AnalysisResponse>, ApiError> {
    let article = scrape_article(&payload.url, &cache).await.map_err(scrape_err)?;

    // Generate a neutral LLM summary of the article
    let article_summary = archetypes::summarize_article(&article.body_text)
        .await
        .unwrap_or_else(|e| {
            tracing::warn!("Article summarization failed, using truncated text: {e}");
            article.body_text.chars().take(500).collect::<String>() + "..."
        });

    let analyses = archetypes::analyze_all_archetypes(&article.body_text)
        .await
        .map_err(analysis_err)?;

    let synthesis_result = archetypes::synthesize_perspectives(&analyses).await.ok();

    let (synthesis, commonalities) = match synthesis_result {
        Some(result) => (Some(result.synthesis), Some(result.commonalities)),
        None => (None, None),
    };

    Ok(Json(AnalysisResponse {
        article_title: article.title,
        article_summary,
        analyses,
        synthesis,
        commonalities,
    }))
}

/// POST /synthesize — accepts archetype analyses and returns a unified synthesis.
pub async fn synthesize(
    Json(payload): Json<SynthesisRequest>,
) -> Result<Json<SynthesisResponse>, ApiError> {
    if payload.analyses.is_empty() {
        return Err(ApiError {
            status: StatusCode::BAD_REQUEST,
            error: "No analyses provided".to_string(),
            details: Some("The analyses array must contain at least one entry".to_string()),
        });
    }

    let result = archetypes::synthesize_perspectives(&payload.analyses)
        .await
        .map_err(analysis_err)?;

    Ok(Json(SynthesisResponse {
        synthesis: result.synthesis,
        commonalities: result.commonalities,
    }))
}
