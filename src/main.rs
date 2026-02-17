use std::collections::HashMap;
use std::sync::Arc;

use axum::{routing, Router};
use political_debaiser::{models, routes};
use tokio::sync::RwLock;
use axum::http::{HeaderValue, Method};
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load .env if present
    let _ = dotenvy::dotenv();

    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse()?))
        .init();

    let state = models::AppState {
        cache: Arc::new(RwLock::new(HashMap::new())),
        store: Arc::new(RwLock::new(HashMap::new())),
    };

    let app = Router::new()
        .route("/", routing::get(routes::index))
        .route("/analyze", routing::post(routes::analyze))
        .route("/analyze-text", routing::post(routes::analyze_text))
        .route("/health", routing::get(routes::health))
        .route("/history", routing::get(routes::list_history).post(routes::store_analysis))
        .route("/history/{id}", routing::get(routes::get_analysis).delete(routes::delete_history))
        .with_state(state)
        .nest_service("/static", ServeDir::new("static"))
        // Body size limit: 256KB max request body
        .layer(axum::extract::DefaultBodyLimit::max(256 * 1024))
        // Security headers
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::header::X_FRAME_OPTIONS,
            HeaderValue::from_static("DENY"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::header::REFERRER_POLICY,
            HeaderValue::from_static("strict-origin-when-cross-origin"),
        ))
        // CORS: restrict to same-origin by default; allow configurable origins via env
        .layer(
            CorsLayer::new()
                .allow_origin(
                    std::env::var("CORS_ORIGIN")
                        .ok()
                        .and_then(|o| HeaderValue::from_str(&o).ok())
                        .map(tower_http::cors::AllowOrigin::exact)
                        .unwrap_or_else(|| tower_http::cors::AllowOrigin::exact(
                            HeaderValue::from_static("http://localhost:3000"),
                        )),
                )
                .allow_methods([Method::GET, Method::POST, Method::DELETE, Method::OPTIONS])
                .allow_headers([axum::http::header::CONTENT_TYPE]),
        )
        .layer(TraceLayer::new_for_http());

    let addr = "0.0.0.0:3000";
    tracing::info!("PoliticalDebAIser listening on {addr}");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
