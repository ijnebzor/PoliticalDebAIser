mod archetypes;
mod models;
mod routes;
mod scraper;

use std::collections::HashMap;
use std::sync::Arc;

use axum::{routing, Router};
use tokio::sync::RwLock;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;
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

    let cache: models::ArticleCache = Arc::new(RwLock::new(HashMap::new()));

    let app = Router::<models::ArticleCache>::new()
        .route("/", routing::get(routes::index))
        .route("/analyze", routing::post(routes::analyze))
        .route("/synthesize", routing::post(routes::synthesize))
        .route("/health", routing::get(routes::health))
        .with_state(cache)
        .nest_service("/static", ServeDir::new("static"))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http());

    let addr = "0.0.0.0:3000";
    tracing::info!("PoliticalDebAIser listening on {addr}");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
