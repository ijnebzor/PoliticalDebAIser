use std::net::SocketAddr;

use axum::extract::State;
use axum::http::{HeaderName, HeaderValue, Method};
use axum::middleware::{self, Next};
use axum::{Router, routing};
use political_debaiser::{models, routes};
use tower_governor::GovernorLayer;
use tower_governor::governor::GovernorConfigBuilder;
use tower_http::compression::CompressionLayer;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

/// Middleware that increments the total request counter.
async fn count_requests(
    State(state): State<models::AppState>,
    req: axum::http::Request<axum::body::Body>,
    next: Next,
) -> axum::response::Response {
    state.inc_requests();
    next.run(req).await
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load .env if present
    let _ = dotenvy::dotenv();

    // Initialize tracing — JSON format for production, human-readable for development
    let env_filter = EnvFilter::from_default_env().add_directive("info".parse()?);
    let log_format = std::env::var("LOG_FORMAT").unwrap_or_default();
    if log_format == "json" {
        tracing_subscriber::fmt()
            .json()
            .with_env_filter(env_filter)
            .init();
    } else {
        tracing_subscriber::fmt().with_env_filter(env_filter).init();
    }

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

    // Rate limiting: configurable via RATE_LIMIT_RPM (default 60 requests/minute per IP)
    let rpm: u64 = std::env::var("RATE_LIMIT_RPM")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(60);
    let interval_ms = 60_000 / rpm.max(1);
    let burst = (rpm / 6).max(1) as u32;
    tracing::info!("Rate limit: {rpm} RPM per IP (interval={interval_ms}ms, burst={burst})");

    let global_rate_conf = GovernorConfigBuilder::default()
        .per_millisecond(interval_ms)
        .burst_size(burst)
        .finish()
        .expect("valid rate limit config");

    // Stricter rate limit on analysis endpoints: 5 requests/minute per IP
    let analysis_rate_conf = GovernorConfigBuilder::default()
        .per_second(12)
        .burst_size(5)
        .finish()
        .expect("valid analysis rate limit config");

    // Analysis endpoints with strict rate limiting
    let rate_limited = Router::new()
        .route("/analyze", routing::post(routes::analyze))
        .route("/analyze-text", routing::post(routes::analyze_text))
        .layer(GovernorLayer::new(analysis_rate_conf));

    // All other routes — no rate limiting
    let app = Router::new()
        .route("/", routing::get(routes::index))
        .route("/health", routing::get(routes::health))
        .route("/metrics", routing::get(routes::metrics))
        .route("/history", routing::get(routes::list_history).post(routes::store_analysis))
        .route("/history/search", routing::get(routes::search_history))
        .route("/history/{id}", routing::get(routes::get_analysis).delete(routes::delete_history))
        .route("/config", routing::get(routes::get_config).post(routes::set_config))
        .merge(rate_limited)
        .layer(middleware::from_fn_with_state(state.clone(), count_requests))
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
        .layer(TraceLayer::new_for_http())
        // Response compression: gzip + brotli
        .layer(CompressionLayer::new().gzip(true).br(true))
        // Global per-IP rate limiting
        .layer(GovernorLayer::new(global_rate_conf));

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
