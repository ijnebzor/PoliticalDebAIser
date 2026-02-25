use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::routing;
use http_body_util::BodyExt;
use tower::ServiceExt;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;

use political_debaiser::models::{AnalysisResult, DebiasedSummary, SourceMeta, ToneAnalysis};

/// Build the app router with stubs for LLM-dependent endpoints.
fn app() -> Router {
    Router::new()
        .route(
            "/",
            routing::get(|| async { axum::response::Html(include_str!("../static/index.html")) }),
        )
        .route(
            "/health",
            routing::get(|| async { axum::Json(serde_json::json!({"status": "ok"})) }),
        )
        .route(
            "/analyze",
            routing::post(
                |axum::Json(payload): axum::Json<serde_json::Value>| async move {
                    if payload.get("url").and_then(|v| v.as_str()).is_some() {
                        (
                            StatusCode::OK,
                            axum::Json(serde_json::json!({
                                "title": "Test Article",
                                "source_url": "https://example.com/article",
                                "personas": [],
                                "debiaser": {
                                    "consensus_points": [],
                                    "disagreements": [],
                                    "likely_bias_drivers": [],
                                    "truth_seeking_summary": "Stub summary.",
                                    "spectrum_score": 0.0,
                                    "spectrum_explain": "Stub."
                                }
                            })),
                        )
                    } else {
                        (
                            StatusCode::BAD_REQUEST,
                            axum::Json(serde_json::json!({
                                "error": "Invalid URL",
                                "details": "URL must start with http:// or https://"
                            })),
                        )
                    }
                },
            ),
        )
        .route(
            "/analyze-text",
            routing::post(
                |axum::Json(payload): axum::Json<serde_json::Value>| async move {
                    let text = payload.get("text").and_then(|v| v.as_str()).unwrap_or("");
                    if text.trim().is_empty() {
                        (
                            StatusCode::BAD_REQUEST,
                            axum::Json(serde_json::json!({
                                "error": "Empty text",
                                "details": "The text field must not be empty"
                            })),
                        )
                    } else {
                        let title = payload
                            .get("title")
                            .and_then(|v| v.as_str())
                            .unwrap_or("Untitled");
                        (
                            StatusCode::OK,
                            axum::Json(serde_json::json!({
                                "title": title,
                                "source_url": null,
                                "personas": [],
                                "debiaser": {
                                    "consensus_points": [],
                                    "disagreements": [],
                                    "likely_bias_drivers": [],
                                    "truth_seeking_summary": "Stub.",
                                    "spectrum_score": 0.0,
                                    "spectrum_explain": "Stub."
                                }
                            })),
                        )
                    }
                },
            ),
        )
        .nest_service("/static", ServeDir::new("static"))
        .layer(CorsLayer::permissive())
}

/// Build an app using the real history handlers with shared state.
fn app_with_state() -> Router {
    use political_debaiser::models::AppState;
    use political_debaiser::routes;

    let state = AppState::new(
        political_debaiser::models::DEFAULT_CACHE_SIZE,
        political_debaiser::models::DEFAULT_STORE_SIZE,
    );

    Router::new()
        .route("/health", routing::get(routes::health))
        .route(
            "/history",
            routing::get(routes::list_history).post(routes::store_analysis),
        )
        .route(
            "/history/{id}",
            routing::get(routes::get_analysis).delete(routes::delete_history),
        )
        .with_state(state)
        .layer(CorsLayer::permissive())
}

/// Helper: build a valid v3 StoreHistoryRequest JSON body.
fn store_history_body(source_url: &str, title: &str) -> String {
    serde_json::json!({
        "source_url": source_url,
        "result": {
            "title": title,
            "source_url": source_url,
            "personas": [],
            "debiaser": {
                "consensus_points": ["Agreement A"],
                "disagreements": ["Disagreement B"],
                "likely_bias_drivers": ["Bias C"],
                "truth_seeking_summary": "A balanced summary.",
                "spectrum_score": -0.42,
                "spectrum_explain": "Leans liberty."
            }
        }
    })
    .to_string()
}

// =============================================================================
// Basic routing tests
// =============================================================================

#[tokio::test]
async fn get_index_returns_html() {
    let response = app()
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let text = String::from_utf8(body.to_vec()).unwrap();
    assert!(text.contains("Debiaser"));
    assert!(text.contains("<!DOCTYPE html>"));
}

#[tokio::test]
async fn get_static_css() {
    let response = app()
        .oneshot(
            Request::builder()
                .uri("/static/styles.css")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let text = String::from_utf8(body.to_vec()).unwrap();
    assert!(text.contains(":root"));
}

#[tokio::test]
async fn get_static_js() {
    let response = app()
        .oneshot(
            Request::builder()
                .uri("/static/app.js")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn nonexistent_route_returns_404() {
    let response = app()
        .oneshot(
            Request::builder()
                .uri("/nonexistent")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn cors_headers_are_present() {
    let response = app()
        .oneshot(
            Request::builder()
                .method("OPTIONS")
                .uri("/analyze")
                .header("origin", "http://localhost:3000")
                .header("access-control-request-method", "POST")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert!(
        response.status() == StatusCode::OK || response.status() == StatusCode::NO_CONTENT,
        "Expected 200 or 204 for CORS preflight, got {}",
        response.status()
    );
}

// =============================================================================
// Health endpoint tests
// =============================================================================

#[tokio::test]
async fn get_health_returns_ok_with_status() {
    let response = app()
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["status"], "ok");
}

#[tokio::test]
async fn get_health_returns_json_content_type() {
    let response = app()
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let content_type = response
        .headers()
        .get("content-type")
        .expect("content-type header should be present")
        .to_str()
        .unwrap();
    assert!(
        content_type.contains("application/json"),
        "Expected JSON content-type, got {content_type}"
    );
}

// =============================================================================
// /analyze endpoint tests (stub)
// =============================================================================

#[tokio::test]
async fn post_analyze_with_url() {
    let response = app()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/analyze")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"url": "https://example.com/article"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(json.get("title").is_some());
    assert!(json.get("personas").is_some());
    assert!(json.get("debiaser").is_some());
}

#[tokio::test]
async fn post_analyze_without_url_returns_error_json() {
    let response = app()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/analyze")
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(
        json.get("error").is_some(),
        "Error response missing 'error' field"
    );
    assert!(
        json.get("details").is_some(),
        "Error response missing 'details' field"
    );
}

#[tokio::test]
async fn post_analyze_with_non_http_url_returns_ok_in_stub() {
    let response = app()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/analyze")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"url": "ftp://example.com/article"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    // Stub treats any string with "url" key as valid
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn post_analyze_with_empty_url_returns_ok_in_stub() {
    let response = app()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/analyze")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"url": ""}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    // Stub: empty string is still a string
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn post_analyze_with_invalid_json_returns_400() {
    let response = app()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/analyze")
                .header("content-type", "application/json")
                .body(Body::from("not json"))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

// =============================================================================
// /analyze-text endpoint tests (stub)
// =============================================================================

#[tokio::test]
async fn post_analyze_text_with_valid_text_returns_ok() {
    let response = app()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/analyze-text")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"text": "This is a political article about recent policy changes."}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(json.get("title").is_some());
    assert!(json.get("personas").is_some());
    assert!(json.get("debiaser").is_some());
}

#[tokio::test]
async fn post_analyze_text_with_empty_text_returns_400() {
    let response = app()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/analyze-text")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"text": ""}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["error"], "Empty text");
}

#[tokio::test]
async fn post_analyze_text_with_whitespace_only_returns_400() {
    let response = app()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/analyze-text")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"text": "   \n  "}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn post_analyze_text_with_custom_title() {
    let response = app()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/analyze-text")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"text": "Some article content.", "title": "Custom Title"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["title"], "Custom Title");
}

#[tokio::test]
async fn post_analyze_text_defaults_to_untitled() {
    let response = app()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/analyze-text")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"text": "Some article content."}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["title"], "Untitled");
}

#[tokio::test]
async fn post_analyze_text_with_invalid_json_returns_400() {
    let response = app()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/analyze-text")
                .header("content-type", "application/json")
                .body(Body::from("not json"))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

// =============================================================================
// History CRUD tests (real handlers with AppState)
// =============================================================================

#[tokio::test]
async fn post_history_stores_analysis_returns_201() {
    let response = app_with_state()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/history")
                .header("content-type", "application/json")
                .body(Body::from(store_history_body(
                    "https://example.com/article",
                    "Test Article",
                )))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CREATED);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(json.get("id").is_some(), "Response must contain 'id'");
    assert!(
        json.get("share_url").is_some(),
        "Response must contain 'share_url'"
    );
    let id = json["id"].as_str().unwrap();
    assert_eq!(id.len(), 8, "Short ID should be 8 characters");
    assert!(json["share_url"].as_str().unwrap().contains(id));
}

#[tokio::test]
async fn get_history_returns_empty_list_initially() {
    let response = app_with_state()
        .oneshot(
            Request::builder()
                .uri("/history")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(json.is_array());
    assert_eq!(json.as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn get_history_by_invalid_id_returns_404() {
    let response = app_with_state()
        .oneshot(
            Request::builder()
                .uri("/history/nonexistent")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["error"], "Analysis not found");
}

#[tokio::test]
async fn delete_history_nonexistent_returns_404() {
    let response = app_with_state()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri("/history/nonexistent")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["error"], "Analysis not found");
}

// =============================================================================
// History roundtrip tests (real handlers, multi-request)
// =============================================================================

#[tokio::test]
async fn history_store_then_retrieve_roundtrip() {
    use tower::Service;

    let mut app = app_with_state();

    // Step 1: Store an analysis
    let store_req = Request::builder()
        .method("POST")
        .uri("/history")
        .header("content-type", "application/json")
        .body(Body::from(store_history_body(
            "https://example.com/roundtrip-test",
            "Roundtrip Test Article",
        )))
        .unwrap();

    let store_resp = app.call(store_req).await.unwrap();
    assert_eq!(store_resp.status(), StatusCode::CREATED);
    let store_body = store_resp.into_body().collect().await.unwrap().to_bytes();
    let store_json: serde_json::Value = serde_json::from_slice(&store_body).unwrap();
    let id = store_json["id"].as_str().unwrap();

    // Step 2: Retrieve it by ID
    let get_req = Request::builder()
        .uri(format!("/history/{id}"))
        .body(Body::empty())
        .unwrap();

    let get_resp = app.call(get_req).await.unwrap();
    assert_eq!(get_resp.status(), StatusCode::OK);
    let get_body = get_resp.into_body().collect().await.unwrap().to_bytes();
    let get_json: serde_json::Value = serde_json::from_slice(&get_body).unwrap();
    assert_eq!(get_json["id"], id);
    assert_eq!(get_json["source_url"], "https://example.com/roundtrip-test");
    assert_eq!(get_json["response"]["title"], "Roundtrip Test Article");
    assert_eq!(get_json["response"]["debiaser"]["spectrum_score"], -0.42);

    // Step 3: Verify it appears in the list
    let list_req = Request::builder()
        .uri("/history")
        .body(Body::empty())
        .unwrap();

    let list_resp = app.call(list_req).await.unwrap();
    assert_eq!(list_resp.status(), StatusCode::OK);
    let list_body = list_resp.into_body().collect().await.unwrap().to_bytes();
    let list_json: serde_json::Value = serde_json::from_slice(&list_body).unwrap();
    let items = list_json.as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["id"], id);
    assert_eq!(items[0]["article_title"], "Roundtrip Test Article");
    assert_eq!(items[0]["source_url"], "https://example.com/roundtrip-test");
}

#[tokio::test]
async fn history_store_then_delete_roundtrip() {
    use tower::Service;

    let mut app = app_with_state();

    // Step 1: Store an analysis
    let store_req = Request::builder()
        .method("POST")
        .uri("/history")
        .header("content-type", "application/json")
        .body(Body::from(store_history_body(
            "https://example.com/delete-test",
            "Delete Me",
        )))
        .unwrap();

    let store_resp = app.call(store_req).await.unwrap();
    assert_eq!(store_resp.status(), StatusCode::CREATED);
    let store_body = store_resp.into_body().collect().await.unwrap().to_bytes();
    let store_json: serde_json::Value = serde_json::from_slice(&store_body).unwrap();
    let id = store_json["id"].as_str().unwrap();

    // Step 2: Delete it
    let delete_req = Request::builder()
        .method("DELETE")
        .uri(format!("/history/{id}"))
        .body(Body::empty())
        .unwrap();

    let delete_resp = app.call(delete_req).await.unwrap();
    assert_eq!(delete_resp.status(), StatusCode::NO_CONTENT);

    // Step 3: Verify it's gone
    let get_req = Request::builder()
        .uri(format!("/history/{id}"))
        .body(Body::empty())
        .unwrap();

    let get_resp = app.call(get_req).await.unwrap();
    assert_eq!(get_resp.status(), StatusCode::NOT_FOUND);

    // Step 4: Verify list is empty
    let list_req = Request::builder()
        .uri("/history")
        .body(Body::empty())
        .unwrap();

    let list_resp = app.call(list_req).await.unwrap();
    assert_eq!(list_resp.status(), StatusCode::OK);
    let list_body = list_resp.into_body().collect().await.unwrap().to_bytes();
    let list_json: serde_json::Value = serde_json::from_slice(&list_body).unwrap();
    assert_eq!(list_json.as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn post_history_with_invalid_json_returns_error() {
    let response = app_with_state()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/history")
                .header("content-type", "application/json")
                .body(Body::from("not json"))
                .unwrap(),
        )
        .await
        .unwrap();

    // Axum rejects invalid JSON — could be 400 or 422 depending on extractor
    assert!(
        response.status() == StatusCode::BAD_REQUEST
            || response.status() == StatusCode::UNPROCESSABLE_ENTITY,
        "Expected 400 or 422 for invalid JSON, got {}",
        response.status()
    );
}

// =============================================================================
// Stage 3 — ToneAnalysis Serialization Tests
// Stubs: #[ignore] until Sir Reginald adds ToneAnalysis to models.rs
// =============================================================================

#[test]
fn tone_analysis_serialization_roundtrip() {
    let tone = ToneAnalysis {
        rhetorical_devices: vec![
            "appeal to fear".to_string(),
            "loaded language".to_string(),
            "false equivalence".to_string(),
        ],
        emotional_tone: "alarmist".to_string(),
        framing_strategy: "conflict frame".to_string(),
        objectivity_score: 0.35,
    };
    let json = serde_json::to_string(&tone).unwrap();
    let roundtripped: ToneAnalysis = serde_json::from_str(&json).unwrap();
    assert_eq!(roundtripped.rhetorical_devices.len(), 3);
    assert_eq!(roundtripped.rhetorical_devices[0], "appeal to fear");
    assert_eq!(roundtripped.emotional_tone, "alarmist");
    assert_eq!(roundtripped.framing_strategy, "conflict frame");
    assert!((roundtripped.objectivity_score - 0.35).abs() < f64::EPSILON);
}

#[test]
fn tone_analysis_all_fields_populated() {
    let tone = ToneAnalysis {
        rhetorical_devices: vec!["straw man".to_string(), "appeal to authority".to_string()],
        emotional_tone: "measured".to_string(),
        framing_strategy: "human interest".to_string(),
        objectivity_score: 0.78,
    };
    let json = serde_json::to_string(&tone).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    // All fields present in serialized JSON
    assert!(parsed.get("rhetorical_devices").is_some());
    assert!(parsed.get("emotional_tone").is_some());
    assert!(parsed.get("framing_strategy").is_some());
    assert!(parsed.get("objectivity_score").is_some());
    assert_eq!(parsed["rhetorical_devices"].as_array().unwrap().len(), 2);
    assert_eq!(parsed["emotional_tone"], "measured");
    assert_eq!(parsed["framing_strategy"], "human interest");
    assert!((parsed["objectivity_score"].as_f64().unwrap() - 0.78).abs() < f64::EPSILON);
}

#[test]
fn analysis_result_with_tone_analysis_backward_compat() {
    // Stage 2 JSON — no tone_analysis or source_meta fields
    let stage2_json = r#"{
        "title": "Stage 2 Article",
        "source_url": "https://example.com/old",
        "personas": [],
        "debiaser": {
            "consensus_points": ["All agree"],
            "disagreements": [],
            "likely_bias_drivers": [],
            "truth_seeking_summary": "Old format summary.",
            "spectrum_score": -0.5,
            "spectrum_explain": "Slightly left."
        }
    }"#;
    let result: AnalysisResult = serde_json::from_str(stage2_json).unwrap();
    // Stage 3 optional fields default to None
    assert!(result.tone_analysis.is_none());
    assert!(result.source_meta.is_none());
    assert!(result.warnings.is_empty());
    // Core fields intact
    assert_eq!(result.title, "Stage 2 Article");
    assert_eq!(
        result.source_url,
        Some("https://example.com/old".to_string())
    );
    assert!((result.debiaser.spectrum_score - (-0.5)).abs() < f64::EPSILON);
}

// =============================================================================
// Stage 3 — SourceMeta Serialization Tests
// Stubs: #[ignore] until Sir Reginald adds SourceMeta to models.rs
// =============================================================================

#[test]
fn source_meta_serialization_roundtrip() {
    let meta = SourceMeta {
        publication: "Reuters".to_string(),
        known_bias: Some("center".to_string()),
        ownership_type: Some("wire_service".to_string()),
    };
    let json = serde_json::to_string(&meta).unwrap();
    let roundtripped: SourceMeta = serde_json::from_str(&json).unwrap();
    assert_eq!(roundtripped.publication, "Reuters");
    assert_eq!(roundtripped.known_bias, Some("center".to_string()));
    assert_eq!(
        roundtripped.ownership_type,
        Some("wire_service".to_string())
    );
}

#[test]
fn source_meta_with_known_publication() {
    let meta = SourceMeta {
        publication: "Fox News".to_string(),
        known_bias: Some("right-leaning".to_string()),
        ownership_type: Some("corporate".to_string()),
    };
    let json = serde_json::to_string(&meta).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["publication"], "Fox News");
    assert_eq!(parsed["known_bias"], "right-leaning");
    assert_eq!(parsed["ownership_type"], "corporate");
}

#[test]
fn source_meta_with_unknown_publication() {
    let meta = SourceMeta {
        publication: "Random Blog".to_string(),
        known_bias: None,
        ownership_type: None,
    };
    let json = serde_json::to_string(&meta).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["publication"], "Random Blog");
    assert!(parsed["known_bias"].is_null());
    assert!(parsed["ownership_type"].is_null());
    // Verify roundtrip preserves None fields
    let roundtripped: SourceMeta = serde_json::from_str(&json).unwrap();
    assert!(roundtripped.known_bias.is_none());
    assert!(roundtripped.ownership_type.is_none());
}

#[test]
fn analysis_result_with_source_meta_backward_compat() {
    // Stage 2 JSON with warnings field but no source_meta or tone_analysis
    let stage2_json = r#"{
        "title": "Backward Compat Test",
        "source_url": null,
        "personas": [],
        "debiaser": {
            "consensus_points": [],
            "disagreements": [],
            "likely_bias_drivers": [],
            "truth_seeking_summary": "Test.",
            "spectrum_score": 0.0,
            "spectrum_explain": "Test."
        },
        "warnings": ["2/8 personas failed"]
    }"#;
    let result: AnalysisResult = serde_json::from_str(stage2_json).unwrap();
    assert!(result.source_meta.is_none());
    assert!(result.tone_analysis.is_none());
    assert_eq!(result.warnings.len(), 1);
    assert_eq!(result.title, "Backward Compat Test");
}

// =============================================================================
// Stage 3 — Integration: Stub /analyze and /analyze-text with new fields
// Stubs: #[ignore] until routes deliver new API shape
// =============================================================================

#[tokio::test]
async fn analyze_response_includes_tone_and_source_meta() {
    // Verify the full Stage 3 AnalysisResult shape serializes correctly,
    // simulating what /analyze-text would return with Stage 3 features.
    let full_result = AnalysisResult {
        title: "Stage 3 Response Shape".to_string(),
        source_url: Some("https://example.com".to_string()),
        personas: vec![],
        debiaser: DebiasedSummary {
            consensus_points: vec!["Agreement".to_string()],
            disagreements: vec![],
            likely_bias_drivers: vec![],
            truth_seeking_summary: "Comprehensive analysis.".to_string(),
            spectrum_score: 0.1,
            spectrum_explain: "Slightly right.".to_string(),
        },
        tone_analysis: Some(ToneAnalysis {
            rhetorical_devices: vec!["appeal to fear".to_string()],
            emotional_tone: "urgent".to_string(),
            framing_strategy: "conflict frame".to_string(),
            objectivity_score: 0.45,
        }),
        source_meta: Some(SourceMeta {
            publication: "The Guardian".to_string(),
            known_bias: Some("left-leaning".to_string()),
            ownership_type: Some("corporate".to_string()),
        }),
        warnings: vec![],
    };

    // Serialize as the API would
    let json = serde_json::to_string(&full_result).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

    // Verify Stage 3 fields present in API response shape
    assert!(
        parsed.get("tone_analysis").is_some(),
        "Response must include tone_analysis"
    );
    assert!(
        parsed.get("source_meta").is_some(),
        "Response must include source_meta"
    );
    let tone = &parsed["tone_analysis"];
    assert!(tone.get("rhetorical_devices").is_some());
    assert!(tone.get("emotional_tone").is_some());
    assert!(tone.get("framing_strategy").is_some());
    assert!(tone.get("objectivity_score").is_some());
    let source = &parsed["source_meta"];
    assert!(source.get("publication").is_some());
    assert!(source.get("known_bias").is_some());
    assert!(source.get("ownership_type").is_some());

    // Verify round-trip through JSON layer
    let roundtripped: AnalysisResult = serde_json::from_str(&json).unwrap();
    assert!(roundtripped.tone_analysis.is_some());
    assert!(roundtripped.source_meta.is_some());
    assert_eq!(roundtripped.tone_analysis.unwrap().emotional_tone, "urgent");
    assert_eq!(
        roundtripped.source_meta.unwrap().publication,
        "The Guardian"
    );
}

#[tokio::test]
async fn store_history_body_includes_stage3_fields() {
    use tower::Service;

    let mut app = app_with_state();

    // Create a StoreHistoryRequest with Stage 3 fields (tone_analysis + source_meta)
    let body = serde_json::json!({
        "source_url": "https://example.com/stage3",
        "result": {
            "title": "Stage 3 Article",
            "source_url": "https://example.com/stage3",
            "personas": [],
            "debiaser": {
                "consensus_points": [],
                "disagreements": [],
                "likely_bias_drivers": [],
                "truth_seeking_summary": "Test.",
                "spectrum_score": 0.0,
                "spectrum_explain": "Test."
            },
            "tone_analysis": {
                "rhetorical_devices": ["loaded language", "appeal to emotion"],
                "emotional_tone": "inflammatory",
                "framing_strategy": "morality frame",
                "objectivity_score": 0.3
            },
            "source_meta": {
                "publication": "The New York Times",
                "known_bias": "center-left",
                "ownership_type": "publicly-traded"
            }
        }
    })
    .to_string();

    // Store
    let store_req = Request::builder()
        .method("POST")
        .uri("/history")
        .header("content-type", "application/json")
        .body(Body::from(body))
        .unwrap();
    let store_resp = app.call(store_req).await.unwrap();
    assert_eq!(store_resp.status(), StatusCode::CREATED);
    let store_body = store_resp.into_body().collect().await.unwrap().to_bytes();
    let store_json: serde_json::Value = serde_json::from_slice(&store_body).unwrap();
    let id = store_json["id"].as_str().unwrap();

    // Retrieve
    let get_req = Request::builder()
        .uri(format!("/history/{id}"))
        .body(Body::empty())
        .unwrap();
    let get_resp = app.call(get_req).await.unwrap();
    assert_eq!(get_resp.status(), StatusCode::OK);
    let get_body = get_resp.into_body().collect().await.unwrap().to_bytes();
    let stored: serde_json::Value = serde_json::from_slice(&get_body).unwrap();

    // Verify Stage 3 fields preserved in stored result
    let response = &stored["response"];
    assert!(
        response.get("tone_analysis").is_some(),
        "tone_analysis must be preserved"
    );
    assert!(
        response.get("source_meta").is_some(),
        "source_meta must be preserved"
    );
    assert_eq!(response["tone_analysis"]["emotional_tone"], "inflammatory");
    assert_eq!(response["tone_analysis"]["objectivity_score"], 0.3);
    assert_eq!(
        response["tone_analysis"]["rhetorical_devices"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(response["source_meta"]["publication"], "The New York Times");
    assert_eq!(response["source_meta"]["known_bias"], "center-left");
    assert_eq!(response["source_meta"]["ownership_type"], "publicly-traded");
}
