// =============================================================================
// Stage 6 — Consistency & Determinism Regression Tests
//
// Validates that temperature=0 is set in all LLM API requests and that the
// analysis pipeline produces deterministic results for identical inputs.
//
// Uses wiremock to mock LLM providers and verify request body contents.
// All tests use #[serial] to avoid env var races with e2e_tests.
// =============================================================================

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::routing;
use http_body_util::BodyExt;
use serial_test::serial;
use tower::ServiceExt;
use tower_http::cors::CorsLayer;
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use political_debaiser::archetypes;
use political_debaiser::models::{
    AnalysisResult, AppState, Axes2D, DebiasedSummary, FactCheck, FactCheckAssessment, PersonaId,
    PersonaOutput,
};

// =============================================================================
// Test fixtures
// =============================================================================

/// A fixed persona JSON response for determinism testing.
/// All values are intentionally distinct to verify exact parsing fidelity.
fn fixture_persona_json() -> String {
    serde_json::json!({
        "stance_score": -1.2,
        "confidence": 0.85,
        "summary": "This article raises significant civil rights concerns about government overreach.",
        "key_claims": [
            "The policy disproportionately affects marginalized communities",
            "Surveillance expansion has a chilling effect on free speech"
        ],
        "fact_checks": [{
            "claim": "The regulation targets specific communities",
            "assessment": "supported",
            "rationale": "Multiple studies confirm disproportionate impact"
        }],
        "caveats": ["May underweight legitimate security concerns"],
        "axes": {
            "economic": -0.8,
            "social": -1.5
        }
    })
    .to_string()
}

/// A fixed debiased synthesis JSON response for determinism testing.
fn fixture_debiased_json() -> String {
    serde_json::json!({
        "consensus_points": [
            "All perspectives agree the policy has significant implications",
            "Multiple viewpoints note the need for oversight mechanisms"
        ],
        "disagreements": [
            "Progressive and libertarian perspectives emphasize liberty, security hawks emphasize safety"
        ],
        "likely_bias_drivers": [
            "Security-first framing in the original article"
        ],
        "truth_seeking_summary": "The policy addresses real security concerns but risks disproportionate impact on civil liberties.",
        "spectrum_explain": "The weighted analysis tilts slightly toward liberty concerns."
    })
    .to_string()
}

/// Wrap content as an Ollama chat response.
fn ollama_response(content: &str) -> serde_json::Value {
    serde_json::json!({
        "model": "test-model",
        "message": {
            "role": "assistant",
            "content": content
        },
        "done": true
    })
}

/// Build the real app router pointing to a mock Ollama URL.
fn app_with_ollama(ollama_url: &str) -> Router {
    // SAFETY: Tests are serialized via #[serial] to prevent env var races.
    unsafe {
        std::env::set_var("LLM_PROVIDERS", "ollama");
        std::env::set_var("OLLAMA_URL", ollama_url);
        std::env::set_var("OLLAMA_MODEL", "test-model");
        std::env::set_var("LLM_TIMEOUT", "30");
    }

    use political_debaiser::routes;

    let state = AppState::new(
        political_debaiser::models::DEFAULT_CACHE_SIZE,
        political_debaiser::models::DEFAULT_STORE_SIZE,
    );

    Router::new()
        .route("/health", routing::get(routes::health))
        .route("/analyze-text", routing::post(routes::analyze_text))
        .with_state(state)
        .layer(CorsLayer::permissive())
}

/// The fixed article text used for all determinism tests.
/// Intentionally short (<4000 chars) to skip summarization.
const FIXTURE_ARTICLE: &str = "The government announced sweeping new surveillance \
    regulations today, requiring technology companies to provide backdoor access \
    to encrypted communications. Privacy advocates condemned the move as an \
    unprecedented expansion of state power, while national security officials \
    argued it was necessary to combat emerging threats. The legislation passed \
    along party lines with supporters citing recent intelligence failures.";

/// Helper: build a PersonaOutput with specified values for spectrum calculation tests.
fn make_persona(id: PersonaId, stance: f64, confidence: f64) -> PersonaOutput {
    PersonaOutput {
        id,
        title: "Test".to_string(),
        stance_score: stance,
        confidence,
        summary: "Test summary.".to_string(),
        key_claims: vec!["Claim".to_string()],
        fact_checks: vec![],
        caveats: vec![],
        axes: Some(Axes2D {
            economic: stance * 0.5,
            social: stance,
        }),
    }
}

// =============================================================================
// Temperature=0 Verification Tests
//
// These verify that temperature=0 is included in LLM API request bodies.
// The body construction in call_provider is shared across ALL providers
// (Ollama, Groq, Gemini, HuggingFace) — only the URL path differs.
// Verifying it with Ollama confirms it for all providers.
// =============================================================================

/// Verify that temperature=0 is included in Ollama API requests.
#[tokio::test]
#[serial]
async fn temperature_zero_sent_in_ollama_requests() {
    let server = MockServer::start().await;

    unsafe {
        std::env::set_var("LLM_PROVIDERS", "ollama");
        std::env::set_var("OLLAMA_URL", &server.uri());
        std::env::set_var("OLLAMA_MODEL", "test-model");
    }

    // This mock ONLY matches if temperature=0 is in the request body.
    // If temperature is missing or non-zero, the mock won't match,
    // the request gets a 404, and call_llm fails.
    Mock::given(method("POST"))
        .and(path("/api/chat"))
        .and(body_partial_json(serde_json::json!({"temperature": 0})))
        .respond_with(ResponseTemplate::new(200).set_body_json(ollama_response("test response")))
        .expect(1)
        .mount(&server)
        .await;

    let result = political_debaiser::llm::call_llm("system prompt", "user message").await;
    assert!(
        result.is_ok(),
        "call_llm failed — temperature=0 may not be in request body: {:?}",
        result.err()
    );
}

/// Verify the full request body structure: model, messages, stream, temperature.
#[tokio::test]
#[serial]
async fn request_body_includes_all_required_fields() {
    let server = MockServer::start().await;

    unsafe {
        std::env::set_var("LLM_PROVIDERS", "ollama");
        std::env::set_var("OLLAMA_URL", &server.uri());
        std::env::set_var("OLLAMA_MODEL", "test-model");
    }

    Mock::given(method("POST"))
        .and(path("/api/chat"))
        .and(body_partial_json(serde_json::json!({
            "model": "test-model",
            "stream": false,
            "temperature": 0,
            "messages": [
                {"role": "system", "content": "sys prompt"},
                {"role": "user", "content": "usr message"}
            ]
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(ollama_response("verified")))
        .expect(1)
        .mount(&server)
        .await;

    let result = political_debaiser::llm::call_llm("sys prompt", "usr message").await;
    assert!(
        result.is_ok(),
        "Request body missing required fields: {:?}",
        result.err()
    );
}

/// Verify temperature=0 is present in the request body for each provider type.
/// Since call_provider uses the same serde_json::json! body for ALL providers,
/// testing Ollama confirms the body structure for Groq, Gemini, and HuggingFace.
/// This test documents that architectural guarantee.
#[tokio::test]
#[serial]
async fn temperature_zero_shared_body_construction() {
    let server = MockServer::start().await;

    unsafe {
        std::env::set_var("LLM_PROVIDERS", "ollama");
        std::env::set_var("OLLAMA_URL", &server.uri());
        std::env::set_var("OLLAMA_MODEL", "test-model");
    }

    // Strict partial match: temperature=0 + stream=false must both be present
    Mock::given(method("POST"))
        .and(path("/api/chat"))
        .and(body_partial_json(
            serde_json::json!({"temperature": 0, "stream": false}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(ollama_response("ok")))
        .expect(1)
        .mount(&server)
        .await;

    let result = political_debaiser::llm::call_llm("sys", "usr").await;
    assert!(result.is_ok());
}

// =============================================================================
// Deterministic Persona Output Tests
// =============================================================================

/// Verify that calling analyze_persona twice with the same mock response
/// produces identical PersonaOutput structs.
#[tokio::test]
#[serial]
async fn same_input_produces_identical_persona_output() {
    let server = MockServer::start().await;

    unsafe {
        std::env::set_var("LLM_PROVIDERS", "ollama");
        std::env::set_var("OLLAMA_URL", &server.uri());
        std::env::set_var("OLLAMA_MODEL", "test-model");
    }

    Mock::given(method("POST"))
        .and(path("/api/chat"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(ollama_response(&fixture_persona_json())),
        )
        .expect(2..)
        .mount(&server)
        .await;

    let result1 = archetypes::analyze_persona(FIXTURE_ARTICLE, &PersonaId::ProgressiveActivist)
        .await
        .unwrap();

    let result2 = archetypes::analyze_persona(FIXTURE_ARTICLE, &PersonaId::ProgressiveActivist)
        .await
        .unwrap();

    // Verify all fields are identical
    assert_eq!(result1.id, result2.id);
    assert_eq!(result1.title, result2.title);
    assert!(
        (result1.stance_score - result2.stance_score).abs() < f64::EPSILON,
        "stance_score differs: {} vs {}",
        result1.stance_score,
        result2.stance_score
    );
    assert!(
        (result1.confidence - result2.confidence).abs() < f64::EPSILON,
        "confidence differs: {} vs {}",
        result1.confidence,
        result2.confidence
    );
    assert_eq!(result1.summary, result2.summary);
    assert_eq!(result1.key_claims, result2.key_claims);
    assert_eq!(result1.caveats, result2.caveats);
    assert_eq!(result1.fact_checks.len(), result2.fact_checks.len());

    for (fc1, fc2) in result1.fact_checks.iter().zip(result2.fact_checks.iter()) {
        assert_eq!(fc1.claim, fc2.claim);
        assert_eq!(fc1.assessment, fc2.assessment);
        assert_eq!(fc1.rationale, fc2.rationale);
    }

    let axes1 = result1.axes.unwrap();
    let axes2 = result2.axes.unwrap();
    assert!(
        (axes1.economic - axes2.economic).abs() < f64::EPSILON,
        "axes.economic differs"
    );
    assert!(
        (axes1.social - axes2.social).abs() < f64::EPSILON,
        "axes.social differs"
    );
}

// =============================================================================
// Deterministic Spectrum Score Tests
// =============================================================================

/// Verify spectrum score calculation is deterministic across 100 runs.
#[test]
fn spectrum_score_deterministic_across_100_runs() {
    let personas = vec![
        make_persona(PersonaId::ProgressiveActivist, -2.5, 0.9),
        make_persona(PersonaId::LiberalSocialDemocrat, -1.2, 0.85),
        make_persona(PersonaId::CentristTechnocrat, 0.1, 0.8),
        make_persona(PersonaId::LibertarianCivil, -1.8, 0.75),
        make_persona(PersonaId::ConservativeFiscal, 1.5, 0.7),
        make_persona(PersonaId::NationalSecurityHawk, 2.3, 0.65),
        make_persona(PersonaId::EnvironmentalistGreen, -2.0, 0.88),
        make_persona(PersonaId::PopulistAntiElite, -0.5, 0.72),
    ];

    let first = archetypes::fallback_debiaser(&personas);
    for i in 1..100 {
        let result = archetypes::fallback_debiaser(&personas);
        assert!(
            (result.spectrum_score - first.spectrum_score).abs() < f64::EPSILON,
            "Spectrum score changed on iteration {i}: expected {}, got {}",
            first.spectrum_score,
            result.spectrum_score
        );
    }
}

/// Verify spectrum score with known fixture data produces exact expected value.
#[test]
fn spectrum_score_regression_known_values() {
    // All 8 personas with equal confidence=0.8 and varying stances.
    // Weighted mean = sum(stance * 0.8) / (0.8 * 8) = simple average of stances.
    let personas = vec![
        make_persona(PersonaId::ProgressiveActivist, -2.0, 0.8),
        make_persona(PersonaId::LiberalSocialDemocrat, -1.0, 0.8),
        make_persona(PersonaId::CentristTechnocrat, 0.0, 0.8),
        make_persona(PersonaId::LibertarianCivil, -1.5, 0.8),
        make_persona(PersonaId::ConservativeFiscal, 1.0, 0.8),
        make_persona(PersonaId::NationalSecurityHawk, 2.0, 0.8),
        make_persona(PersonaId::EnvironmentalistGreen, -1.0, 0.8),
        make_persona(PersonaId::PopulistAntiElite, -0.5, 0.8),
    ];

    // sum of stances = -2.0 + -1.0 + 0.0 + -1.5 + 1.0 + 2.0 + -1.0 + -0.5 = -3.0
    // simple average = -3.0 / 8 = -0.375
    // rounded: (-0.375 * 100.0).round() / 100.0 = -0.38
    let result = archetypes::fallback_debiaser(&personas);
    assert!(
        (result.spectrum_score - (-0.38)).abs() < 0.01,
        "Expected spectrum_score ~ -0.38, got {}",
        result.spectrum_score
    );
}

/// Verify that equal confidence + opposite stances cancel to zero.
#[test]
fn spectrum_score_equal_confidence_cancels_to_zero() {
    let personas = vec![
        make_persona(PersonaId::ProgressiveActivist, -3.0, 1.0),
        make_persona(PersonaId::NationalSecurityHawk, 3.0, 1.0),
    ];

    let result = archetypes::fallback_debiaser(&personas);
    assert!(
        (result.spectrum_score - 0.0).abs() < f64::EPSILON,
        "Equal-weight opposite stances should cancel to 0.0, got {}",
        result.spectrum_score
    );
}

/// Verify that asymmetric confidence produces expected weighted result.
#[test]
fn spectrum_score_asymmetric_confidence_regression() {
    let personas = vec![
        make_persona(PersonaId::ProgressiveActivist, -2.0, 0.9),
        make_persona(PersonaId::NationalSecurityHawk, 2.0, 0.3),
    ];

    // weighted = (-2.0 * 0.9 + 2.0 * 0.3) / (0.9 + 0.3)
    //          = (-1.8 + 0.6) / 1.2
    //          = -1.2 / 1.2
    //          = -1.0
    let result = archetypes::fallback_debiaser(&personas);
    assert!(
        (result.spectrum_score - (-1.0)).abs() < 0.01,
        "Expected spectrum_score ~ -1.0, got {}",
        result.spectrum_score
    );
}

// =============================================================================
// Deterministic Debiased Summary Tests
// =============================================================================

/// Verify that synthesize_debiased with same mock produces identical output.
#[tokio::test]
#[serial]
async fn same_personas_produce_identical_debiased_summary() {
    let server = MockServer::start().await;

    unsafe {
        std::env::set_var("LLM_PROVIDERS", "ollama");
        std::env::set_var("OLLAMA_URL", &server.uri());
        std::env::set_var("OLLAMA_MODEL", "test-model");
    }

    Mock::given(method("POST"))
        .and(path("/api/chat"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(ollama_response(&fixture_debiased_json())),
        )
        .expect(2)
        .mount(&server)
        .await;

    let personas = vec![
        make_persona(PersonaId::ProgressiveActivist, -2.0, 0.8),
        make_persona(PersonaId::NationalSecurityHawk, 2.0, 0.6),
    ];

    let result1 = archetypes::synthesize_debiased(&personas).await.unwrap();
    let result2 = archetypes::synthesize_debiased(&personas).await.unwrap();

    assert_eq!(result1.consensus_points, result2.consensus_points);
    assert_eq!(result1.disagreements, result2.disagreements);
    assert_eq!(result1.likely_bias_drivers, result2.likely_bias_drivers);
    assert_eq!(result1.truth_seeking_summary, result2.truth_seeking_summary);
    assert!(
        (result1.spectrum_score - result2.spectrum_score).abs() < f64::EPSILON,
        "spectrum_score differs: {} vs {}",
        result1.spectrum_score,
        result2.spectrum_score
    );
    assert_eq!(result1.spectrum_explain, result2.spectrum_explain);
}

/// Verify that fallback_debiaser is perfectly deterministic (pure calculation, no LLM).
#[test]
fn fallback_debiaser_deterministic_across_10_runs() {
    let personas = vec![
        make_persona(PersonaId::ProgressiveActivist, -2.5, 0.9),
        make_persona(PersonaId::LiberalSocialDemocrat, -1.0, 0.85),
        make_persona(PersonaId::CentristTechnocrat, 0.3, 0.8),
        make_persona(PersonaId::LibertarianCivil, -2.0, 0.75),
        make_persona(PersonaId::ConservativeFiscal, 1.8, 0.7),
        make_persona(PersonaId::NationalSecurityHawk, 2.5, 0.65),
        make_persona(PersonaId::EnvironmentalistGreen, -1.5, 0.88),
        make_persona(PersonaId::PopulistAntiElite, -0.3, 0.72),
    ];

    let first = archetypes::fallback_debiaser(&personas);
    for i in 1..10 {
        let result = archetypes::fallback_debiaser(&personas);
        assert!(
            (result.spectrum_score - first.spectrum_score).abs() < f64::EPSILON,
            "Fallback debiaser spectrum_score differs on run {i}"
        );
        assert_eq!(
            result.truth_seeking_summary, first.truth_seeking_summary,
            "Fallback debiaser summary differs on run {i}"
        );
        assert_eq!(
            result.spectrum_explain, first.spectrum_explain,
            "Fallback debiaser explain differs on run {i}"
        );
    }
}

// =============================================================================
// Full Pipeline Determinism Tests
// =============================================================================

/// Verify the full analysis pipeline produces identical results for identical inputs.
#[tokio::test]
#[serial]
async fn full_pipeline_deterministic_with_same_mock() {
    let server = MockServer::start().await;
    let app = app_with_ollama(&server.uri());

    // All LLM calls return persona JSON. The debiased synthesis will use
    // the fallback (persona JSON won't parse as debiased), which is deterministic.
    // Tone analysis and source credibility will also fail gracefully.
    Mock::given(method("POST"))
        .and(path("/api/chat"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(ollama_response(&fixture_persona_json())),
        )
        .expect(1..)
        .mount(&server)
        .await;

    let body = serde_json::json!({
        "text": FIXTURE_ARTICLE,
        "title": "Surveillance Regulation Article"
    })
    .to_string();

    // First request
    let response1 = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/analyze-text")
                .header("content-type", "application/json")
                .body(Body::from(body.clone()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response1.status(), StatusCode::OK);
    let body1 = response1.into_body().collect().await.unwrap().to_bytes();
    let result1: AnalysisResult = serde_json::from_slice(&body1).unwrap();

    // Second request with identical input
    let response2 = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/analyze-text")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response2.status(), StatusCode::OK);
    let body2 = response2.into_body().collect().await.unwrap().to_bytes();
    let result2: AnalysisResult = serde_json::from_slice(&body2).unwrap();

    // Compare top-level fields
    assert_eq!(result1.title, result2.title);
    assert_eq!(result1.source_url, result2.source_url);
    assert_eq!(
        result1.personas.len(),
        result2.personas.len(),
        "Persona count differs"
    );

    // Compare each persona output (order is deterministic: handles awaited in order)
    for (p1, p2) in result1.personas.iter().zip(result2.personas.iter()) {
        assert_eq!(p1.id, p2.id, "Persona ID order differs");
        assert!(
            (p1.stance_score - p2.stance_score).abs() < f64::EPSILON,
            "stance_score differs for {:?}: {} vs {}",
            p1.id,
            p1.stance_score,
            p2.stance_score
        );
        assert!(
            (p1.confidence - p2.confidence).abs() < f64::EPSILON,
            "confidence differs for {:?}",
            p1.id
        );
        assert_eq!(p1.summary, p2.summary, "summary differs for {:?}", p1.id);
        assert_eq!(
            p1.key_claims, p2.key_claims,
            "key_claims differ for {:?}",
            p1.id
        );
    }

    // Compare debiased summary
    assert!(
        (result1.debiaser.spectrum_score - result2.debiaser.spectrum_score).abs() < f64::EPSILON,
        "Debiaser spectrum_score differs: {} vs {}",
        result1.debiaser.spectrum_score,
        result2.debiaser.spectrum_score
    );
}

/// Verify that a mocked pipeline returns all 8 personas with correct fixture values.
#[tokio::test]
#[serial]
async fn full_pipeline_returns_complete_deterministic_result() {
    let server = MockServer::start().await;
    let app = app_with_ollama(&server.uri());

    Mock::given(method("POST"))
        .and(path("/api/chat"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(ollama_response(&fixture_persona_json())),
        )
        .expect(1..)
        .mount(&server)
        .await;

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/analyze-text")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "text": FIXTURE_ARTICLE,
                        "title": "Completeness Check"
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

    // Should have all 8 personas
    assert_eq!(
        result.personas.len(),
        8,
        "Expected 8 personas, got {}",
        result.personas.len()
    );

    // Each persona should have the exact fixture values
    for persona in &result.personas {
        assert!(
            (persona.stance_score - (-1.2)).abs() < f64::EPSILON,
            "Persona {} has unexpected stance_score: {}",
            persona.title,
            persona.stance_score
        );
        assert!(
            (persona.confidence - 0.85).abs() < f64::EPSILON,
            "Persona {} has unexpected confidence: {}",
            persona.title,
            persona.confidence
        );
        assert_eq!(persona.key_claims.len(), 2);
        assert_eq!(persona.fact_checks.len(), 1);
        assert_eq!(persona.caveats.len(), 1);
    }

    // All personas have identical stance=-1.2, confidence=0.85.
    // Weighted mean = (-1.2 * 0.85 * 8) / (0.85 * 8) = -1.2
    assert!(
        (result.debiaser.spectrum_score - (-1.2)).abs() < 0.01,
        "Expected spectrum_score ~ -1.2, got {}",
        result.debiaser.spectrum_score
    );
}

/// Verify all 8 persona IDs are present in the output.
#[tokio::test]
#[serial]
async fn all_eight_persona_ids_present_in_output() {
    let server = MockServer::start().await;
    let app = app_with_ollama(&server.uri());

    Mock::given(method("POST"))
        .and(path("/api/chat"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(ollama_response(&fixture_persona_json())),
        )
        .expect(1..)
        .mount(&server)
        .await;

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/analyze-text")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({"text": FIXTURE_ARTICLE}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let result: AnalysisResult = serde_json::from_slice(&body).unwrap();

    let expected_ids: Vec<&PersonaId> = PersonaId::all().iter().collect();
    let actual_ids: Vec<&PersonaId> = result.personas.iter().map(|p| &p.id).collect();

    assert_eq!(actual_ids.len(), 8, "Expected 8 personas");
    for expected in &expected_ids {
        assert!(
            actual_ids.contains(expected),
            "Missing persona: {:?}",
            expected
        );
    }
}

// =============================================================================
// JSON Serialization Determinism
// =============================================================================

/// Verify that AnalysisResult serialization is deterministic across runs.
#[test]
fn analysis_result_serialization_deterministic() {
    let result = AnalysisResult {
        title: "Test Article".to_string(),
        source_url: Some("https://example.com".to_string()),
        personas: vec![PersonaOutput {
            id: PersonaId::ProgressiveActivist,
            title: "Progressive Activist".to_string(),
            stance_score: -2.0,
            confidence: 0.85,
            summary: "Civil rights concerns.".to_string(),
            key_claims: vec!["Claim 1".to_string(), "Claim 2".to_string()],
            fact_checks: vec![FactCheck {
                claim: "A claim".to_string(),
                assessment: FactCheckAssessment::Supported,
                rationale: "Reason".to_string(),
            }],
            caveats: vec!["Caveat".to_string()],
            axes: Some(Axes2D {
                economic: -1.0,
                social: -2.0,
            }),
        }],
        debiaser: DebiasedSummary {
            consensus_points: vec!["Agreement".to_string()],
            disagreements: vec!["Disagreement".to_string()],
            likely_bias_drivers: vec!["Bias driver".to_string()],
            truth_seeking_summary: "Balanced summary.".to_string(),
            spectrum_score: -0.42,
            spectrum_explain: "Explanation.".to_string(),
        },
        tone_analysis: None,
        source_meta: None,
        warnings: vec![],
    };

    // Serialize 10 times and verify identical output
    let first_json = serde_json::to_string(&result).unwrap();
    for i in 1..10 {
        let json = serde_json::to_string(&result).unwrap();
        assert_eq!(json, first_json, "Serialization differs on iteration {i}");
    }

    // Verify roundtrip preserves serialization form
    let roundtripped: AnalysisResult = serde_json::from_str(&first_json).unwrap();
    let roundtrip_json = serde_json::to_string(&roundtripped).unwrap();
    assert_eq!(
        roundtrip_json, first_json,
        "Roundtrip changed serialization"
    );
}
