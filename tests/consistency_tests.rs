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
    PersonaOutput, SourceMeta, ToneAnalysis,
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
        std::env::set_var("OLLAMA_URL", server.uri());
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
        std::env::set_var("OLLAMA_URL", server.uri());
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
        std::env::set_var("OLLAMA_URL", server.uri());
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
        std::env::set_var("OLLAMA_URL", server.uri());
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
        std::env::set_var("OLLAMA_URL", server.uri());
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

// =============================================================================
// Persona ID & Title Determinism Tests
// =============================================================================

/// Verify PersonaId::all() returns the same 8 IDs in the same order across 100 runs.
#[test]
fn persona_id_all_order_deterministic_across_100_runs() {
    let first: Vec<PersonaId> = PersonaId::all().to_vec();
    assert_eq!(first.len(), 8);
    for i in 1..100 {
        let current: Vec<PersonaId> = PersonaId::all().to_vec();
        assert_eq!(
            current, first,
            "PersonaId::all() order changed on iteration {i}"
        );
    }
}

/// Verify that each PersonaId maps to the same title across 100 runs.
#[test]
fn persona_id_title_mapping_deterministic_across_100_runs() {
    let first_titles: Vec<(&PersonaId, &str)> = PersonaId::all()
        .iter()
        .map(|id| (id, id.title()))
        .collect();

    for i in 1..100 {
        let current: Vec<(&PersonaId, &str)> = PersonaId::all()
            .iter()
            .map(|id| (id, id.title()))
            .collect();
        assert_eq!(
            current, first_titles,
            "Title mapping changed on iteration {i}"
        );
    }
}

/// Verify all 8 persona titles are unique and non-empty.
#[test]
fn persona_titles_unique_and_nonempty() {
    let titles: Vec<&str> = PersonaId::all().iter().map(|id| id.title()).collect();
    for title in &titles {
        assert!(!title.is_empty(), "Persona title must not be empty");
    }
    let mut sorted = titles.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(
        sorted.len(),
        titles.len(),
        "Persona titles must be unique"
    );
}

// =============================================================================
// Extended Spectrum Score Determinism Tests
// =============================================================================

/// Verify that all-same-stance returns that stance as the weighted score.
#[test]
fn spectrum_score_all_same_stance_returns_that_stance() {
    let personas: Vec<PersonaOutput> = PersonaId::all()
        .iter()
        .map(|id| make_persona(id.clone(), 1.5, 0.7))
        .collect();

    let result = archetypes::fallback_debiaser(&personas);
    assert!(
        (result.spectrum_score - 1.5).abs() < f64::EPSILON,
        "All stances at 1.5 should produce 1.5, got {}",
        result.spectrum_score
    );
}

/// Verify extreme values (-3.0 and +3.0) produce deterministic results.
#[test]
fn spectrum_score_extreme_values_deterministic() {
    let personas = vec![
        make_persona(PersonaId::ProgressiveActivist, -3.0, 1.0),
        make_persona(PersonaId::LiberalSocialDemocrat, -3.0, 0.5),
        make_persona(PersonaId::NationalSecurityHawk, 3.0, 1.0),
        make_persona(PersonaId::ConservativeFiscal, 3.0, 0.5),
    ];

    let first = archetypes::fallback_debiaser(&personas);
    for i in 1..50 {
        let result = archetypes::fallback_debiaser(&personas);
        assert!(
            (result.spectrum_score - first.spectrum_score).abs() < f64::EPSILON,
            "Extreme values spectrum_score changed on iteration {i}: expected {}, got {}",
            first.spectrum_score,
            result.spectrum_score
        );
    }
    // (-3*1 + -3*0.5 + 3*1 + 3*0.5) / (1+0.5+1+0.5) = 0 / 3 = 0
    assert!(
        (first.spectrum_score - 0.0).abs() < f64::EPSILON,
        "Symmetric extreme values should cancel to 0, got {}",
        first.spectrum_score
    );
}

/// Verify 2-decimal rounding is consistent for values that require it.
#[test]
fn spectrum_score_rounding_consistency() {
    // Create a scenario where rounding matters:
    // (-1.0 * 0.3 + 2.0 * 0.7) / (0.3 + 0.7) = (-0.3 + 1.4) / 1.0 = 1.1
    let personas = vec![
        make_persona(PersonaId::ProgressiveActivist, -1.0, 0.3),
        make_persona(PersonaId::NationalSecurityHawk, 2.0, 0.7),
    ];

    let first = archetypes::fallback_debiaser(&personas);
    assert!(
        (first.spectrum_score - 1.1).abs() < 0.01,
        "Expected ~1.1, got {}",
        first.spectrum_score
    );

    // Verify rounding is stable across runs
    for i in 1..100 {
        let result = archetypes::fallback_debiaser(&personas);
        assert!(
            (result.spectrum_score - first.spectrum_score).abs() < f64::EPSILON,
            "Rounding changed on iteration {i}"
        );
    }
}

/// Verify single-persona edge case: spectrum score equals that persona's stance.
#[test]
fn spectrum_score_single_persona_equals_stance() {
    for persona_id in PersonaId::all() {
        let stance = -1.75;
        let personas = vec![make_persona(persona_id.clone(), stance, 0.8)];
        let result = archetypes::fallback_debiaser(&personas);
        assert!(
            (result.spectrum_score - stance).abs() < 0.01,
            "Single persona {:?} should produce stance {}, got {}",
            persona_id,
            stance,
            result.spectrum_score
        );
    }
}

// =============================================================================
// Extended Fallback Debiaser Field Determinism
// =============================================================================

/// Verify fallback_debiaser produces identical values for ALL fields across runs,
/// not just spectrum_score.
#[test]
fn fallback_debiaser_all_fields_identical_across_50_runs() {
    let personas = vec![
        make_persona(PersonaId::ProgressiveActivist, -2.5, 0.9),
        make_persona(PersonaId::LiberalSocialDemocrat, -1.0, 0.85),
        make_persona(PersonaId::CentristTechnocrat, 0.3, 0.8),
        make_persona(PersonaId::ConservativeFiscal, 1.8, 0.7),
        make_persona(PersonaId::NationalSecurityHawk, 2.5, 0.65),
    ];

    let first = archetypes::fallback_debiaser(&personas);
    for i in 1..50 {
        let result = archetypes::fallback_debiaser(&personas);
        assert_eq!(
            result.consensus_points, first.consensus_points,
            "consensus_points changed on iteration {i}"
        );
        assert_eq!(
            result.disagreements, first.disagreements,
            "disagreements changed on iteration {i}"
        );
        assert_eq!(
            result.likely_bias_drivers, first.likely_bias_drivers,
            "likely_bias_drivers changed on iteration {i}"
        );
        assert_eq!(
            result.truth_seeking_summary, first.truth_seeking_summary,
            "truth_seeking_summary changed on iteration {i}"
        );
        assert!(
            (result.spectrum_score - first.spectrum_score).abs() < f64::EPSILON,
            "spectrum_score changed on iteration {i}"
        );
        assert_eq!(
            result.spectrum_explain, first.spectrum_explain,
            "spectrum_explain changed on iteration {i}"
        );
    }
}

/// Verify fallback_debiaser with empty input produces consistent defaults.
#[test]
fn fallback_debiaser_empty_personas_deterministic() {
    let first = archetypes::fallback_debiaser(&[]);
    for i in 1..50 {
        let result = archetypes::fallback_debiaser(&[]);
        assert!(
            (result.spectrum_score - first.spectrum_score).abs() < f64::EPSILON,
            "Empty personas spectrum changed on iteration {i}"
        );
        assert_eq!(result.truth_seeking_summary, first.truth_seeking_summary);
        assert_eq!(result.spectrum_explain, first.spectrum_explain);
    }
    // Empty input should produce 0.0 spectrum
    assert!(
        (first.spectrum_score - 0.0).abs() < f64::EPSILON,
        "Empty personas should produce 0.0, got {}",
        first.spectrum_score
    );
    // Fallback fields should be empty/default
    assert!(first.consensus_points.is_empty());
    assert!(first.disagreements.is_empty());
    assert!(first.likely_bias_drivers.is_empty());
}

// =============================================================================
// ToneAnalysis Serialization Determinism Tests
// =============================================================================

/// Helper: construct a full ToneAnalysis fixture.
fn fixture_tone_analysis() -> ToneAnalysis {
    ToneAnalysis {
        rhetorical_devices: vec![
            "appeal to fear".to_string(),
            "loaded language".to_string(),
            "false dichotomy".to_string(),
        ],
        emotional_tone: "alarmist".to_string(),
        framing_strategy: "conflict frame".to_string(),
        objectivity_score: 0.35,
    }
}

/// Verify ToneAnalysis serialization is deterministic across 50 runs.
#[test]
fn tone_analysis_serialization_deterministic_across_50_runs() {
    let tone = fixture_tone_analysis();
    let first_json = serde_json::to_string(&tone).unwrap();

    for i in 1..50 {
        let json = serde_json::to_string(&tone).unwrap();
        assert_eq!(
            json, first_json,
            "ToneAnalysis serialization differs on iteration {i}"
        );
    }
}

/// Verify ToneAnalysis roundtrip preserves exact serialization form.
#[test]
fn tone_analysis_roundtrip_preserves_serialization() {
    let tone = fixture_tone_analysis();
    let json = serde_json::to_string(&tone).unwrap();
    let roundtripped: ToneAnalysis = serde_json::from_str(&json).unwrap();
    let roundtrip_json = serde_json::to_string(&roundtripped).unwrap();

    assert_eq!(roundtrip_json, json, "ToneAnalysis roundtrip changed serialization");

    // Verify field values survived roundtrip
    assert_eq!(roundtripped.rhetorical_devices.len(), 3);
    assert_eq!(roundtripped.emotional_tone, "alarmist");
    assert_eq!(roundtripped.framing_strategy, "conflict frame");
    assert!((roundtripped.objectivity_score - 0.35).abs() < f64::EPSILON);
}

/// Verify ToneAnalysis with empty rhetorical_devices serializes deterministically.
#[test]
fn tone_analysis_empty_devices_serialization_deterministic() {
    let tone = ToneAnalysis {
        rhetorical_devices: vec![],
        emotional_tone: "neutral".to_string(),
        framing_strategy: "straight news".to_string(),
        objectivity_score: 0.92,
    };

    let first_json = serde_json::to_string(&tone).unwrap();
    for i in 1..20 {
        let json = serde_json::to_string(&tone).unwrap();
        assert_eq!(json, first_json, "Empty-devices tone differs on iteration {i}");
    }

    let roundtripped: ToneAnalysis = serde_json::from_str(&first_json).unwrap();
    assert!(roundtripped.rhetorical_devices.is_empty());
    assert!((roundtripped.objectivity_score - 0.92).abs() < f64::EPSILON);
}

// =============================================================================
// SourceMeta Serialization Determinism Tests
// =============================================================================

/// Verify SourceMeta with all fields serializes deterministically across 50 runs.
#[test]
fn source_meta_serialization_deterministic_across_50_runs() {
    let meta = SourceMeta {
        publication: "The Guardian".to_string(),
        known_bias: Some("center-left".to_string()),
        ownership_type: Some("corporate".to_string()),
    };

    let first_json = serde_json::to_string(&meta).unwrap();
    for i in 1..50 {
        let json = serde_json::to_string(&meta).unwrap();
        assert_eq!(
            json, first_json,
            "SourceMeta serialization differs on iteration {i}"
        );
    }
}

/// Verify SourceMeta with None fields serializes deterministically.
#[test]
fn source_meta_with_nulls_serialization_deterministic() {
    let meta = SourceMeta {
        publication: "Unknown Blog".to_string(),
        known_bias: None,
        ownership_type: None,
    };

    let first_json = serde_json::to_string(&meta).unwrap();
    for i in 1..50 {
        let json = serde_json::to_string(&meta).unwrap();
        assert_eq!(
            json, first_json,
            "SourceMeta with nulls differs on iteration {i}"
        );
    }

    // Verify roundtrip preserves None values
    let roundtripped: SourceMeta = serde_json::from_str(&first_json).unwrap();
    let roundtrip_json = serde_json::to_string(&roundtripped).unwrap();
    assert_eq!(roundtrip_json, first_json);
    assert!(roundtripped.known_bias.is_none());
    assert!(roundtripped.ownership_type.is_none());
}

/// Verify SourceMeta roundtrip preserves all fields exactly.
#[test]
fn source_meta_roundtrip_preserves_all_fields() {
    let meta = SourceMeta {
        publication: "Fox News".to_string(),
        known_bias: Some("right".to_string()),
        ownership_type: Some("corporate".to_string()),
    };

    let json = serde_json::to_string(&meta).unwrap();
    let roundtripped: SourceMeta = serde_json::from_str(&json).unwrap();

    assert_eq!(roundtripped.publication, "Fox News");
    assert_eq!(roundtripped.known_bias, Some("right".to_string()));
    assert_eq!(roundtripped.ownership_type, Some("corporate".to_string()));

    let roundtrip_json = serde_json::to_string(&roundtripped).unwrap();
    assert_eq!(roundtrip_json, json, "SourceMeta roundtrip changed serialization");
}

// =============================================================================
// Full AnalysisResult Serialization Determinism (Extended)
// =============================================================================

/// Helper: construct a complete AnalysisResult with all optional fields populated.
fn fixture_full_analysis_result() -> AnalysisResult {
    AnalysisResult {
        title: "Surveillance Expansion Act 2024".to_string(),
        source_url: Some("https://example.com/article".to_string()),
        personas: vec![
            PersonaOutput {
                id: PersonaId::ProgressiveActivist,
                title: "Progressive Activist".to_string(),
                stance_score: -2.5,
                confidence: 0.9,
                summary: "This represents a dangerous expansion of state power.".to_string(),
                key_claims: vec![
                    "Disproportionate impact on minorities".to_string(),
                    "Chilling effect on free speech".to_string(),
                ],
                fact_checks: vec![FactCheck {
                    claim: "Targets specific communities".to_string(),
                    assessment: FactCheckAssessment::Supported,
                    rationale: "Multiple studies confirm".to_string(),
                }],
                caveats: vec!["May underweight security concerns".to_string()],
                axes: Some(Axes2D {
                    economic: -1.0,
                    social: -2.5,
                }),
            },
            PersonaOutput {
                id: PersonaId::NationalSecurityHawk,
                title: "National Security Hawk".to_string(),
                stance_score: 2.8,
                confidence: 0.75,
                summary: "Necessary measures for national defense.".to_string(),
                key_claims: vec![
                    "Intelligence gaps are real".to_string(),
                    "Encryption threatens security".to_string(),
                ],
                fact_checks: vec![FactCheck {
                    claim: "Recent intelligence failures".to_string(),
                    assessment: FactCheckAssessment::Contested,
                    rationale: "Classified data limits verification".to_string(),
                }],
                caveats: vec!["May overweight threat assessments".to_string()],
                axes: Some(Axes2D {
                    economic: 0.5,
                    social: 2.8,
                }),
            },
        ],
        debiaser: DebiasedSummary {
            consensus_points: vec!["Policy has significant implications".to_string()],
            disagreements: vec!["Liberty vs security weighting".to_string()],
            likely_bias_drivers: vec!["Security-first framing".to_string()],
            truth_seeking_summary: "Balanced concerns exist on both sides.".to_string(),
            spectrum_score: -0.38,
            spectrum_explain: "Weighted analysis tilts toward liberty.".to_string(),
        },
        tone_analysis: Some(ToneAnalysis {
            rhetorical_devices: vec![
                "appeal to fear".to_string(),
                "loaded language".to_string(),
            ],
            emotional_tone: "alarmist".to_string(),
            framing_strategy: "conflict frame".to_string(),
            objectivity_score: 0.35,
        }),
        source_meta: Some(SourceMeta {
            publication: "The Daily News".to_string(),
            known_bias: Some("center-left".to_string()),
            ownership_type: Some("corporate".to_string()),
        }),
        warnings: vec![],
    }
}

/// Verify full AnalysisResult with tone_analysis and source_meta serializes deterministically.
#[test]
fn full_analysis_result_with_all_optionals_serialization_deterministic() {
    let result = fixture_full_analysis_result();
    let first_json = serde_json::to_string(&result).unwrap();

    for i in 1..50 {
        let json = serde_json::to_string(&result).unwrap();
        assert_eq!(
            json, first_json,
            "Full result serialization differs on iteration {i}"
        );
    }
}

/// Verify full AnalysisResult roundtrip preserves exact JSON form.
#[test]
fn full_analysis_result_roundtrip_preserves_serialization() {
    let result = fixture_full_analysis_result();
    let json = serde_json::to_string(&result).unwrap();
    let roundtripped: AnalysisResult = serde_json::from_str(&json).unwrap();
    let roundtrip_json = serde_json::to_string(&roundtripped).unwrap();

    assert_eq!(
        roundtrip_json, json,
        "Full result roundtrip changed serialization"
    );
}

/// Verify AnalysisResult with warnings serializes deterministically.
#[test]
fn analysis_result_with_warnings_serialization_deterministic() {
    let mut result = fixture_full_analysis_result();
    result.warnings = vec![
        "2/8 personas failed".to_string(),
        "Tone analysis unavailable".to_string(),
    ];

    let first_json = serde_json::to_string(&result).unwrap();
    for i in 1..20 {
        let json = serde_json::to_string(&result).unwrap();
        assert_eq!(
            json, first_json,
            "Warnings serialization differs on iteration {i}"
        );
    }

    // Verify warnings are present in output
    let parsed: serde_json::Value = serde_json::from_str(&first_json).unwrap();
    assert_eq!(parsed["warnings"].as_array().unwrap().len(), 2);
}

/// Verify AnalysisResult without optional fields omits them consistently.
#[test]
fn analysis_result_omits_none_optionals_deterministically() {
    let result = AnalysisResult {
        title: "Plain Text".to_string(),
        source_url: None,
        personas: vec![],
        debiaser: DebiasedSummary {
            consensus_points: vec![],
            disagreements: vec![],
            likely_bias_drivers: vec![],
            truth_seeking_summary: "N/A".to_string(),
            spectrum_score: 0.0,
            spectrum_explain: "N/A".to_string(),
        },
        tone_analysis: None,
        source_meta: None,
        warnings: vec![],
    };

    let first_json = serde_json::to_string(&result).unwrap();
    // Verify optionals are omitted (skip_serializing_if)
    assert!(!first_json.contains("tone_analysis"));
    assert!(!first_json.contains("source_meta"));
    assert!(!first_json.contains("warnings"));

    for i in 1..20 {
        let json = serde_json::to_string(&result).unwrap();
        assert_eq!(
            json, first_json,
            "None-optionals serialization differs on iteration {i}"
        );
    }
}

// =============================================================================
// FactCheck Assessment Serialization Determinism
// =============================================================================

/// Verify all 4 FactCheckAssessment variants serialize identically across runs.
#[test]
fn fact_check_assessment_all_variants_deterministic() {
    let variants = [
        FactCheckAssessment::Supported,
        FactCheckAssessment::Contested,
        FactCheckAssessment::Unsupported,
        FactCheckAssessment::Unclear,
    ];

    let first_jsons: Vec<String> = variants
        .iter()
        .map(|v| serde_json::to_string(v).unwrap())
        .collect();

    for i in 1..100 {
        for (idx, variant) in variants.iter().enumerate() {
            let json = serde_json::to_string(variant).unwrap();
            assert_eq!(
                json, first_jsons[idx],
                "FactCheckAssessment {:?} serialization changed on iteration {i}",
                variant
            );
        }
    }

    // Verify expected values
    assert_eq!(first_jsons[0], r#""supported""#);
    assert_eq!(first_jsons[1], r#""contested""#);
    assert_eq!(first_jsons[2], r#""unsupported""#);
    assert_eq!(first_jsons[3], r#""unclear""#);
}

/// Verify FactCheck struct (claim + assessment + rationale) roundtrip is deterministic.
#[test]
fn fact_check_struct_roundtrip_deterministic() {
    let checks = vec![
        FactCheck {
            claim: "Crime rates increased by 15%".to_string(),
            assessment: FactCheckAssessment::Supported,
            rationale: "FBI UCR data confirms".to_string(),
        },
        FactCheck {
            claim: "Policy has bipartisan support".to_string(),
            assessment: FactCheckAssessment::Contested,
            rationale: "Only 2 opposition senators voted yes".to_string(),
        },
        FactCheck {
            claim: "No other country has tried this".to_string(),
            assessment: FactCheckAssessment::Unsupported,
            rationale: "UK and Australia have similar programs".to_string(),
        },
        FactCheck {
            claim: "Costs will be recouped in 5 years".to_string(),
            assessment: FactCheckAssessment::Unclear,
            rationale: "CBO has not released projections".to_string(),
        },
    ];

    let first_json = serde_json::to_string(&checks).unwrap();
    for i in 1..20 {
        let json = serde_json::to_string(&checks).unwrap();
        assert_eq!(json, first_json, "FactCheck array differs on iteration {i}");
    }

    // Roundtrip
    let roundtripped: Vec<FactCheck> = serde_json::from_str(&first_json).unwrap();
    let roundtrip_json = serde_json::to_string(&roundtripped).unwrap();
    assert_eq!(roundtrip_json, first_json, "FactCheck roundtrip changed serialization");
}

// =============================================================================
// PersonaOutput Construction Determinism
// =============================================================================

/// Verify PersonaOutput with axes=None serializes consistently (axes estimation edge case).
#[test]
fn persona_output_without_axes_serialization_deterministic() {
    let output = PersonaOutput {
        id: PersonaId::LibertarianCivil,
        title: "Libertarian, Civil Liberties".to_string(),
        stance_score: -2.6,
        confidence: 0.76,
        summary: "Privacy as fundamental liberty.".to_string(),
        key_claims: vec!["Government overreach".to_string()],
        fact_checks: vec![],
        caveats: vec!["May downplay collective security".to_string()],
        axes: None,
    };

    let first_json = serde_json::to_string(&output).unwrap();
    for i in 1..50 {
        let json = serde_json::to_string(&output).unwrap();
        assert_eq!(
            json, first_json,
            "PersonaOutput without axes differs on iteration {i}"
        );
    }

    let roundtripped: PersonaOutput = serde_json::from_str(&first_json).unwrap();
    assert!(roundtripped.axes.is_none());
    let roundtrip_json = serde_json::to_string(&roundtripped).unwrap();
    assert_eq!(roundtrip_json, first_json);
}

/// Verify PersonaOutput with all 8 persona IDs produces stable serialization.
#[test]
fn all_persona_outputs_serialize_deterministically() {
    let outputs: Vec<PersonaOutput> = PersonaId::all()
        .iter()
        .enumerate()
        .map(|(i, id)| {
            let stance = -3.0 + (i as f64 * 0.75);
            make_persona(id.clone(), stance, 0.5 + (i as f64 * 0.05))
        })
        .collect();

    let first_json = serde_json::to_string(&outputs).unwrap();
    for i in 1..20 {
        let json = serde_json::to_string(&outputs).unwrap();
        assert_eq!(
            json, first_json,
            "All-personas serialization differs on iteration {i}"
        );
    }

    let roundtripped: Vec<PersonaOutput> = serde_json::from_str(&first_json).unwrap();
    assert_eq!(roundtripped.len(), 8);
    let roundtrip_json = serde_json::to_string(&roundtripped).unwrap();
    assert_eq!(roundtrip_json, first_json);
}

// =============================================================================
// DebiasedSummary Serialization Determinism
// =============================================================================

/// Verify DebiasedSummary with typical values roundtrips deterministically.
#[test]
fn debiased_summary_serialization_roundtrip_deterministic() {
    let summary = DebiasedSummary {
        consensus_points: vec![
            "All perspectives agree the policy is significant".to_string(),
            "Oversight mechanisms are needed".to_string(),
        ],
        disagreements: vec![
            "Progressive vs hawk on liberty-order balance".to_string(),
        ],
        likely_bias_drivers: vec![
            "Security-first framing in source article".to_string(),
            "Absence of affected community voices".to_string(),
        ],
        truth_seeking_summary: "The policy addresses real concerns but lacks proportionality safeguards.".to_string(),
        spectrum_score: -0.73,
        spectrum_explain: "Weighted analysis reflects stronger liberty-oriented confidence.".to_string(),
    };

    let first_json = serde_json::to_string(&summary).unwrap();
    for i in 1..50 {
        let json = serde_json::to_string(&summary).unwrap();
        assert_eq!(
            json, first_json,
            "DebiasedSummary serialization differs on iteration {i}"
        );
    }

    let roundtripped: DebiasedSummary = serde_json::from_str(&first_json).unwrap();
    let roundtrip_json = serde_json::to_string(&roundtripped).unwrap();
    assert_eq!(roundtrip_json, first_json, "DebiasedSummary roundtrip changed");
}

/// Verify DebiasedSummary with empty lists serializes deterministically.
#[test]
fn debiased_summary_empty_lists_serialization_deterministic() {
    let summary = DebiasedSummary {
        consensus_points: vec![],
        disagreements: vec![],
        likely_bias_drivers: vec![],
        truth_seeking_summary: "Debiased summary could not be generated.".to_string(),
        spectrum_score: 0.0,
        spectrum_explain: "Fallback: confidence-weighted mean of persona stance scores.".to_string(),
    };

    let first_json = serde_json::to_string(&summary).unwrap();
    for i in 1..20 {
        let json = serde_json::to_string(&summary).unwrap();
        assert_eq!(
            json, first_json,
            "Empty DebiasedSummary differs on iteration {i}"
        );
    }

    let roundtripped: DebiasedSummary = serde_json::from_str(&first_json).unwrap();
    assert!(roundtripped.consensus_points.is_empty());
    assert!(roundtripped.disagreements.is_empty());
    assert!(roundtripped.likely_bias_drivers.is_empty());
}

// =============================================================================
// Multi-Roundtrip Chain Tests
// =============================================================================

/// Verify DebiasedSummary survives 10 serialization roundtrips without drift.
#[test]
fn debiased_summary_10_roundtrip_chain_no_drift() {
    let original = DebiasedSummary {
        consensus_points: vec![
            "Policy has broad implications".to_string(),
            "Oversight is necessary".to_string(),
        ],
        disagreements: vec!["Liberty vs security trade-off".to_string()],
        likely_bias_drivers: vec![
            "Security-first framing".to_string(),
            "Selective source citation".to_string(),
        ],
        truth_seeking_summary: "The policy balances competing interests with uneven results."
            .to_string(),
        spectrum_score: -0.67,
        spectrum_explain: "Weighted analysis tilts toward liberty concerns.".to_string(),
    };

    let first_json = serde_json::to_string(&original).unwrap();
    let mut current_json = first_json.clone();

    for i in 0..10 {
        let deserialized: DebiasedSummary = serde_json::from_str(&current_json).unwrap();
        current_json = serde_json::to_string(&deserialized).unwrap();
        assert_eq!(
            current_json, first_json,
            "DebiasedSummary drifted on roundtrip {i}"
        );
    }
}

/// Verify full AnalysisResult with all optionals survives 5 roundtrip cycles.
#[test]
fn full_analysis_result_5_roundtrip_chain_no_drift() {
    let result = fixture_full_analysis_result();
    let first_json = serde_json::to_string(&result).unwrap();
    let mut current = first_json.clone();

    for i in 0..5 {
        let deserialized: AnalysisResult = serde_json::from_str(&current).unwrap();
        current = serde_json::to_string(&deserialized).unwrap();
        assert_eq!(
            current, first_json,
            "AnalysisResult drifted on roundtrip {i}"
        );
    }
}

/// Verify PersonaOutput survives 10 roundtrip cycles with all fields populated.
#[test]
fn persona_output_10_roundtrip_chain_no_drift() {
    let output = PersonaOutput {
        id: PersonaId::ConservativeFiscal,
        title: "Conservative, Fiscal".to_string(),
        stance_score: 1.75,
        confidence: 0.72,
        summary: "Market-based solutions preferred.".to_string(),
        key_claims: vec![
            "Regulation stifles innovation".to_string(),
            "Free markets self-correct".to_string(),
        ],
        fact_checks: vec![
            FactCheck {
                claim: "Deregulation boosts GDP".to_string(),
                assessment: FactCheckAssessment::Contested,
                rationale: "Mixed evidence from multiple studies.".to_string(),
            },
            FactCheck {
                claim: "Tax cuts pay for themselves".to_string(),
                assessment: FactCheckAssessment::Unsupported,
                rationale: "CBO analyses disagree.".to_string(),
            },
        ],
        caveats: vec!["May underweight externalities".to_string()],
        axes: Some(Axes2D {
            economic: 2.1,
            social: 0.5,
        }),
    };

    let first_json = serde_json::to_string(&output).unwrap();
    let mut current = first_json.clone();

    for i in 0..10 {
        let deserialized: PersonaOutput = serde_json::from_str(&current).unwrap();
        current = serde_json::to_string(&deserialized).unwrap();
        assert_eq!(
            current, first_json,
            "PersonaOutput drifted on roundtrip {i}"
        );
    }
}

// =============================================================================
// ToneAnalysis Field Validation
// =============================================================================

/// Verify ToneAnalysis objectivity_score at bounds (0.0 and 1.0) roundtrips.
#[test]
fn tone_analysis_objectivity_score_boundary_roundtrip() {
    for score in [0.0_f64, 0.5, 1.0] {
        let tone = ToneAnalysis {
            rhetorical_devices: vec![],
            emotional_tone: "neutral".to_string(),
            framing_strategy: "reporting".to_string(),
            objectivity_score: score,
        };
        let json = serde_json::to_string(&tone).unwrap();
        let parsed: ToneAnalysis = serde_json::from_str(&json).unwrap();
        assert!(
            (parsed.objectivity_score - score).abs() < f64::EPSILON,
            "objectivity_score {score} failed roundtrip: got {}",
            parsed.objectivity_score
        );
    }
}

/// Verify ToneAnalysis with many rhetorical devices preserves order and content.
#[test]
fn tone_analysis_many_devices_order_preserved() {
    let devices: Vec<String> = vec![
        "appeal to fear",
        "loaded language",
        "false dichotomy",
        "bandwagon",
        "appeal to authority",
        "straw man",
        "ad hominem",
    ]
    .into_iter()
    .map(String::from)
    .collect();

    let tone = ToneAnalysis {
        rhetorical_devices: devices.clone(),
        emotional_tone: "inflammatory".to_string(),
        framing_strategy: "conflict frame".to_string(),
        objectivity_score: 0.1,
    };

    let json = serde_json::to_string(&tone).unwrap();
    let parsed: ToneAnalysis = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.rhetorical_devices, devices);
}

// =============================================================================
// Edge Cases: Empty, Whitespace, Long Text, Special Characters
// =============================================================================

/// Verify the pipeline returns 400 for empty text input.
#[tokio::test]
#[serial]
async fn edge_case_empty_text_returns_400() {
    let server = MockServer::start().await;
    let app = app_with_ollama(&server.uri());

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/analyze-text")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({"text": ""}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::BAD_REQUEST,
        "Empty text should return 400"
    );

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let error: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(error["error"], "Empty text");
}

/// Verify the pipeline returns 400 for whitespace-only text.
#[tokio::test]
#[serial]
async fn edge_case_whitespace_only_text_returns_400() {
    let server = MockServer::start().await;
    let app = app_with_ollama(&server.uri());

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/analyze-text")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({"text": "   \n\t  "}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::BAD_REQUEST,
        "Whitespace-only text should return 400"
    );
}

/// Verify the pipeline returns 400 for text exceeding 100K characters.
#[tokio::test]
#[serial]
async fn edge_case_text_too_long_returns_400() {
    let server = MockServer::start().await;
    let app = app_with_ollama(&server.uri());

    let long_text = "A".repeat(100_001);

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/analyze-text")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({"text": long_text}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::BAD_REQUEST,
        "Text over 100K should return 400"
    );

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let error: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(error["error"], "Text too long");
}

/// Verify special characters (unicode, HTML, quotes) don't break the pipeline.
#[tokio::test]
#[serial]
async fn edge_case_special_chars_handled() {
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

    let special_text = "The government\u{2019}s \u{201C}new\u{201D} policy \u{2014} worth \u{20AC}1.5B \u{2014} affects <em>all</em> citizens. O\u{2019}Brien said: \"It's a 'paradigm shift' & a game-changer.\" \u{4E2D}\u{6587}\u{6D4B}\u{8BD5} \u{1F512}";

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/analyze-text")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "text": special_text,
                        "title": "Special Chars Test"
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
    assert_eq!(result.personas.len(), 8);
    assert_eq!(result.title, "Special Chars Test");
}

/// Verify PersonaOutput with unicode and HTML in strings roundtrips safely.
#[test]
fn edge_case_persona_output_unicode_html_roundtrip() {
    let output = PersonaOutput {
        id: PersonaId::LiberalSocialDemocrat,
        title: "Liberal Social Democrat".to_string(),
        stance_score: -1.0,
        confidence: 0.8,
        summary: "L\u{2019}\u{00E9}tat doit prot\u{00E9}ger les droits \u{2014} \"libert\u{00E9}, \u{00E9}galit\u{00E9}, fraternit\u{00E9}\".".to_string(),
        key_claims: vec![
            "Workers\u{2019} rights are paramount".to_string(),
            "Cost: \u{20AC}1.5B/year".to_string(),
        ],
        fact_checks: vec![FactCheck {
            claim: "The <script>alert('xss')</script> claim".to_string(),
            assessment: FactCheckAssessment::Contested,
            rationale: "See O\u{2019}Brien et al. & \u{4E2D}\u{6587}\u{6D4B}\u{8BD5}".to_string(),
        }],
        caveats: vec!["May not apply to non-EU jurisdictions".to_string()],
        axes: Some(Axes2D {
            economic: -0.8,
            social: -1.2,
        }),
    };

    let json = serde_json::to_string(&output).unwrap();
    let parsed: PersonaOutput = serde_json::from_str(&json).unwrap();
    assert!(parsed.summary.contains("libert\u{00E9}"));
    assert!(parsed.key_claims[1].contains('\u{20AC}'));
    assert!(parsed.fact_checks[0].claim.contains("<script>"));
    assert!(parsed.fact_checks[0].rationale.contains('\u{4E2D}'));

    let json2 = serde_json::to_string(&parsed).unwrap();
    assert_eq!(json, json2, "Unicode/HTML roundtrip changed serialization");
}

/// Verify DebiasedSummary with HTML-like content roundtrips without mangling.
#[test]
fn edge_case_debiased_summary_html_content_roundtrip() {
    let summary = DebiasedSummary {
        consensus_points: vec![
            "All <perspectives> agree on <b>oversight</b>".to_string(),
            "The \"<script>alert('xss')</script>\" claim is notable".to_string(),
        ],
        disagreements: vec!["Left vs right on <tax policy>".to_string()],
        likely_bias_drivers: vec!["Framing: \"us vs them\" & <fear>".to_string()],
        truth_seeking_summary: "Key finding: A & B > C, but D < E.".to_string(),
        spectrum_score: 0.0,
        spectrum_explain: "Balanced between <left> and <right>.".to_string(),
    };

    let json = serde_json::to_string(&summary).unwrap();
    let parsed: DebiasedSummary = serde_json::from_str(&json).unwrap();
    assert!(parsed.consensus_points[1].contains("<script>"));
    assert!(parsed.truth_seeking_summary.contains("A & B > C"));

    let json2 = serde_json::to_string(&parsed).unwrap();
    assert_eq!(json, json2, "HTML content roundtrip changed serialization");
}

/// Verify AnalysisResult with very long text fields roundtrips correctly.
#[test]
fn edge_case_long_text_fields_roundtrip() {
    let long_summary = "A".repeat(10_000);
    let long_claim = "B".repeat(5_000);

    let result = AnalysisResult {
        title: "C".repeat(1_000),
        source_url: Some("https://example.com/".to_string() + &"d".repeat(500)),
        personas: vec![PersonaOutput {
            id: PersonaId::LibertarianCivil,
            title: "Libertarian, Civil Liberties".to_string(),
            stance_score: -2.8,
            confidence: 0.95,
            summary: long_summary.clone(),
            key_claims: vec![long_claim.clone()],
            fact_checks: vec![FactCheck {
                claim: long_claim.clone(),
                assessment: FactCheckAssessment::Supported,
                rationale: "E".repeat(3_000),
            }],
            caveats: vec!["F".repeat(2_000)],
            axes: Some(Axes2D {
                economic: -2.5,
                social: -2.9,
            }),
        }],
        debiaser: DebiasedSummary {
            consensus_points: vec!["G".repeat(5_000)],
            disagreements: vec![],
            likely_bias_drivers: vec![],
            truth_seeking_summary: long_summary.clone(),
            spectrum_score: -2.8,
            spectrum_explain: "H".repeat(3_000),
        },
        tone_analysis: None,
        source_meta: None,
        warnings: vec![],
    };

    let json = serde_json::to_string(&result).unwrap();
    let parsed: AnalysisResult = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.personas[0].summary.len(), 10_000);
    assert_eq!(parsed.personas[0].key_claims[0].len(), 5_000);
    assert_eq!(parsed.debiaser.consensus_points[0].len(), 5_000);

    let json2 = serde_json::to_string(&parsed).unwrap();
    assert_eq!(json, json2, "Long text roundtrip changed serialization");
}

/// Verify AnalysisResult with warnings roundtrips correctly.
#[test]
fn edge_case_warnings_roundtrip() {
    let result = AnalysisResult {
        title: "Partial Analysis".to_string(),
        source_url: None,
        personas: vec![make_persona(PersonaId::CentristTechnocrat, 0.0, 0.8)],
        debiaser: DebiasedSummary {
            consensus_points: vec![],
            disagreements: vec![],
            likely_bias_drivers: vec![],
            truth_seeking_summary: "Partial.".to_string(),
            spectrum_score: 0.0,
            spectrum_explain: "Limited data.".to_string(),
        },
        tone_analysis: None,
        source_meta: None,
        warnings: vec![
            "3/8 personas failed".to_string(),
            "Tone analysis unavailable".to_string(),
            "Source credibility analysis unavailable".to_string(),
        ],
    };

    let json = serde_json::to_string(&result).unwrap();
    let parsed: AnalysisResult = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.warnings.len(), 3);
    assert_eq!(parsed.warnings[0], "3/8 personas failed");

    let json2 = serde_json::to_string(&parsed).unwrap();
    assert_eq!(json, json2, "Warnings roundtrip changed serialization");
}

/// Verify PersonaOutput with boundary stance/confidence values roundtrips.
#[test]
fn edge_case_boundary_scores_roundtrip() {
    let boundary_values: Vec<(f64, f64)> = vec![
        (-3.0, 0.0),
        (3.0, 1.0),
        (0.0, 0.5),
        (-3.0, 1.0),
        (3.0, 0.0),
    ];

    for (stance, confidence) in &boundary_values {
        let output = make_persona(PersonaId::CentristTechnocrat, *stance, *confidence);
        let json = serde_json::to_string(&output).unwrap();
        let parsed: PersonaOutput = serde_json::from_str(&json).unwrap();
        assert!(
            (parsed.stance_score - stance).abs() < f64::EPSILON,
            "stance_score {stance} failed roundtrip: got {}",
            parsed.stance_score
        );
        assert!(
            (parsed.confidence - confidence).abs() < f64::EPSILON,
            "confidence {confidence} failed roundtrip: got {}",
            parsed.confidence
        );
    }
}
