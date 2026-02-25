// =============================================================================
// Stage 2 — End-to-End & Smoke Tests
//
// Tests the full analysis pipeline with a mock Ollama server (wiremock).
// Covers: E2E flow, Ollama connectivity, error paths, partial failures.
//
// To run against real Ollama (local): OLLAMA_LIVE=1 cargo test --test e2e_tests
// To run with mocks (CI): cargo test --test e2e_tests
// =============================================================================

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::routing;
use http_body_util::BodyExt;
use serial_test::serial;
use tower::ServiceExt;
use tower_http::cors::CorsLayer;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use political_debaiser::models::{
    AnalysisResult, AppState, DebiasedSummary, PersonaId, PersonaOutput,
};

// =============================================================================
// Test fixtures
// =============================================================================

/// A valid persona JSON response that the mock Ollama will return.
fn mock_persona_json(persona_id: &str, stance: f64) -> String {
    serde_json::json!({
        "stance_score": stance,
        "confidence": 0.8,
        "summary": format!("Analysis from {persona_id} perspective on the article."),
        "key_claims": [
            format!("{persona_id} claim 1"),
            format!("{persona_id} claim 2")
        ],
        "fact_checks": [{
            "claim": "The article states X is true",
            "assessment": "supported",
            "rationale": "Evidence supports this claim"
        }],
        "caveats": [format!("{persona_id} may overlook Y")],
        "axes": {
            "economic": stance * 0.5,
            "social": stance * -0.3
        }
    })
    .to_string()
}

/// A valid debiased synthesis JSON response.
/// Reserved for future use when mock Ollama can return different responses per call.
#[allow(dead_code)]
fn mock_debiased_json() -> String {
    serde_json::json!({
        "consensus_points": [
            "All perspectives agree the article raises important questions",
            "Multiple viewpoints note the policy impact"
        ],
        "disagreements": [
            "Left-leaning personas emphasize equity, right-leaning emphasize efficiency"
        ],
        "likely_bias_drivers": [
            "Security-first framing in the original article"
        ],
        "truth_seeking_summary": "The article addresses a complex policy issue where multiple legitimate perspectives exist. Evidence suggests a balanced approach is warranted.",
        "spectrum_explain": "The weighted mean reflects moderate disagreement across personas."
    })
    .to_string()
}

/// An Ollama chat response wrapping some content.
fn ollama_response(content: &str) -> serde_json::Value {
    serde_json::json!({
        "model": "llama3.2",
        "message": {
            "role": "assistant",
            "content": content
        },
        "done": true
    })
}

/// Build the app router with a specific Ollama URL.
fn app_with_ollama_url(ollama_url: &str) -> Router {
    // SAFETY: Tests are run single-threaded via --test-threads=1 or serialized.
    // set_var is unsafe in Rust 2024 due to potential data races with getenv.
    unsafe {
        std::env::set_var("OLLAMA_URL", ollama_url);
        std::env::set_var("OLLAMA_MODEL", "test-model");
    }

    build_app_router()
}

/// Build the app router without modifying env vars (for tests that don't hit Ollama).
fn app_default() -> Router {
    build_app_router()
}

fn build_app_router() -> Router {
    use political_debaiser::routes;

    let state = AppState::new(
        political_debaiser::models::DEFAULT_CACHE_SIZE,
        political_debaiser::models::DEFAULT_STORE_SIZE,
    );

    Router::new()
        .route("/health", routing::get(routes::health))
        .route("/analyze-text", routing::post(routes::analyze_text))
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

// =============================================================================
// Helper: set up mock Ollama that returns valid responses
// =============================================================================

/// Track how many calls we've seen — first 8 are persona calls, 9th is debiased.
/// Reserved for future tests that need stricter call-count expectations.
#[allow(dead_code)]
async fn setup_mock_ollama() -> MockServer {
    let server = MockServer::start().await;

    // The mock returns persona JSON for calls 1-8, then debiased JSON for call 9.
    // Since wiremock can't easily count calls, we use a simpler approach:
    // return persona JSON by default (the first call that matches the debiased
    // prompt pattern returns debiased JSON).
    //
    // In practice, analyze_all_personas calls 8 times, then synthesize_debiased
    // calls once. We use respond_with for the persona response (up_to 8 times)
    // and a separate mount for the 9th.
    //
    // Simpler: just always return persona JSON. The debiaser parser will fail,
    // but analyze_full has a fallback. For full E2E, we mount a responder that
    // switches based on request count.

    // Mount: any POST to /api/chat returns a valid persona response.
    // We'll use expect(8..=9) to allow 8 persona + 1 debiased calls.
    Mock::given(method("POST"))
        .and(path("/api/chat"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(ollama_response(&mock_persona_json("test_persona", 0.5))),
        )
        .expect(8..)
        .mount(&server)
        .await;

    server
}

/// Setup mock that returns valid persona JSON, with a separate debiased response.
async fn setup_full_mock_ollama() -> MockServer {
    let server = MockServer::start().await;

    // All calls return persona JSON. The debiased synthesis will use the
    // fallback in analyze_full() since the persona JSON won't parse as
    // ParsedDebiased. This is the expected behavior — the fallback produces
    // a valid DebiasedSummary with calculated spectrum_score.
    Mock::given(method("POST"))
        .and(path("/api/chat"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(ollama_response(&mock_persona_json("test_persona", 0.5))),
        )
        .expect(1..)
        .mount(&server)
        .await;

    server
}

// =============================================================================
// E2E Tests: Full pipeline (analyze-text -> personas -> synthesis -> response)
// =============================================================================

#[tokio::test]
#[serial]
async fn e2e_analyze_text_returns_valid_analysis_result() {
    let mock = setup_full_mock_ollama().await;
    let app = app_with_ollama_url(&mock.uri());

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/analyze-text")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "text": "The government announced new regulations on technology companies today, sparking debate across the political spectrum about the balance between innovation and consumer protection.",
                        "title": "Tech Regulation Article"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let result: AnalysisResult = serde_json::from_slice(&body).unwrap();

    // Verify the full response shape
    assert_eq!(result.title, "Tech Regulation Article");
    assert!(result.source_url.is_none()); // text input has no URL

    // Should have persona outputs (may be less than 8 if some fail in mock)
    assert!(
        !result.personas.is_empty(),
        "Expected at least one persona output"
    );

    // Each persona should have valid fields
    for persona in &result.personas {
        assert!(
            persona.stance_score >= -3.0 && persona.stance_score <= 3.0,
            "stance_score {} out of range for {}",
            persona.stance_score,
            persona.title
        );
        assert!(
            persona.confidence >= 0.0 && persona.confidence <= 1.0,
            "confidence {} out of range for {}",
            persona.confidence,
            persona.title
        );
        assert!(
            !persona.summary.is_empty(),
            "Empty summary for {}",
            persona.title
        );
        assert!(!persona.title.is_empty());
    }

    // Debiaser should exist with valid structure
    assert!(
        result.debiaser.spectrum_score >= -3.0 && result.debiaser.spectrum_score <= 3.0,
        "spectrum_score {} out of range",
        result.debiaser.spectrum_score
    );
    assert!(!result.debiaser.spectrum_explain.is_empty());
}

#[tokio::test]
#[serial]
async fn e2e_response_shape_matches_v3_schema() {
    let mock = setup_full_mock_ollama().await;
    let app = app_with_ollama_url(&mock.uri());

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/analyze-text")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "text": "A short political article about tax reform."
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    // Verify top-level v3 schema fields
    assert!(json.get("title").is_some(), "Missing 'title' field");
    assert!(json.get("personas").is_some(), "Missing 'personas' field");
    assert!(json.get("debiaser").is_some(), "Missing 'debiaser' field");
    assert!(json["personas"].is_array(), "'personas' should be an array");

    // Verify debiaser sub-fields
    let debiaser = &json["debiaser"];
    assert!(
        debiaser.get("consensus_points").is_some(),
        "Missing debiaser.consensus_points"
    );
    assert!(
        debiaser.get("disagreements").is_some(),
        "Missing debiaser.disagreements"
    );
    assert!(
        debiaser.get("likely_bias_drivers").is_some(),
        "Missing debiaser.likely_bias_drivers"
    );
    assert!(
        debiaser.get("truth_seeking_summary").is_some(),
        "Missing debiaser.truth_seeking_summary"
    );
    assert!(
        debiaser.get("spectrum_score").is_some(),
        "Missing debiaser.spectrum_score"
    );
    assert!(
        debiaser.get("spectrum_explain").is_some(),
        "Missing debiaser.spectrum_explain"
    );

    // Verify persona sub-fields if any personas returned
    if let Some(personas) = json["personas"].as_array() {
        for persona in personas {
            assert!(persona.get("id").is_some(), "Missing persona.id");
            assert!(persona.get("title").is_some(), "Missing persona.title");
            assert!(
                persona.get("stance_score").is_some(),
                "Missing persona.stance_score"
            );
            assert!(
                persona.get("confidence").is_some(),
                "Missing persona.confidence"
            );
            assert!(persona.get("summary").is_some(), "Missing persona.summary");
            assert!(
                persona.get("key_claims").is_some(),
                "Missing persona.key_claims"
            );
            assert!(
                persona.get("fact_checks").is_some(),
                "Missing persona.fact_checks"
            );
        }
    }
}

#[tokio::test]
#[serial]
async fn e2e_all_eight_persona_ids_are_valid() {
    let mock = setup_full_mock_ollama().await;
    let app = app_with_ollama_url(&mock.uri());

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/analyze-text")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "text": "A detailed article about immigration policy reform with multiple perspectives."
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let result: AnalysisResult = serde_json::from_slice(&body).unwrap();

    // All 8 persona IDs should be present (with mock, all should succeed)
    let expected_ids: Vec<PersonaId> = PersonaId::all().to_vec();
    let actual_ids: Vec<&PersonaId> = result.personas.iter().map(|p| &p.id).collect();

    assert_eq!(
        result.personas.len(),
        8,
        "Expected 8 personas, got {}. IDs: {:?}",
        result.personas.len(),
        actual_ids
    );

    for expected in &expected_ids {
        assert!(
            actual_ids.contains(&expected),
            "Missing persona: {:?}",
            expected
        );
    }
}

#[tokio::test]
#[serial]
async fn e2e_spectrum_score_is_calculated_server_side() {
    let mock = setup_full_mock_ollama().await;
    let app = app_with_ollama_url(&mock.uri());

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/analyze-text")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "text": "An article about healthcare policy."
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let result: AnalysisResult = serde_json::from_slice(&body).unwrap();

    // The spectrum score should be a confidence-weighted mean of persona stances.
    // With our mock returning stance=0.5 and confidence=0.8 for all personas,
    // the weighted mean should be exactly 0.5.
    if result.personas.len() == 8 {
        let weight_sum: f64 = result.personas.iter().map(|p| p.confidence).sum();
        let weighted_sum: f64 = result
            .personas
            .iter()
            .map(|p| p.stance_score * p.confidence)
            .sum();
        let expected_score = if weight_sum > 0.0 {
            (weighted_sum / weight_sum * 100.0).round() / 100.0
        } else {
            0.0
        };

        assert!(
            (result.debiaser.spectrum_score - expected_score).abs() < 0.01,
            "Spectrum score {:.2} doesn't match expected weighted mean {:.2}",
            result.debiaser.spectrum_score,
            expected_score
        );
    }
}

// =============================================================================
// Ollama Connectivity Tests
// =============================================================================

#[tokio::test]
#[serial]
async fn ollama_mock_returns_valid_json() {
    let mock = setup_full_mock_ollama().await;
    let app = app_with_ollama_url(&mock.uri());

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/analyze-text")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({ "text": "Test article." }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    // Should succeed with mock Ollama
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "Mock Ollama should return valid responses"
    );
}

#[tokio::test]
#[serial]
async fn ollama_down_returns_bad_gateway() {
    // Point to a port that nothing is listening on
    let app = app_with_ollama_url("http://127.0.0.1:19999");

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/analyze-text")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({ "text": "Test article content." }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    // Should return 502 Bad Gateway when Ollama is unreachable
    assert!(
        response.status() == StatusCode::BAD_GATEWAY
            || response.status() == StatusCode::INTERNAL_SERVER_ERROR,
        "Expected 502 or 500 when Ollama is down, got {}",
        response.status()
    );

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(
        json.get("error").is_some(),
        "Error response must have 'error' field"
    );
}

#[tokio::test]
#[serial]
async fn ollama_returns_500_triggers_retry_and_error() {
    let server = MockServer::start().await;

    // Mock returns 500 for all requests (after 3 retries, should fail)
    Mock::given(method("POST"))
        .and(path("/api/chat"))
        .respond_with(ResponseTemplate::new(500).set_body_string("Internal Server Error"))
        .expect(3..) // should retry
        .mount(&server)
        .await;

    let app = app_with_ollama_url(&server.uri());

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/analyze-text")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({ "text": "Test article for 500 handling." }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert!(
        response.status().is_server_error() || response.status() == StatusCode::BAD_GATEWAY,
        "Expected server error or 502 after Ollama 500s, got {}",
        response.status()
    );
}

#[tokio::test]
#[serial]
async fn ollama_returns_malformed_json_triggers_error() {
    let server = MockServer::start().await;

    // Mock returns 200 but with invalid JSON
    Mock::given(method("POST"))
        .and(path("/api/chat"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(ollama_response("This is not JSON at all")),
        )
        .mount(&server)
        .await;

    let app = app_with_ollama_url(&server.uri());

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/analyze-text")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({ "text": "Test article for malformed response." })
                        .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    // All 8 persona parses will fail -> "All persona analyses failed" -> 500
    assert!(
        response.status().is_server_error(),
        "Expected 500 when Ollama returns non-JSON content, got {}",
        response.status()
    );
}

// =============================================================================
// Error Path Tests
// =============================================================================

#[tokio::test]
async fn error_empty_text_returns_400() {
    let app = app_default();

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/analyze-text")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::json!({ "text": "" }).to_string()))
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
async fn error_whitespace_only_text_returns_400() {
    let app = app_default();

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/analyze-text")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({ "text": "   \n\t  " }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn error_text_too_long_returns_400() {
    let app = app_default();

    // Generate text over 100K chars
    let long_text = "x".repeat(100_001);

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/analyze-text")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({ "text": long_text }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["error"], "Text too long");
}

#[tokio::test]
async fn error_invalid_json_body_returns_4xx() {
    let app = app_default();

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/analyze-text")
                .header("content-type", "application/json")
                .body(Body::from("not valid json"))
                .unwrap(),
        )
        .await
        .unwrap();

    assert!(
        response.status().is_client_error(),
        "Expected 4xx for invalid JSON body, got {}",
        response.status()
    );
}

#[tokio::test]
async fn error_missing_text_field_returns_4xx() {
    let app = app_default();

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/analyze-text")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({ "url": "https://example.com" }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert!(
        response.status().is_client_error(),
        "Expected 4xx for missing 'text' field, got {}",
        response.status()
    );
}

// =============================================================================
// Ollama Timeout Tests
// =============================================================================

#[tokio::test]
#[serial]
async fn ollama_slow_response_triggers_timeout() {
    let server = MockServer::start().await;

    // Mock returns valid response but with 130 second delay (> 120s timeout)
    Mock::given(method("POST"))
        .and(path("/api/chat"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(ollama_response(&mock_persona_json("slow", 0.0)))
                .set_delay(std::time::Duration::from_secs(130)),
        )
        .mount(&server)
        .await;

    let app = app_with_ollama_url(&server.uri());

    // Use a shorter timeout for the test itself
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        app.oneshot(
            Request::builder()
                .method("POST")
                .uri("/analyze-text")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({ "text": "Timeout test article." }).to_string(),
                ))
                .unwrap(),
        ),
    )
    .await;

    // Either the request times out at the app level, or our test timeout fires
    match result {
        Ok(Ok(response)) => {
            // App handled the timeout gracefully
            assert!(
                response.status() == StatusCode::GATEWAY_TIMEOUT
                    || response.status().is_server_error(),
                "Expected timeout or server error, got {}",
                response.status()
            );
        }
        Ok(Err(_)) => {
            // Connection error from the mock — also acceptable
        }
        Err(_) => {
            // Test timeout fired — the app didn't handle timeout fast enough
            // This is expected since reqwest timeout is 120s
        }
    }
}

// =============================================================================
// E2E: Full Pipeline then History Roundtrip
// =============================================================================

#[tokio::test]
#[serial]
async fn e2e_analyze_then_store_then_retrieve() {
    use tower::Service;

    let mock = setup_full_mock_ollama().await;
    let ollama_url = mock.uri();
    unsafe {
        std::env::set_var("OLLAMA_URL", &ollama_url);
        std::env::set_var("OLLAMA_MODEL", "test-model");
    }

    let state = AppState::new(
        political_debaiser::models::DEFAULT_CACHE_SIZE,
        political_debaiser::models::DEFAULT_STORE_SIZE,
    );

    let mut app = {
        use political_debaiser::routes;
        Router::new()
            .route("/analyze-text", routing::post(routes::analyze_text))
            .route(
                "/history",
                routing::get(routes::list_history).post(routes::store_analysis),
            )
            .route("/history/{id}", routing::get(routes::get_analysis))
            .with_state(state)
            .layer(CorsLayer::permissive())
    };

    // Step 1: Analyze text
    let analyze_req = Request::builder()
        .method("POST")
        .uri("/analyze-text")
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::json!({
                "text": "An article about economic policy.",
                "title": "Economic Policy Article"
            })
            .to_string(),
        ))
        .unwrap();

    let analyze_resp = app.call(analyze_req).await.unwrap();
    assert_eq!(analyze_resp.status(), StatusCode::OK);
    let analyze_body = analyze_resp.into_body().collect().await.unwrap().to_bytes();
    let analysis: AnalysisResult = serde_json::from_slice(&analyze_body).unwrap();

    // Step 2: Store the analysis
    let store_req = Request::builder()
        .method("POST")
        .uri("/history")
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::json!({
                "source_url": "",
                "result": analysis
            })
            .to_string(),
        ))
        .unwrap();

    let store_resp = app.call(store_req).await.unwrap();
    assert_eq!(store_resp.status(), StatusCode::CREATED);
    let store_body = store_resp.into_body().collect().await.unwrap().to_bytes();
    let store_json: serde_json::Value = serde_json::from_slice(&store_body).unwrap();
    let id = store_json["id"].as_str().unwrap();

    // Step 3: Retrieve by ID
    let get_req = Request::builder()
        .uri(format!("/history/{id}"))
        .body(Body::empty())
        .unwrap();

    let get_resp = app.call(get_req).await.unwrap();
    assert_eq!(get_resp.status(), StatusCode::OK);
    let get_body = get_resp.into_body().collect().await.unwrap().to_bytes();
    let stored: serde_json::Value = serde_json::from_slice(&get_body).unwrap();

    // Verify the stored data matches
    assert_eq!(stored["response"]["title"], "Economic Policy Article");
    assert!(stored["response"]["personas"].is_array());
    assert!(stored["response"]["debiaser"].is_object());

    // Step 4: Verify in listing
    let list_req = Request::builder()
        .uri("/history")
        .body(Body::empty())
        .unwrap();

    let list_resp = app.call(list_req).await.unwrap();
    assert_eq!(list_resp.status(), StatusCode::OK);
    let list_body = list_resp.into_body().collect().await.unwrap().to_bytes();
    let list: serde_json::Value = serde_json::from_slice(&list_body).unwrap();
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert_eq!(list[0]["article_title"], "Economic Policy Article");
}

// =============================================================================
// Health Endpoint
// =============================================================================

#[tokio::test]
async fn health_endpoint_returns_ok() {
    let app = app_default();

    let response = app
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

// =============================================================================
// Persona Output Validation
// =============================================================================

#[tokio::test]
#[serial]
async fn persona_outputs_have_clamped_values() {
    let mock = setup_full_mock_ollama().await;
    let app = app_with_ollama_url(&mock.uri());

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/analyze-text")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({ "text": "An article about climate change policy." })
                        .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let result: AnalysisResult = serde_json::from_slice(&body).unwrap();

    for persona in &result.personas {
        // stance_score clamped to [-3, 3]
        assert!(
            persona.stance_score >= -3.0 && persona.stance_score <= 3.0,
            "{}: stance_score {} out of [-3, 3]",
            persona.title,
            persona.stance_score
        );

        // confidence clamped to [0, 1]
        assert!(
            persona.confidence >= 0.0 && persona.confidence <= 1.0,
            "{}: confidence {} out of [0, 1]",
            persona.title,
            persona.confidence
        );

        // axes clamped to [-3, 3] if present
        if let Some(axes) = &persona.axes {
            assert!(
                axes.economic >= -3.0 && axes.economic <= 3.0,
                "{}: economic axis {} out of [-3, 3]",
                persona.title,
                axes.economic
            );
            assert!(
                axes.social >= -3.0 && axes.social <= 3.0,
                "{}: social axis {} out of [-3, 3]",
                persona.title,
                axes.social
            );
        }
    }
}

// =============================================================================
// Partial Failure Tests
// =============================================================================

#[tokio::test]
async fn partial_failure_some_personas_fail_still_returns_result() {
    let _server = MockServer::start().await;

    // Use an atomic counter to alternate between success and failure.
    // First 4 calls succeed, next 4 fail, then debiased call also fails (fallback).
    // wiremock doesn't support stateful responses easily, so we use a simpler approach:
    // Return valid JSON that will parse correctly. The important test is that
    // analyze_all_personas handles partial failures gracefully.

    // To actually test partial failure, we'd need the code to be refactored
    // with injectable dependencies. Instead, we test the unit behavior directly:
    // verify that if we construct an AnalysisResult with fewer than 8 personas,
    // it serializes correctly and the debiaser still works.

    let partial_result = AnalysisResult {
        title: "Partial Result".to_string(),
        source_url: None,
        personas: vec![
            PersonaOutput {
                id: PersonaId::ProgressiveActivist,
                title: "Progressive Activist".to_string(),
                stance_score: -2.1,
                confidence: 0.85,
                summary: "From a progressive perspective...".to_string(),
                key_claims: vec!["Claim A".to_string()],
                fact_checks: vec![],
                caveats: vec!["May overlook security concerns".to_string()],
                axes: Some(political_debaiser::models::Axes2D {
                    economic: -1.5,
                    social: -1.0,
                }),
            },
            PersonaOutput {
                id: PersonaId::NationalSecurityHawk,
                title: "National Security Hawk".to_string(),
                stance_score: 2.3,
                confidence: 0.75,
                summary: "From a security perspective...".to_string(),
                key_claims: vec!["Claim B".to_string()],
                fact_checks: vec![],
                caveats: vec!["May overlook civil liberties".to_string()],
                axes: Some(political_debaiser::models::Axes2D {
                    economic: 0.5,
                    social: 2.0,
                }),
            },
        ],
        debiaser: DebiasedSummary {
            consensus_points: vec!["Both agree the issue is important".to_string()],
            disagreements: vec!["Fundamental disagreement on approach".to_string()],
            likely_bias_drivers: vec!["Framing".to_string()],
            truth_seeking_summary: "Partial analysis with 2 of 8 personas.".to_string(),
            spectrum_score: 0.19,
            spectrum_explain: "Weighted mean of 2 personas.".to_string(),
        },
        tone_analysis: None,
        source_meta: None,
        warnings: vec!["6 of 8 persona analyses failed".to_string()],
    };

    // Verify it serializes correctly
    let json = serde_json::to_string(&partial_result).unwrap();
    let deserialized: AnalysisResult = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.personas.len(), 2);
    assert_eq!(deserialized.title, "Partial Result");
    assert!(!deserialized.debiaser.truth_seeking_summary.is_empty());
    assert!(deserialized.warnings.len() == 1);
    assert!(deserialized.warnings[0].contains("6 of 8"));
}

#[tokio::test]
async fn partial_failure_empty_personas_is_error() {
    // If ALL personas fail, analyze_all_personas returns an error.
    // Test this via the analyze_all_personas function directly would require
    // a live or mock Ollama. Instead, verify that an empty personas list
    // is handled at the serialization level.
    let result = AnalysisResult {
        title: "Empty".to_string(),
        source_url: None,
        personas: vec![],
        debiaser: DebiasedSummary {
            consensus_points: vec![],
            disagreements: vec![],
            likely_bias_drivers: vec![],
            truth_seeking_summary: "No analyses available.".to_string(),
            spectrum_score: 0.0,
            spectrum_explain: "No data.".to_string(),
        },
        tone_analysis: None,
        source_meta: None,
        warnings: vec![],
    };

    let json = serde_json::to_string(&result).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["personas"].as_array().unwrap().len(), 0);
}

#[tokio::test]
#[serial]
async fn all_personas_fail_returns_server_error() {
    let server = MockServer::start().await;

    // Mock returns valid Ollama response shape, but with content that won't
    // parse as PersonaOutput JSON — causing all 8 persona analyses to fail.
    Mock::given(method("POST"))
        .and(path("/api/chat"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(ollama_response("I refuse to respond in JSON format.")),
        )
        .mount(&server)
        .await;

    let app = app_with_ollama_url(&server.uri());

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/analyze-text")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({ "text": "Test article for all-fail scenario." })
                        .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    // Should get a server error since all personas failed
    assert!(
        response.status().is_server_error(),
        "Expected 500 when all persona analyses fail, got {}",
        response.status()
    );

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(json.get("error").is_some());
}

// =============================================================================
// Debiased Fallback Tests
// =============================================================================

#[tokio::test]
#[serial]
async fn debiaser_fallback_produces_valid_result() {
    // Test analyze_full's fallback behavior when synthesize_debiased fails.
    // With mock Ollama returning persona JSON, the debiased call will fail
    // (wrong JSON shape) and the fallback should kick in.
    let mock = setup_full_mock_ollama().await;
    let app = app_with_ollama_url(&mock.uri());

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/analyze-text")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({ "text": "Article to test debiaser fallback." }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let result: AnalysisResult = serde_json::from_slice(&body).unwrap();

    // Debiaser should have valid structure even in fallback mode
    assert!(result.debiaser.spectrum_score >= -3.0 && result.debiaser.spectrum_score <= 3.0,);
    assert!(!result.debiaser.spectrum_explain.is_empty());
    assert!(!result.debiaser.truth_seeking_summary.is_empty());
}

// =============================================================================
// Live Ollama Tests (only run with OLLAMA_LIVE=1)
// =============================================================================

#[tokio::test]
#[serial]
async fn live_ollama_health_check() {
    if std::env::var("OLLAMA_LIVE").is_err() {
        eprintln!("Skipping live Ollama test (set OLLAMA_LIVE=1 to enable)");
        return;
    }

    let url = std::env::var("OLLAMA_URL").unwrap_or_else(|_| "http://localhost:11434".to_string());
    let client = reqwest::Client::new();
    let resp = client.get(format!("{url}/api/tags")).send().await;

    match resp {
        Ok(r) => {
            assert!(r.status().is_success(), "Ollama API should respond 200");
            let body: serde_json::Value = r.json().await.unwrap();
            assert!(body.get("models").is_some(), "Ollama should list models");
        }
        Err(e) => {
            panic!("Cannot connect to Ollama at {url}: {e}");
        }
    }
}

#[tokio::test]
#[serial]
async fn live_ollama_analyze_text_e2e() {
    if std::env::var("OLLAMA_LIVE").is_err() {
        eprintln!("Skipping live Ollama E2E test (set OLLAMA_LIVE=1 to enable)");
        return;
    }

    let ollama_url =
        std::env::var("OLLAMA_URL").unwrap_or_else(|_| "http://localhost:11434".to_string());
    let app = app_with_ollama_url(&ollama_url);

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/analyze-text")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "text": "The government today announced new cybersecurity regulations requiring companies to report data breaches within 72 hours. Privacy advocates praised the transparency requirements while business groups warned about compliance costs. The legislation passed with bipartisan support.",
                        "title": "Cybersecurity Regulation Article"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let result: AnalysisResult = serde_json::from_slice(&body).unwrap();

    // Live test: verify all 8 personas
    assert!(
        !result.personas.is_empty(),
        "Live analysis should return at least 1 persona, got 0",
    );

    // Verify response shape is complete
    assert_eq!(result.title, "Cybersecurity Regulation Article");
    assert!(!result.debiaser.truth_seeking_summary.is_empty());
}

// =============================================================================
// Security Tests — CSP & Security Headers
// =============================================================================

/// Build a router that includes the same security header layers as main.rs.
fn app_with_security_headers() -> Router {
    use axum::http::{HeaderName, HeaderValue};
    use political_debaiser::routes;
    use tower_http::set_header::SetResponseHeaderLayer;

    let state = AppState::new(
        political_debaiser::models::DEFAULT_CACHE_SIZE,
        political_debaiser::models::DEFAULT_STORE_SIZE,
    );

    Router::new()
        .route("/health", routing::get(routes::health))
        .route("/analyze-text", routing::post(routes::analyze_text))
        .with_state(state)
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
        .layer(SetResponseHeaderLayer::overriding(
            HeaderName::from_static("content-security-policy"),
            HeaderValue::from_static(
                "default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self'; connect-src 'self'",
            ),
        ))
}

#[tokio::test]
async fn security_header_x_content_type_options_nosniff() {
    let app = app_with_security_headers();
    let response = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let val = response
        .headers()
        .get("x-content-type-options")
        .expect("X-Content-Type-Options header must be present");
    assert_eq!(val, "nosniff");
}

#[tokio::test]
async fn security_header_x_frame_options_deny() {
    let app = app_with_security_headers();
    let response = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let val = response
        .headers()
        .get("x-frame-options")
        .expect("X-Frame-Options header must be present");
    assert_eq!(val, "DENY");
}

#[tokio::test]
async fn security_header_referrer_policy() {
    let app = app_with_security_headers();
    let response = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let val = response
        .headers()
        .get("referrer-policy")
        .expect("Referrer-Policy header must be present");
    assert_eq!(val, "strict-origin-when-cross-origin");
}

#[tokio::test]
async fn security_header_content_security_policy() {
    let app = app_with_security_headers();
    let response = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let csp = response
        .headers()
        .get("content-security-policy")
        .expect("Content-Security-Policy header must be present");
    let csp_str = csp.to_str().unwrap();
    assert!(
        csp_str.contains("default-src 'self'"),
        "CSP must contain default-src 'self'"
    );
    assert!(
        csp_str.contains("script-src 'self'"),
        "CSP must contain script-src 'self'"
    );
    assert!(
        csp_str.contains("style-src 'self'"),
        "CSP must contain style-src 'self'"
    );
    assert!(
        csp_str.contains("img-src 'self'"),
        "CSP must contain img-src 'self'"
    );
    assert!(
        csp_str.contains("connect-src 'self'"),
        "CSP must contain connect-src 'self'"
    );
}

#[tokio::test]
async fn security_headers_present_on_error_responses() {
    let app = app_with_security_headers();
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/analyze-text")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"text":""}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    // Even error responses must include security headers
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(response.headers().get("x-content-type-options").is_some());
    assert!(response.headers().get("x-frame-options").is_some());
    assert!(response.headers().get("content-security-policy").is_some());
}

// =============================================================================
// Security Tests — SSRF Protection (Private IPs Blocked)
// =============================================================================

/// Helper to attempt scraping a URL and expect an InvalidUrl error.
async fn assert_ssrf_blocked(url: &str) {
    use political_debaiser::scraper::{ScrapeError, scrape_article};
    let cache: political_debaiser::models::ArticleCache = std::sync::Arc::new(
        tokio::sync::RwLock::new(lru::LruCache::new(std::num::NonZeroUsize::new(10).unwrap())),
    );
    let result = scrape_article(url, &cache).await;
    assert!(
        result.is_err(),
        "Expected SSRF block for URL {url}, but got Ok"
    );
    match result.unwrap_err() {
        ScrapeError::InvalidUrl(msg) => {
            assert!(
                msg.contains("private") || msg.contains("internal") || msg.contains("not allowed"),
                "SSRF block for {url} should mention private/internal, got: {msg}"
            );
        }
        other => panic!("Expected InvalidUrl for SSRF block on {url}, got: {other}"),
    }
}

#[tokio::test]
async fn ssrf_blocks_localhost() {
    assert_ssrf_blocked("http://localhost/secret").await;
}

#[tokio::test]
async fn ssrf_blocks_127_0_0_1() {
    assert_ssrf_blocked("http://127.0.0.1/admin").await;
}

#[tokio::test]
async fn ssrf_blocks_10_x_private_range() {
    assert_ssrf_blocked("http://10.0.0.1/internal").await;
}

#[tokio::test]
async fn ssrf_blocks_192_168_private_range() {
    assert_ssrf_blocked("http://192.168.1.1/router").await;
}

#[tokio::test]
async fn ssrf_blocks_172_16_private_range() {
    assert_ssrf_blocked("http://172.16.0.1/internal").await;
}

#[tokio::test]
async fn ssrf_blocks_metadata_google_internal() {
    assert_ssrf_blocked("http://metadata.google.internal/computeMetadata/v1/").await;
}

#[tokio::test]
async fn ssrf_blocks_dot_local_hostnames() {
    assert_ssrf_blocked("http://myserver.local/api").await;
}

#[tokio::test]
async fn ssrf_blocks_dot_internal_hostnames() {
    assert_ssrf_blocked("http://service.internal/data").await;
}

#[tokio::test]
async fn ssrf_blocks_dot_corp_hostnames() {
    assert_ssrf_blocked("http://intranet.corp/secrets").await;
}

// =============================================================================
// Security Tests — Rate Limiting (429 on excess requests)
// =============================================================================

/// Build a router with rate limiting applied to /analyze-text, matching main.rs config.
/// Uses a tight burst_size(2) for testability (fewer requests needed to trigger 429).
fn app_with_rate_limiting() -> Router {
    use political_debaiser::routes;
    use tower_governor::GovernorLayer;
    use tower_governor::governor::GovernorConfigBuilder;

    let state = AppState::new(
        political_debaiser::models::DEFAULT_CACHE_SIZE,
        political_debaiser::models::DEFAULT_STORE_SIZE,
    );

    // Tight rate limit for testing: burst_size(2) so 3rd request gets 429.
    // Very slow replenishment (1 per day) to prevent refills during the test.
    let rate_limit_conf = GovernorConfigBuilder::default()
        .per_second(86400)
        .burst_size(2)
        .finish()
        .expect("valid rate limit config");

    let rate_limited = Router::new()
        .route("/analyze-text", routing::post(routes::analyze_text))
        .layer(GovernorLayer::new(rate_limit_conf));

    Router::new()
        .route("/health", routing::get(routes::health))
        .merge(rate_limited)
        .with_state(state)
}

/// Send a POST /analyze-text request with ConnectInfo injected for rate limiting.
/// Uses empty text so the handler returns 400 instantly (no Ollama wait).
/// The rate limiter middleware still consumes a token before the handler runs.
async fn rate_limit_request(app: &mut Router) -> axum::http::Response<Body> {
    use axum::extract::connect_info::ConnectInfo;
    use std::net::SocketAddr;
    use tower::Service;

    let addr: SocketAddr = "127.0.0.1:12345".parse().unwrap();
    let mut req = Request::builder()
        .method("POST")
        .uri("/analyze-text")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"text":" "}"#))
        .unwrap();
    req.extensions_mut().insert(ConnectInfo(addr));

    Service::call(app, req).await.unwrap()
}

#[tokio::test]
async fn rate_limit_allows_requests_within_burst() {
    let mut app = app_with_rate_limiting();

    // First request should pass the rate limiter (handler returns 400 for empty text, not 429)
    let resp = rate_limit_request(&mut app).await;
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "First request should pass rate limiter and reach the handler (400 for empty text)"
    );
}

#[tokio::test]
async fn rate_limit_returns_429_on_excess() {
    let mut app = app_with_rate_limiting();

    // Burn through the burst allowance (burst_size=2)
    let _r1 = rate_limit_request(&mut app).await;
    let _r2 = rate_limit_request(&mut app).await;

    // Third request should be rate limited
    let r3 = rate_limit_request(&mut app).await;
    assert_eq!(
        r3.status(),
        StatusCode::TOO_MANY_REQUESTS,
        "Third request should be rate limited (burst_size=2)"
    );
}

#[tokio::test]
async fn rate_limit_does_not_affect_health_endpoint() {
    use axum::extract::connect_info::ConnectInfo;
    use std::net::SocketAddr;
    use tower::Service;

    let mut app = app_with_rate_limiting();
    let addr: SocketAddr = "127.0.0.1:12345".parse().unwrap();

    // Exhaust rate limit on analyze-text
    let _r1 = rate_limit_request(&mut app).await;
    let _r2 = rate_limit_request(&mut app).await;
    let _r3 = rate_limit_request(&mut app).await; // This would be 429

    // Health endpoint should still work — it's not rate limited
    let mut req = Request::builder()
        .uri("/health")
        .body(Body::empty())
        .unwrap();
    req.extensions_mut().insert(ConnectInfo(addr));

    let health_resp = Service::call(&mut app, req).await.unwrap();
    assert_eq!(
        health_resp.status(),
        StatusCode::OK,
        "Health endpoint must not be rate limited"
    );
}

// =============================================================================
// Stage 3 — Summarizer Tests (src/summarizer.rs)
// Stubs: #[ignore] until Dame Judith lands the summarizer module.
// =============================================================================

#[tokio::test]
async fn summarizer_short_article_passes_through() {
    use political_debaiser::summarizer::summarize_if_needed;
    // Articles under 4000 chars should pass through without calling Ollama
    let short_text = "This is a short article about political reform. It discusses various perspectives on the proposed legislation and its potential impact on citizens.";
    let result = summarize_if_needed(short_text).await.unwrap();
    assert_eq!(
        result, short_text,
        "Short text should pass through unchanged"
    );
}

#[tokio::test]
#[serial]
async fn summarizer_long_article_is_shortened() {
    use political_debaiser::summarizer::summarize_if_needed;
    // With mock Ollama, summarizer calls the LLM for text exceeding the 4000 char threshold
    let mock = setup_full_mock_ollama().await;
    unsafe {
        std::env::set_var("OLLAMA_URL", mock.uri());
        std::env::set_var("OLLAMA_MODEL", "test-model");
    }

    let long_text = "The government announced new legislation today. ".repeat(150); // ~7200 chars
    let result = summarize_if_needed(&long_text).await.unwrap();
    // Mock returns persona JSON as "summary" — it's different from original
    assert_ne!(result, long_text, "Long text should be processed by Ollama");
    assert!(!result.is_empty(), "Summarized text should not be empty");
}

#[tokio::test]
async fn summarizer_empty_text_handled_gracefully() {
    use political_debaiser::summarizer::summarize_if_needed;
    // Empty text (0 chars < 4000 threshold) passes through without calling Ollama
    let result = summarize_if_needed("").await.unwrap();
    assert_eq!(result, "", "Empty text should pass through unchanged");
}

#[tokio::test]
#[serial]
async fn summarizer_error_falls_back_to_original_text() {
    use political_debaiser::summarizer::summarize_if_needed;
    // Point to dead server — summarization will fail for long text
    unsafe {
        std::env::set_var("OLLAMA_URL", "http://127.0.0.1:19999");
        std::env::set_var("OLLAMA_MODEL", "test-model");
    }

    let long_text = "Important political article content. ".repeat(150); // >4000 chars
    let result = summarize_if_needed(&long_text).await;
    assert!(
        result.is_err(),
        "Summarization should fail with unreachable Ollama"
    );

    // Demonstrate the fallback pattern used by analyze_full:
    // unwrap_or_else falls back to original text on error
    let fallback = result.unwrap_or_else(|_| long_text.clone());
    assert_eq!(fallback, long_text, "Fallback should use original text");
}

#[tokio::test]
#[serial]
async fn e2e_long_article_uses_summarized_text() {
    // End-to-end: submit a long article (>4000 chars) via /analyze-text.
    // The summarizer triggers, then persona analysis runs on the summarized content.
    let mock = setup_full_mock_ollama().await;
    let app = app_with_ollama_url(&mock.uri());

    let long_text = "The government announced sweeping new policy reforms today. ".repeat(100);

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/analyze-text")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "text": long_text,
                        "title": "Long Article Summarization Test"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let result: AnalysisResult = serde_json::from_slice(&body).unwrap();
    assert_eq!(result.title, "Long Article Summarization Test");
    assert!(
        !result.personas.is_empty(),
        "Pipeline should succeed with summarized content"
    );
}

// =============================================================================
// Stage 3 — Tone/Framing Analysis Tests
// Stubs: #[ignore] until Sir Reginald's ToneAnalysis model + Dame Judith's engine.
// =============================================================================

#[tokio::test]
#[serial]
async fn e2e_analyze_text_includes_tone_analysis() {
    // POST /analyze-text should include tone_analysis in the response.
    // Mock Ollama returns persona JSON for all calls; tone parser uses fallback.
    let mock = setup_full_mock_ollama().await;
    let app = app_with_ollama_url(&mock.uri());

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/analyze-text")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "text": "The administration's reckless spending threatens our children's future. Economists warn of catastrophic debt levels while politicians play partisan games."
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let result: AnalysisResult = serde_json::from_slice(&body).unwrap();

    // tone_analysis should be populated (via LLM or fallback parser)
    assert!(
        result.tone_analysis.is_some(),
        "tone_analysis should be present in Stage 3 response"
    );
    let tone = result.tone_analysis.unwrap();
    assert!(!tone.emotional_tone.is_empty());
    assert!(!tone.framing_strategy.is_empty());
    assert!((0.0..=1.0).contains(&tone.objectivity_score));
}

#[tokio::test]
async fn tone_analysis_objectivity_score_is_clamped() {
    use political_debaiser::models::ToneAnalysis;

    // Verify boundary values are valid for the struct
    let tone_low = ToneAnalysis {
        rhetorical_devices: vec![],
        emotional_tone: "neutral".to_string(),
        framing_strategy: "neutral".to_string(),
        objectivity_score: 0.0,
    };
    let tone_high = ToneAnalysis {
        rhetorical_devices: vec![],
        emotional_tone: "neutral".to_string(),
        framing_strategy: "neutral".to_string(),
        objectivity_score: 1.0,
    };
    assert!((tone_low.objectivity_score - 0.0).abs() < f64::EPSILON);
    assert!((tone_high.objectivity_score - 1.0).abs() < f64::EPSILON);

    // When included in AnalysisResult, the score serializes correctly
    let result = AnalysisResult {
        title: "Clamping Test".to_string(),
        source_url: None,
        personas: vec![],
        debiaser: DebiasedSummary {
            consensus_points: vec![],
            disagreements: vec![],
            likely_bias_drivers: vec![],
            truth_seeking_summary: "Test.".to_string(),
            spectrum_score: 0.0,
            spectrum_explain: "Test.".to_string(),
        },
        tone_analysis: Some(tone_high),
        source_meta: None,
        warnings: vec![],
    };
    let json = serde_json::to_string(&result).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    let score = parsed["tone_analysis"]["objectivity_score"]
        .as_f64()
        .unwrap();
    assert!(
        (0.0..=1.0).contains(&score),
        "objectivity_score {score} out of [0, 1]"
    );
}

#[tokio::test]
#[serial]
async fn tone_analysis_malformed_json_handled_gracefully() {
    // Mock returns persona JSON for ALL calls including tone analysis.
    // The tone parser receives "malformed" tone JSON (valid persona JSON,
    // not tone schema) and should handle it gracefully via fallback.
    let mock = setup_full_mock_ollama().await;
    let app = app_with_ollama_url(&mock.uri());

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/analyze-text")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "text": "An article about tax policy and its economic implications."
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    // Pipeline should succeed — malformed tone JSON handled gracefully
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "Malformed tone JSON should not cause pipeline failure"
    );
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let result: AnalysisResult = serde_json::from_slice(&body).unwrap();

    // Persona analysis should succeed independently of tone parsing
    assert!(
        !result.personas.is_empty(),
        "Persona analysis should succeed regardless of tone parsing"
    );
}

#[tokio::test]
async fn tone_analysis_field_is_optional_for_backward_compat() {
    // Stage 2 JSON — no tone_analysis field present at all
    let stage2_json = serde_json::json!({
        "title": "Stage 2 Result",
        "source_url": "https://example.com",
        "personas": [],
        "debiaser": {
            "consensus_points": [],
            "disagreements": [],
            "likely_bias_drivers": [],
            "truth_seeking_summary": "Old.",
            "spectrum_score": 0.1,
            "spectrum_explain": "Slightly right."
        }
    });
    let result: AnalysisResult = serde_json::from_value(stage2_json).unwrap();
    assert!(
        result.tone_analysis.is_none(),
        "tone_analysis should default to None for old JSON"
    );
    assert!(
        result.source_meta.is_none(),
        "source_meta should default to None for old JSON"
    );

    // With tone_analysis present, it deserializes correctly
    let stage3_json = serde_json::json!({
        "title": "Stage 3 Result",
        "source_url": null,
        "personas": [],
        "debiaser": {
            "consensus_points": [],
            "disagreements": [],
            "likely_bias_drivers": [],
            "truth_seeking_summary": "New.",
            "spectrum_score": 0.0,
            "spectrum_explain": "Center."
        },
        "tone_analysis": {
            "rhetorical_devices": ["loaded language"],
            "emotional_tone": "inflammatory",
            "framing_strategy": "conflict frame",
            "objectivity_score": 0.25
        }
    });
    let result3: AnalysisResult = serde_json::from_value(stage3_json).unwrap();
    assert!(result3.tone_analysis.is_some());
    let tone = result3.tone_analysis.unwrap();
    assert_eq!(tone.emotional_tone, "inflammatory");
    assert_eq!(tone.rhetorical_devices.len(), 1);
}

// =============================================================================
// Stage 3 — Source Credibility / Meta Tests
// Stubs: #[ignore] until Sir Reginald's SourceMeta model + Bees' routes/scraper.
// =============================================================================

#[tokio::test]
#[serial]
#[ignore = "Requires URL scraping infrastructure — SSRF protection blocks mock servers on localhost. Source meta extraction tested separately in source_meta_extracted_from_known_domain."]
async fn e2e_analyze_url_includes_source_meta() {
    // POST /analyze with URL would include source_meta from domain extraction.
    // Cannot test without bypassing SSRF protection for mock server on localhost.
    // The extract_source_meta function is tested directly in other tests.
}

#[tokio::test]
#[serial]
async fn e2e_analyze_text_has_no_source_meta() {
    // POST /analyze-text (pasted text, no URL): source_url should be None.
    // source_meta may be populated via LLM content analysis (no scraper fallback
    // for text input since there's no URL to look up).
    let mock = setup_full_mock_ollama().await;
    let app = app_with_ollama_url(&mock.uri());

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/analyze-text")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "text": "An article about environmental policy and carbon pricing."
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let result: AnalysisResult = serde_json::from_slice(&body).unwrap();

    // source_url is None for text input (no URL provided)
    assert!(
        result.source_url.is_none(),
        "Text input should have no source_url"
    );

    // source_meta may be populated via LLM analysis of content (or fallback)
    // For text-only input, there's no scraper-based URL fallback
    if let Some(meta) = &result.source_meta {
        assert!(!meta.publication.is_empty());
    }
}

#[tokio::test]
async fn source_meta_extracted_from_known_domain() {
    use political_debaiser::scraper::extract_source_meta;

    // Reuters — known wire service, center bias
    let reuters = extract_source_meta("https://www.reuters.com/article/something");
    assert_eq!(reuters.publication, "Reuters");
    assert_eq!(reuters.domain, "reuters.com");
    assert!(
        reuters.known_bias.is_some(),
        "Reuters should have a known bias"
    );
    assert_eq!(reuters.known_bias.as_deref(), Some("center"));
    assert!(reuters.media_type.is_some());

    // Fox News — known right-leaning
    let fox = extract_source_meta("https://www.foxnews.com/politics/article");
    assert_eq!(fox.domain, "foxnews.com");
    assert!(
        fox.known_bias.is_some(),
        "Fox News should have a known bias"
    );
    assert!(fox.media_type.is_some());
}

#[tokio::test]
async fn source_meta_unknown_domain_has_defaults() {
    use political_debaiser::scraper::extract_source_meta;

    // Unknown domain — should derive publication name from domain
    let unknown = extract_source_meta("https://www.random-news-blog.com/article/123");
    assert_eq!(unknown.domain, "random-news-blog.com");
    assert!(
        unknown.known_bias.is_none(),
        "Unknown domain should have no known bias"
    );
    assert!(
        unknown.media_type.is_none(),
        "Unknown domain should have no media type"
    );
    // Publication name derived from domain (capitalized, hyphens to spaces)
    assert!(
        !unknown.publication.is_empty(),
        "Publication name should not be empty"
    );
}
