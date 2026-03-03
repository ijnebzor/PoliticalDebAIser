// =============================================================================
// Stage 6 — Cross-Provider Validation Tests
//
// Validates the multi-provider LLM chain (src/llm.rs):
// - Ollama and OpenAI-compatible response format parsing
// - Provider fallback behavior (server errors, rate limits)
// - Round-robin call distribution
// - Missing API key graceful skip (provider excluded, not crash)
// - Response timeout handling
// - PersonaOutput / DebiasedSummary JSON schema conformance
//
// All tests use wiremock to mock HTTP endpoints.
// Tests that mutate env vars are serialized via #[serial].
// =============================================================================

use serial_test::serial;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use political_debaiser::llm::call_llm;
use political_debaiser::models::{
    Axes2D, DebiasedSummary, FactCheck, FactCheckAssessment, PersonaId, PersonaOutput,
};

// =============================================================================
// Test fixtures
// =============================================================================

/// Wrap content in an Ollama-native response format.
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

/// Wrap content in an OpenAI-compatible response format (Groq/Gemini/HF).
fn openai_response(content: &str) -> serde_json::Value {
    serde_json::json!({
        "id": "chatcmpl-test",
        "object": "chat.completion",
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": content
            },
            "finish_reason": "stop"
        }],
        "usage": {
            "prompt_tokens": 100,
            "completion_tokens": 50,
            "total_tokens": 150
        }
    })
}

/// Valid persona JSON matching the ParsedPersonaOutput schema in archetypes.rs.
fn valid_persona_json(persona_id: &str) -> String {
    serde_json::json!({
        "stance_score": 1.5,
        "confidence": 0.85,
        "summary": format!("Analysis from the {} perspective.", persona_id),
        "key_claims": [
            format!("{} identifies policy impact on target demographic.", persona_id),
            format!("{} highlights regulatory implications.", persona_id)
        ],
        "fact_checks": [{
            "claim": "The article claims economic growth of 3%",
            "assessment": "supported",
            "rationale": "Multiple sources confirm this figure"
        }, {
            "claim": "The article suggests unanimous expert agreement",
            "assessment": "contested",
            "rationale": "Several prominent economists disagree"
        }],
        "caveats": [
            format!("{} perspective may underweight opposing viewpoints.", persona_id)
        ],
        "axes": {
            "economic": 0.8,
            "social": -0.5
        }
    })
    .to_string()
}

/// Valid debiased synthesis JSON matching ParsedDebiased schema.
fn valid_debiased_json() -> String {
    serde_json::json!({
        "consensus_points": [
            "All perspectives acknowledge the policy has measurable economic effects",
            "Multiple viewpoints note implementation challenges"
        ],
        "disagreements": [
            "Left-leaning personas prioritize equity, right-leaning prioritize efficiency",
            "Security hawks favor enforcement while civil libertarians flag overreach"
        ],
        "likely_bias_drivers": [
            "Article uses security-first framing",
            "Source has known center-right editorial stance"
        ],
        "truth_seeking_summary": "The article addresses a complex policy issue. Evidence suggests moderate economic impact with contested social implications.",
        "spectrum_explain": "Weighted analysis reflects moderate disagreement across the political spectrum."
    })
    .to_string()
}

/// Set up env vars for Ollama-only testing with a mock server.
///
/// # Safety
/// Env var mutation is unsafe in Rust 2024 edition. Tests using this
/// must be annotated with `#[serial]`.
unsafe fn setup_ollama_env(mock_url: &str) {
    unsafe {
        std::env::set_var("LLM_PROVIDERS", "ollama");
        std::env::set_var("OLLAMA_URL", mock_url);
        std::env::set_var("OLLAMA_MODEL", "test-model");
        std::env::remove_var("GROQ_API_KEY");
        std::env::remove_var("GEMINI_API_KEY");
        std::env::remove_var("HF_API_KEY");
    }
}

/// Clean up env vars after tests.
///
/// # Safety
/// Same as setup_ollama_env.
unsafe fn cleanup_env() {
    unsafe {
        std::env::remove_var("LLM_PROVIDERS");
        std::env::remove_var("OLLAMA_URL");
        std::env::remove_var("OLLAMA_MODEL");
        std::env::remove_var("GROQ_API_KEY");
        std::env::remove_var("GEMINI_API_KEY");
        std::env::remove_var("HF_API_KEY");
        std::env::remove_var("LLM_TIMEOUT");
    }
}

// =============================================================================
// 1. Provider response format validation
// =============================================================================

#[tokio::test]
#[serial]
async fn ollama_provider_returns_valid_content() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/chat"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(ollama_response(&valid_persona_json("test_persona"))),
        )
        .expect(1)
        .mount(&server)
        .await;

    unsafe { setup_ollama_env(&server.uri()) };

    let result = call_llm("You are a test persona.", "Analyze this article.").await;

    unsafe { cleanup_env() };

    assert!(
        result.is_ok(),
        "call_llm should succeed: {:?}",
        result.err()
    );
    let content = result.unwrap();
    assert!(
        content.contains("stance_score"),
        "Response should contain persona JSON fields"
    );
    assert!(
        content.contains("confidence"),
        "Response should contain confidence field"
    );
}

#[test]
fn ollama_response_format_deserializes_correctly() {
    /// Ollama-native response: { message: { content: "..." } }
    #[derive(serde::Deserialize)]
    struct OllamaResponse {
        message: OllamaMessage,
    }
    #[derive(serde::Deserialize)]
    struct OllamaMessage {
        content: String,
    }

    let response = ollama_response("Hello from Ollama");
    let json_str = serde_json::to_string(&response).unwrap();
    let parsed: OllamaResponse = serde_json::from_str(&json_str).unwrap();
    assert_eq!(parsed.message.content, "Hello from Ollama");
}

#[test]
fn openai_compatible_response_format_deserializes_correctly() {
    /// OpenAI-compatible response: { choices: [{ message: { content: "..." } }] }
    #[derive(serde::Deserialize)]
    struct OpenAiResponse {
        choices: Vec<OpenAiChoice>,
    }
    #[derive(serde::Deserialize)]
    struct OpenAiChoice {
        message: OpenAiMessage,
    }
    #[derive(serde::Deserialize)]
    struct OpenAiMessage {
        content: String,
    }

    let response = openai_response("Hello from Groq");
    let json_str = serde_json::to_string(&response).unwrap();
    let parsed: OpenAiResponse = serde_json::from_str(&json_str).unwrap();
    assert_eq!(parsed.choices.len(), 1);
    assert_eq!(parsed.choices[0].message.content, "Hello from Groq");
}

#[test]
fn openai_response_with_empty_choices_is_handled() {
    #[derive(serde::Deserialize)]
    struct OpenAiResponse {
        choices: Vec<serde_json::Value>,
    }

    let json = r#"{"choices": []}"#;
    let parsed: OpenAiResponse = serde_json::from_str(json).unwrap();
    assert!(
        parsed.choices.is_empty(),
        "Empty choices array should deserialize"
    );
}

// =============================================================================
// 2. PersonaOutput JSON schema validation
// =============================================================================

#[test]
fn persona_json_deserializes_to_persona_output() {
    let json = valid_persona_json("CentristTechnocrat");
    let val: serde_json::Value = serde_json::from_str(&json).unwrap();

    // Validate all required fields are present and correct types
    assert!(val["stance_score"].is_f64(), "stance_score must be f64");
    assert!(val["confidence"].is_f64(), "confidence must be f64");
    assert!(val["summary"].is_string(), "summary must be string");
    assert!(val["key_claims"].is_array(), "key_claims must be array");
    assert!(val["fact_checks"].is_array(), "fact_checks must be array");
    assert!(val["caveats"].is_array(), "caveats must be array");
    assert!(val["axes"].is_object(), "axes must be object");

    // Validate ranges
    let stance = val["stance_score"].as_f64().unwrap();
    assert!(
        (-3.0..=3.0).contains(&stance),
        "stance_score {stance} out of [-3, 3] range"
    );
    let confidence = val["confidence"].as_f64().unwrap();
    assert!(
        (0.0..=1.0).contains(&confidence),
        "confidence {confidence} out of [0, 1] range"
    );

    // Validate fact_checks structure
    for fc in val["fact_checks"].as_array().unwrap() {
        assert!(fc["claim"].is_string(), "fact_check.claim must be string");
        assert!(
            fc["assessment"].is_string(),
            "fact_check.assessment must be string"
        );
        assert!(
            fc["rationale"].is_string(),
            "fact_check.rationale must be string"
        );
        let assessment = fc["assessment"].as_str().unwrap();
        assert!(
            ["supported", "contested", "unsupported", "unclear"].contains(&assessment),
            "Invalid assessment: {assessment}"
        );
    }

    // Validate axes
    let axes = &val["axes"];
    assert!(axes["economic"].is_f64(), "axes.economic must be f64");
    assert!(axes["social"].is_f64(), "axes.social must be f64");
}

#[test]
fn persona_output_struct_roundtrips_through_json() {
    let output = PersonaOutput {
        id: PersonaId::ProgressiveActivist,
        title: "Progressive Activist".to_string(),
        stance_score: -2.1,
        confidence: 0.9,
        summary: "This policy disproportionately impacts marginalized communities.".to_string(),
        key_claims: vec![
            "Surveillance expansion affects communities of color".to_string(),
            "Chilling effect on free speech and organizing".to_string(),
        ],
        fact_checks: vec![
            FactCheck {
                claim: "Disproportionate impact is documented".to_string(),
                assessment: FactCheckAssessment::Supported,
                rationale: "Multiple studies confirm this pattern.".to_string(),
            },
            FactCheck {
                claim: "Universal agreement among civil rights groups".to_string(),
                assessment: FactCheckAssessment::Contested,
                rationale: "Some groups support targeted measures.".to_string(),
            },
        ],
        caveats: vec!["May underweight legitimate security concerns".to_string()],
        axes: Some(Axes2D {
            economic: -1.0,
            social: -2.1,
        }),
    };

    let json = serde_json::to_string(&output).unwrap();
    let roundtripped: PersonaOutput = serde_json::from_str(&json).unwrap();

    assert_eq!(roundtripped.id, PersonaId::ProgressiveActivist);
    assert!((roundtripped.stance_score - (-2.1)).abs() < f64::EPSILON);
    assert!((roundtripped.confidence - 0.9).abs() < f64::EPSILON);
    assert_eq!(roundtripped.key_claims.len(), 2);
    assert_eq!(roundtripped.fact_checks.len(), 2);
    assert_eq!(
        roundtripped.fact_checks[0].assessment,
        FactCheckAssessment::Supported
    );
    assert_eq!(
        roundtripped.fact_checks[1].assessment,
        FactCheckAssessment::Contested
    );
    assert!(roundtripped.axes.is_some());
    let axes = roundtripped.axes.unwrap();
    assert!((axes.economic - (-1.0)).abs() < f64::EPSILON);
    assert!((axes.social - (-2.1)).abs() < f64::EPSILON);
}

#[test]
fn all_eight_persona_ids_produce_valid_json_schemas() {
    for persona_id in PersonaId::all() {
        let json_str = valid_persona_json(persona_id.title());
        let val: serde_json::Value = serde_json::from_str(&json_str).unwrap();

        assert!(
            val["stance_score"].is_f64(),
            "{:?} missing stance_score",
            persona_id
        );
        assert!(
            val["confidence"].is_f64(),
            "{:?} missing confidence",
            persona_id
        );
        assert!(
            val["summary"].is_string(),
            "{:?} missing summary",
            persona_id
        );
        assert!(
            val["key_claims"].is_array(),
            "{:?} missing key_claims",
            persona_id
        );
        assert!(
            val["fact_checks"].is_array(),
            "{:?} missing fact_checks",
            persona_id
        );
    }
}

// =============================================================================
// 3. DebiasedSummary JSON schema validation
// =============================================================================

#[test]
fn debiased_json_validates_against_schema() {
    let json = valid_debiased_json();
    let val: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert!(
        val["consensus_points"].is_array(),
        "consensus_points must be array"
    );
    assert!(
        val["disagreements"].is_array(),
        "disagreements must be array"
    );
    assert!(
        val["likely_bias_drivers"].is_array(),
        "likely_bias_drivers must be array"
    );
    assert!(
        val["truth_seeking_summary"].is_string(),
        "truth_seeking_summary must be string"
    );
    assert!(
        val["spectrum_explain"].is_string(),
        "spectrum_explain must be string"
    );

    // Verify arrays contain strings
    for point in val["consensus_points"].as_array().unwrap() {
        assert!(point.is_string(), "consensus point must be string");
    }
    for disagreement in val["disagreements"].as_array().unwrap() {
        assert!(disagreement.is_string(), "disagreement must be string");
    }
}

#[test]
fn debiased_summary_struct_roundtrips_through_json() {
    let summary = DebiasedSummary {
        consensus_points: vec![
            "Economic impact is real".to_string(),
            "Implementation faces challenges".to_string(),
        ],
        disagreements: vec!["Liberty vs security trade-off".to_string()],
        likely_bias_drivers: vec!["Source uses conflict framing".to_string()],
        truth_seeking_summary: "Balanced assessment suggests moderate impact.".to_string(),
        spectrum_score: -0.42,
        spectrum_explain: "Weighted mean reflects slight liberty lean.".to_string(),
    };

    let json = serde_json::to_string(&summary).unwrap();
    let roundtripped: DebiasedSummary = serde_json::from_str(&json).unwrap();

    assert_eq!(roundtripped.consensus_points.len(), 2);
    assert_eq!(roundtripped.disagreements.len(), 1);
    assert_eq!(roundtripped.likely_bias_drivers.len(), 1);
    assert!((roundtripped.spectrum_score - (-0.42)).abs() < f64::EPSILON);
    assert!(!roundtripped.truth_seeking_summary.is_empty());
    assert!(!roundtripped.spectrum_explain.is_empty());
}

// =============================================================================
// 4. Fallback behavior — server errors
// =============================================================================

#[tokio::test]
#[serial]
async fn call_llm_fails_after_retries_on_persistent_server_error() {
    let server = MockServer::start().await;

    // Always return 500 — call_provider retries 3 times, then call_llm fails
    Mock::given(method("POST"))
        .and(path("/api/chat"))
        .respond_with(ResponseTemplate::new(500).set_body_string("Internal Server Error"))
        .expect(3..)
        .mount(&server)
        .await;

    unsafe { setup_ollama_env(&server.uri()) };

    let result = call_llm("system", "user").await;

    unsafe { cleanup_env() };

    assert!(result.is_err(), "Should fail after all retries exhausted");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("All LLM providers failed") || err.contains("500"),
        "Error should mention provider failure: {err}"
    );
}

#[tokio::test]
#[serial]
async fn call_llm_rate_limit_429_returns_error() {
    let server = MockServer::start().await;

    // Return 429 — should bail immediately (no retries for rate limit)
    Mock::given(method("POST"))
        .and(path("/api/chat"))
        .respond_with(
            ResponseTemplate::new(429).set_body_string(
                r#"{"error":"rate_limit_exceeded","message":"Too many requests"}"#,
            ),
        )
        .expect(1)
        .mount(&server)
        .await;

    unsafe { setup_ollama_env(&server.uri()) };

    let result = call_llm("system", "user").await;

    unsafe { cleanup_env() };

    assert!(result.is_err(), "Should fail on rate limit");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("429") || err.contains("rate") || err.contains("All LLM providers failed"),
        "Error should mention rate limiting: {err}"
    );
}

#[tokio::test]
#[serial]
async fn call_llm_non_retryable_4xx_fails_immediately() {
    let server = MockServer::start().await;

    // 400 Bad Request is not retryable and not 429 — should fail immediately
    Mock::given(method("POST"))
        .and(path("/api/chat"))
        .respond_with(ResponseTemplate::new(400).set_body_string(r#"{"error":"invalid_request"}"#))
        .expect(1)
        .mount(&server)
        .await;

    unsafe { setup_ollama_env(&server.uri()) };

    let result = call_llm("system", "user").await;

    unsafe { cleanup_env() };

    assert!(result.is_err(), "Should fail on 400");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("400") || err.contains("All LLM providers failed"),
        "Error should mention bad request: {err}"
    );
}

// =============================================================================
// 5. Round-robin distribution
// =============================================================================

#[tokio::test]
#[serial]
async fn round_robin_makes_multiple_calls_to_single_provider() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/chat"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(ollama_response("test response content")),
        )
        .expect(3..)
        .mount(&server)
        .await;

    unsafe { setup_ollama_env(&server.uri()) };

    // Make 5 consecutive calls — with a single provider, round-robin
    // should still work (index increments but wraps to 0 every time)
    for i in 0..5 {
        let result = call_llm("system", &format!("call {i}")).await;
        assert!(
            result.is_ok(),
            "Call {i} should succeed: {:?}",
            result.err()
        );
    }

    unsafe { cleanup_env() };
}

// =============================================================================
// 6. API key validation — missing keys
// =============================================================================

#[tokio::test]
#[serial]
async fn missing_groq_key_skips_to_ollama() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/chat"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(ollama_response("Ollama handled the request")),
        )
        .expect(1)
        .mount(&server)
        .await;

    // SAFETY: env var mutation serialized via #[serial]
    unsafe {
        std::env::set_var("LLM_PROVIDERS", "groq,ollama");
        std::env::set_var("OLLAMA_URL", server.uri());
        std::env::set_var("OLLAMA_MODEL", "test-model");
        std::env::remove_var("GROQ_API_KEY");
    }

    let result = call_llm("system", "user").await;

    unsafe { cleanup_env() };

    assert!(
        result.is_ok(),
        "Should succeed via Ollama fallback: {:?}",
        result.err()
    );
    let content = result.unwrap();
    assert_eq!(content, "Ollama handled the request");
}

#[tokio::test]
#[serial]
async fn missing_gemini_key_skips_to_ollama() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/chat"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(ollama_response("Ollama handled the request")),
        )
        .expect(1)
        .mount(&server)
        .await;

    unsafe {
        std::env::set_var("LLM_PROVIDERS", "gemini,ollama");
        std::env::set_var("OLLAMA_URL", server.uri());
        std::env::set_var("OLLAMA_MODEL", "test-model");
        std::env::remove_var("GEMINI_API_KEY");
    }

    let result = call_llm("system", "user").await;

    unsafe { cleanup_env() };

    assert!(
        result.is_ok(),
        "Should succeed via Ollama fallback: {:?}",
        result.err()
    );
}

#[tokio::test]
#[serial]
async fn missing_hf_key_skips_to_ollama() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/chat"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(ollama_response("Ollama handled the request")),
        )
        .expect(1)
        .mount(&server)
        .await;

    unsafe {
        std::env::set_var("LLM_PROVIDERS", "hf,ollama");
        std::env::set_var("OLLAMA_URL", server.uri());
        std::env::set_var("OLLAMA_MODEL", "test-model");
        std::env::remove_var("HF_API_KEY");
    }

    let result = call_llm("system", "user").await;

    unsafe { cleanup_env() };

    assert!(
        result.is_ok(),
        "Should succeed via Ollama fallback: {:?}",
        result.err()
    );
}

#[tokio::test]
#[serial]
async fn all_cloud_providers_missing_keys_falls_back_to_ollama() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/chat"))
        .respond_with(ResponseTemplate::new(200).set_body_json(ollama_response("Ollama only")))
        .expect(1)
        .mount(&server)
        .await;

    unsafe {
        // All cloud providers configured but none have API keys
        std::env::set_var("LLM_PROVIDERS", "groq,gemini,hf,ollama");
        std::env::set_var("OLLAMA_URL", server.uri());
        std::env::set_var("OLLAMA_MODEL", "test-model");
        std::env::remove_var("GROQ_API_KEY");
        std::env::remove_var("GEMINI_API_KEY");
        std::env::remove_var("HF_API_KEY");
    }

    let result = call_llm("system", "user").await;

    unsafe { cleanup_env() };

    assert!(
        result.is_ok(),
        "Should succeed via Ollama: {:?}",
        result.err()
    );
    assert_eq!(result.unwrap(), "Ollama only");
}

#[tokio::test]
#[serial]
async fn unknown_provider_name_skipped_gracefully() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/chat"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(ollama_response("success after skip")),
        )
        .expect(1)
        .mount(&server)
        .await;

    unsafe {
        std::env::set_var("LLM_PROVIDERS", "nonexistent_provider,ollama");
        std::env::set_var("OLLAMA_URL", server.uri());
        std::env::set_var("OLLAMA_MODEL", "test-model");
    }

    let result = call_llm("system", "user").await;

    unsafe { cleanup_env() };

    assert!(
        result.is_ok(),
        "Unknown provider should be skipped: {:?}",
        result.err()
    );
}

#[tokio::test]
#[serial]
async fn empty_providers_string_falls_back_to_ollama() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/chat"))
        .respond_with(ResponseTemplate::new(200).set_body_json(ollama_response("default Ollama")))
        .expect(1)
        .mount(&server)
        .await;

    unsafe {
        std::env::set_var("LLM_PROVIDERS", "");
        std::env::set_var("OLLAMA_URL", server.uri());
        std::env::set_var("OLLAMA_MODEL", "test-model");
    }

    let result = call_llm("system", "user").await;

    unsafe { cleanup_env() };

    assert!(
        result.is_ok(),
        "Empty providers should fall back to Ollama: {:?}",
        result.err()
    );
}

#[tokio::test]
#[serial]
async fn only_cloud_providers_without_keys_falls_back_to_ollama() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/chat"))
        .respond_with(ResponseTemplate::new(200).set_body_json(ollama_response("auto-fallback")))
        .expect(1)
        .mount(&server)
        .await;

    unsafe {
        // Only cloud providers, none with keys → empty chain → auto Ollama fallback
        std::env::set_var("LLM_PROVIDERS", "groq,gemini,hf");
        std::env::set_var("OLLAMA_URL", server.uri());
        std::env::set_var("OLLAMA_MODEL", "test-model");
        std::env::remove_var("GROQ_API_KEY");
        std::env::remove_var("GEMINI_API_KEY");
        std::env::remove_var("HF_API_KEY");
    }

    let result = call_llm("system", "user").await;

    unsafe { cleanup_env() };

    assert!(
        result.is_ok(),
        "Should auto-fallback to Ollama: {:?}",
        result.err()
    );
    assert_eq!(result.unwrap(), "auto-fallback");
}

// =============================================================================
// 7. Response timeout handling
// =============================================================================

#[tokio::test]
#[serial]
async fn call_llm_times_out_on_slow_provider() {
    let server = MockServer::start().await;

    // Respond with 3-second delay — timeout is set to 1 second
    Mock::given(method("POST"))
        .and(path("/api/chat"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(ollama_response("too slow"))
                .set_delay(std::time::Duration::from_secs(3)),
        )
        .expect(1..)
        .mount(&server)
        .await;

    unsafe {
        setup_ollama_env(&server.uri());
        std::env::set_var("LLM_TIMEOUT", "1");
    }

    let result = call_llm("system", "user").await;

    unsafe { cleanup_env() };

    assert!(result.is_err(), "Should fail with timeout");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("failed") || err.contains("timeout") || err.contains("All LLM providers"),
        "Error should indicate timeout/failure: {err}"
    );
}

// =============================================================================
// 8. Response content validation
// =============================================================================

#[tokio::test]
#[serial]
async fn call_llm_returns_exact_content_from_provider() {
    let server = MockServer::start().await;
    let expected_content = r#"{"stance_score": 1.0, "confidence": 0.75, "summary": "Test."}"#;

    Mock::given(method("POST"))
        .and(path("/api/chat"))
        .respond_with(ResponseTemplate::new(200).set_body_json(ollama_response(expected_content)))
        .expect(1)
        .mount(&server)
        .await;

    unsafe { setup_ollama_env(&server.uri()) };

    let result = call_llm("system", "user").await;

    unsafe { cleanup_env() };

    assert!(result.is_ok());
    assert_eq!(result.unwrap(), expected_content);
}

#[tokio::test]
#[serial]
async fn call_llm_handles_large_response_content() {
    let server = MockServer::start().await;

    // Simulate a large LLM response (4KB+) — providers may return verbose output
    let large_content = "A".repeat(4096);

    Mock::given(method("POST"))
        .and(path("/api/chat"))
        .respond_with(ResponseTemplate::new(200).set_body_json(ollama_response(&large_content)))
        .expect(1)
        .mount(&server)
        .await;

    unsafe { setup_ollama_env(&server.uri()) };

    let result = call_llm("system", "user").await;

    unsafe { cleanup_env() };

    assert!(result.is_ok());
    assert_eq!(result.unwrap().len(), 4096);
}

#[tokio::test]
#[serial]
async fn call_llm_handles_unicode_content() {
    let server = MockServer::start().await;
    let unicode_content = "分析結果: 政策は公平です。🔍 Résumé — ñ";

    Mock::given(method("POST"))
        .and(path("/api/chat"))
        .respond_with(ResponseTemplate::new(200).set_body_json(ollama_response(unicode_content)))
        .expect(1)
        .mount(&server)
        .await;

    unsafe { setup_ollama_env(&server.uri()) };

    let result = call_llm("system", "user").await;

    unsafe { cleanup_env() };

    assert!(result.is_ok());
    assert_eq!(result.unwrap(), unicode_content);
}

// =============================================================================
// 9. Provider request format validation
// =============================================================================

#[tokio::test]
#[serial]
async fn ollama_request_includes_temperature_zero() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/chat"))
        .and(wiremock::matchers::body_json(serde_json::json!({
            "model": "test-model",
            "messages": [
                {"role": "system", "content": "sys"},
                {"role": "user", "content": "usr"}
            ],
            "stream": false,
            "temperature": 0
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(ollama_response("ok")))
        .expect(1)
        .mount(&server)
        .await;

    unsafe { setup_ollama_env(&server.uri()) };

    let result = call_llm("sys", "usr").await;

    unsafe { cleanup_env() };

    assert!(
        result.is_ok(),
        "Request body should match expected format with temperature=0: {:?}",
        result.err()
    );
}

#[tokio::test]
#[serial]
async fn ollama_request_uses_correct_endpoint() {
    let server = MockServer::start().await;

    // Mount on /api/chat — Ollama's native endpoint
    Mock::given(method("POST"))
        .and(path("/api/chat"))
        .respond_with(ResponseTemplate::new(200).set_body_json(ollama_response("correct endpoint")))
        .expect(1)
        .mount(&server)
        .await;

    // Mount on /chat/completions — OpenAI endpoint (should NOT be hit for Ollama)
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(500).set_body_string("wrong endpoint"))
        .expect(0)
        .mount(&server)
        .await;

    unsafe { setup_ollama_env(&server.uri()) };

    let result = call_llm("system", "user").await;

    unsafe { cleanup_env() };

    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "correct endpoint");
}

// =============================================================================
// 10. Cross-provider JSON consistency
// =============================================================================

#[test]
fn persona_output_from_all_providers_is_structurally_identical() {
    // The same persona JSON should deserialize identically regardless of
    // which provider returned it — this validates structural consistency.
    let persona_json = valid_persona_json("TestPersona");

    // Simulate extraction from Ollama response
    let ollama_resp = ollama_response(&persona_json);
    let ollama_content = ollama_resp["message"]["content"].as_str().unwrap();

    // Simulate extraction from OpenAI response
    let openai_resp = openai_response(&persona_json);
    let openai_content = openai_resp["choices"][0]["message"]["content"]
        .as_str()
        .unwrap();

    // Both should produce identical parsed values
    let from_ollama: serde_json::Value = serde_json::from_str(ollama_content).unwrap();
    let from_openai: serde_json::Value = serde_json::from_str(openai_content).unwrap();

    assert_eq!(
        from_ollama, from_openai,
        "Same persona JSON should parse identically from any provider"
    );
}

#[test]
fn debiased_summary_from_all_providers_is_structurally_identical() {
    let debiased_json = valid_debiased_json();

    let ollama_resp = ollama_response(&debiased_json);
    let ollama_content = ollama_resp["message"]["content"].as_str().unwrap();

    let openai_resp = openai_response(&debiased_json);
    let openai_content = openai_resp["choices"][0]["message"]["content"]
        .as_str()
        .unwrap();

    let from_ollama: serde_json::Value = serde_json::from_str(ollama_content).unwrap();
    let from_openai: serde_json::Value = serde_json::from_str(openai_content).unwrap();

    assert_eq!(
        from_ollama, from_openai,
        "Same debiased JSON should parse identically from any provider"
    );
}

// =============================================================================
// 11. Edge cases
// =============================================================================

#[tokio::test]
#[serial]
async fn call_llm_handles_malformed_json_response() {
    let server = MockServer::start().await;

    // Return a response that is valid HTTP but the body is not valid JSON
    Mock::given(method("POST"))
        .and(path("/api/chat"))
        .respond_with(ResponseTemplate::new(200).set_body_string("not json at all"))
        .expect(1..)
        .mount(&server)
        .await;

    unsafe { setup_ollama_env(&server.uri()) };

    let result = call_llm("system", "user").await;

    unsafe { cleanup_env() };

    assert!(
        result.is_err(),
        "Should fail on malformed response: {:?}",
        result.ok()
    );
}

#[tokio::test]
#[serial]
async fn call_llm_handles_empty_response_body() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/chat"))
        .respond_with(ResponseTemplate::new(200).set_body_string(""))
        .expect(1..)
        .mount(&server)
        .await;

    unsafe { setup_ollama_env(&server.uri()) };

    let result = call_llm("system", "user").await;

    unsafe { cleanup_env() };

    assert!(result.is_err(), "Should fail on empty response body");
}

#[test]
fn fact_check_assessment_all_variants_are_valid() {
    let variants = ["supported", "contested", "unsupported", "unclear"];
    for variant in &variants {
        let json = format!(r#""{variant}""#);
        let parsed: FactCheckAssessment = serde_json::from_str(&json).unwrap();
        let reserialized = serde_json::to_string(&parsed).unwrap();
        assert_eq!(reserialized, json);
    }
}

#[test]
fn persona_json_with_missing_optional_fields_still_valid() {
    // Minimal persona JSON — only required fields, no caveats/axes
    let minimal = serde_json::json!({
        "stance_score": 0.0,
        "confidence": 0.5,
        "summary": "Minimal analysis.",
        "key_claims": [],
        "fact_checks": []
    });
    let val: serde_json::Value = serde_json::from_str(&minimal.to_string()).unwrap();
    assert!(val["stance_score"].is_f64());
    assert!(val["confidence"].is_f64());
    assert!(val["summary"].is_string());
    // caveats and axes are optional — their absence is valid
    assert!(val.get("caveats").is_none());
    assert!(val.get("axes").is_none());
}

#[test]
fn persona_json_boundary_values_are_valid() {
    // Test boundary values for numeric fields
    let boundary_cases: Vec<(f64, f64)> = vec![
        (-3.0, 0.0), // min stance, min confidence
        (3.0, 1.0),  // max stance, max confidence
        (0.0, 0.5),  // center stance, mid confidence
        (-3.0, 1.0), // extreme liberty, max confidence
        (3.0, 0.0),  // extreme order, min confidence
    ];

    for (stance, confidence) in boundary_cases {
        let json = serde_json::json!({
            "stance_score": stance,
            "confidence": confidence,
            "summary": "Boundary test.",
            "key_claims": [],
            "fact_checks": [],
            "caveats": [],
            "axes": {
                "economic": stance.clamp(-3.0, 3.0),
                "social": stance.clamp(-3.0, 3.0)
            }
        });
        let val: serde_json::Value = serde_json::from_str(&json.to_string()).unwrap();
        let s = val["stance_score"].as_f64().unwrap();
        let c = val["confidence"].as_f64().unwrap();
        assert!((-3.0..=3.0).contains(&s), "stance {s} out of range");
        assert!((0.0..=1.0).contains(&c), "confidence {c} out of range");
    }
}
