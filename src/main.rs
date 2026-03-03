use std::net::SocketAddr;

use axum::http::{HeaderName, HeaderValue, Method};
use axum::{Router, routing};
use political_debaiser::{models, routes};
use tower_governor::GovernorLayer;
use tower_governor::governor::GovernorConfigBuilder;
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

    let cache_size: usize = std::env::var("CACHE_SIZE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(models::DEFAULT_CACHE_SIZE);
    let store_size: usize = std::env::var("STORE_SIZE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(models::DEFAULT_STORE_SIZE);

    tracing::info!("Article cache size: {cache_size}, history store size: {store_size}");
    let state = models::AppState::new(cache_size, store_size);

    // Rate limiting: 5 requests/minute per IP on analysis endpoints
    // per_second(12) = 1 token replenished every 12s; burst_size(5) = up to 5 rapid requests
    let rate_limit_conf = GovernorConfigBuilder::default()
        .per_second(12)
        .burst_size(5)
        .finish()
        .expect("valid rate limit config");

    // Analysis endpoints with rate limiting
    let rate_limited = Router::new()
        .route("/analyze", routing::post(routes::analyze))
        .route("/analyze-text", routing::post(routes::analyze_text))
        .layer(GovernorLayer::new(rate_limit_conf));

    // All other routes — no rate limiting
    let app = Router::new()
        .route("/", routing::get(routes::index))
        .route("/health", routing::get(routes::health))
        .route("/history", routing::get(routes::list_history).post(routes::store_analysis))
        .route("/history/{id}", routing::get(routes::get_analysis).delete(routes::delete_history))
        .route("/config", routing::get(routes::get_config).post(routes::set_config))
        .merge(rate_limited)
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
        // Content-Security-Policy: restrict all resource loading to same-origin
        .layer(SetResponseHeaderLayer::overriding(
            HeaderName::from_static("content-security-policy"),
            HeaderValue::from_static("default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self'; connect-src 'self'"),
        ))
        // HSTS: enforce HTTPS for 2 years including subdomains
        .layer(SetResponseHeaderLayer::overriding(
            HeaderName::from_static("strict-transport-security"),
            HeaderValue::from_static("max-age=63072000; includeSubDomains"),
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
                .allow_headers([axum::http::header::CONTENT_TYPE, axum::http::header::AUTHORIZATION]),
        )
        .layer(TraceLayer::new_for_http());

    let addr = "0.0.0.0:3000";
    tracing::info!("PoliticalDebAIser listening on {addr}");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;

    Ok(())
}
