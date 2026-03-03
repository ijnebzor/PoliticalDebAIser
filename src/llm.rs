use std::collections::HashMap;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::{Context, Result};
use serde::Deserialize;
use tokio::sync::{RwLock, Semaphore};

// =============================================================================
// Provider types
// =============================================================================

/// Supported LLM provider backends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LlmProviderType {
    Ollama,
    Groq,
    Gemini,
    HuggingFace,
}

/// Configuration for a single LLM provider.
#[derive(Debug, Clone)]
struct ProviderConfig {
    provider_type: LlmProviderType,
    base_url: String,
    model: String,
    api_key: Option<String>,
}

// =============================================================================
// Response types
// =============================================================================

/// Ollama-native response format: { message: { content: "..." } }
#[derive(Deserialize)]
struct OllamaResponse {
    message: OllamaMessage,
}

#[derive(Deserialize)]
struct OllamaMessage {
    content: String,
}

/// OpenAI-compatible response format: { choices: [{ message: { content: "..." } }] }
#[derive(Deserialize)]
struct OpenAiResponse {
    choices: Vec<OpenAiChoice>,
}

#[derive(Deserialize)]
struct OpenAiChoice {
    message: OpenAiMessage,
}

#[derive(Deserialize)]
struct OpenAiMessage {
    content: String,
}

// =============================================================================
// Runtime configuration (API keys set via POST /config)
// =============================================================================

/// Global runtime config for API keys set by the user at runtime (not persisted).
fn runtime_config() -> &'static RwLock<HashMap<String, String>> {
    static CONFIG: OnceLock<RwLock<HashMap<String, String>>> = OnceLock::new();
    CONFIG.get_or_init(|| RwLock::new(HashMap::new()))
}

/// Update runtime API keys. Empty values remove the key.
pub async fn set_runtime_keys(keys: HashMap<String, String>) {
    let mut config = runtime_config().write().await;
    for (key, value) in keys {
        if value.is_empty() {
            config.remove(&key);
        } else {
            config.insert(key, value);
        }
    }
}

/// Get current runtime API key names (values masked for security).
pub async fn get_runtime_key_names() -> Vec<String> {
    let config = runtime_config().read().await;
    config.keys().cloned().collect()
}

/// Read a config value: runtime config takes precedence over env vars.
/// Uses try_read() to avoid panicking when called from within an async runtime.
fn resolve_config(key: &str) -> Option<String> {
    // Check runtime config first (non-blocking try_read to avoid runtime panic)
    if let Ok(config) = runtime_config().try_read()
        && let Some(val) = config.get(key)
    {
        return Some(val.clone());
    }
    std::env::var(key).ok()
}

// =============================================================================
// Provider chain configuration
// =============================================================================

/// Global concurrency limiter for LLM requests.
/// Configured via LLM_CONCURRENCY env var (default: 4).
fn llm_semaphore() -> &'static Semaphore {
    static SEMAPHORE: OnceLock<Semaphore> = OnceLock::new();
    SEMAPHORE.get_or_init(|| {
        let concurrency: usize = std::env::var("LLM_CONCURRENCY")
            .ok()
            .and_then(|v| v.parse().ok())
            .or_else(|| {
                std::env::var("OLLAMA_CONCURRENCY")
                    .ok()
                    .and_then(|v| v.parse().ok())
            })
            .unwrap_or(4);
        tracing::info!("LLM concurrency limit: {concurrency}");
        Semaphore::new(concurrency)
    })
}

/// Round-robin index for spreading load across providers.
static ROUND_ROBIN_INDEX: AtomicUsize = AtomicUsize::new(0);

/// Read OLLAMA_URL from env with scheme validation (http/https only).
/// Falls back to the default if the env var is unset or has an invalid scheme.
fn validated_ollama_url() -> String {
    let default_url = "http://localhost:11434".to_string();
    match std::env::var("OLLAMA_URL") {
        Ok(url) => {
            if url.starts_with("http://") || url.starts_with("https://") {
                url
            } else {
                tracing::warn!(
                    "OLLAMA_URL has unsupported scheme (must be http:// or https://), using default"
                );
                default_url
            }
        }
        Err(_) => default_url,
    }
}

/// Parse the LLM_PROVIDERS env var and build the provider chain.
/// Defaults to `["ollama"]` if not set.
fn provider_chain() -> Vec<ProviderConfig> {
    let provider_names: Vec<String> = std::env::var("LLM_PROVIDERS")
        .ok()
        .map(|v| {
            v.split(',')
                .map(|s| s.trim().to_lowercase())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_else(|| vec!["ollama".to_string()]);

    let mut providers = Vec::new();

    for name in &provider_names {
        match name.as_str() {
            "ollama" => {
                providers.push(ProviderConfig {
                    provider_type: LlmProviderType::Ollama,
                    base_url: validated_ollama_url(),
                    model: std::env::var("OLLAMA_MODEL").unwrap_or_else(|_| "llama3.2".to_string()),
                    api_key: None,
                });
            }
            "groq" => {
                if let Some(key) = resolve_config("GROQ_API_KEY") {
                    providers.push(ProviderConfig {
                        provider_type: LlmProviderType::Groq,
                        base_url: "https://api.groq.com/openai/v1".to_string(),
                        model: std::env::var("GROQ_MODEL")
                            .unwrap_or_else(|_| "llama-3.1-8b-instant".to_string()),
                        api_key: Some(key),
                    });
                } else {
                    tracing::warn!("Groq provider configured but GROQ_API_KEY not set, skipping");
                }
            }
            "gemini" => {
                if let Some(key) = resolve_config("GEMINI_API_KEY") {
                    providers.push(ProviderConfig {
                        provider_type: LlmProviderType::Gemini,
                        base_url: "https://generativelanguage.googleapis.com/v1beta/openai"
                            .to_string(),
                        model: std::env::var("GEMINI_MODEL")
                            .unwrap_or_else(|_| "gemini-2.5-flash-lite".to_string()),
                        api_key: Some(key),
                    });
                } else {
                    tracing::warn!(
                        "Gemini provider configured but GEMINI_API_KEY not set, skipping"
                    );
                }
            }
            "huggingface" | "hf" => {
                if let Some(key) = resolve_config("HF_API_KEY") {
                    providers.push(ProviderConfig {
                        provider_type: LlmProviderType::HuggingFace,
                        base_url: "https://router.huggingface.co/v1".to_string(),
                        model: std::env::var("HF_MODEL")
                            .unwrap_or_else(|_| "meta-llama/Llama-3.1-8B-Instruct".to_string()),
                        api_key: Some(key),
                    });
                } else {
                    tracing::warn!(
                        "HuggingFace provider configured but HF_API_KEY not set, skipping"
                    );
                }
            }
            other => {
                tracing::warn!("Unknown LLM provider '{other}', skipping");
            }
        }
    }

    // Fallback: if no providers were successfully configured, default to Ollama
    if providers.is_empty() {
        tracing::warn!("No LLM providers configured, falling back to Ollama");
        providers.push(ProviderConfig {
            provider_type: LlmProviderType::Ollama,
            base_url: validated_ollama_url(),
            model: std::env::var("OLLAMA_MODEL").unwrap_or_else(|_| "llama3.2".to_string()),
            api_key: None,
        });
    }

    providers
}

// =============================================================================
// Core LLM call
// =============================================================================

/// Scrub potential API keys/secrets from error response bodies before logging.
/// Scrubs secret patterns first (to avoid partial keys at truncation boundary),
/// then truncates to 200 chars.
fn scrub_error_body(body: &str) -> String {
    use std::sync::LazyLock;
    static SECRET_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(
            r"(?i)(Bearer\s+)\S+|(api[_-]?key\s*[=:]\s*)\S+|(key\s*[=:]\s*)\S+|(token\s*[=:]\s*)\S+|(authorization\s*[=:]\s*)(?:Bearer\s+)?\S+"
        ).unwrap()
    });

    let scrubbed = SECRET_RE
        .replace_all(body, "${1}${2}${3}${4}${5}[REDACTED]")
        .to_string();
    if scrubbed.len() > 200 {
        let truncated: String = scrubbed.chars().take(200).collect();
        format!("{truncated}...")
    } else {
        scrubbed
    }
}

/// Returns true if the HTTP status is retryable (server error).
fn is_retryable(status: reqwest::StatusCode) -> bool {
    status.is_server_error()
}

/// Returns true if the HTTP status indicates rate limiting (429).
fn is_rate_limited(status: reqwest::StatusCode) -> bool {
    status == reqwest::StatusCode::TOO_MANY_REQUESTS
}

/// Call a single provider with retries (up to 3 attempts with 500ms delay).
async fn call_provider(
    provider: &ProviderConfig,
    system_prompt: &str,
    user_message: &str,
) -> Result<String> {
    let timeout_secs: u64 = std::env::var("LLM_TIMEOUT")
        .ok()
        .and_then(|v| v.parse().ok())
        .or_else(|| {
            std::env::var("OLLAMA_TIMEOUT")
                .ok()
                .and_then(|v| v.parse().ok())
        })
        .unwrap_or(120);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .build()
        .context("Failed to build HTTP client for LLM")?;

    let body = serde_json::json!({
        "model": provider.model,
        "messages": [
            { "role": "system", "content": system_prompt },
            { "role": "user", "content": user_message }
        ],
        "stream": false,
        "temperature": 0
    });

    let url = match provider.provider_type {
        LlmProviderType::Ollama => format!("{}/api/chat", provider.base_url),
        _ => format!("{}/chat/completions", provider.base_url),
    };

    let mut last_err = None;

    for attempt in 0..3 {
        if attempt > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }

        let mut request = client.post(&url).json(&body);
        if let Some(ref key) = provider.api_key {
            request = request.bearer_auth(key);
        }

        let response = match request.send().await {
            Ok(resp) => resp,
            Err(e) => {
                tracing::warn!(
                    "{:?} request failed (attempt {}): {e}",
                    provider.provider_type,
                    attempt + 1
                );
                last_err = Some(anyhow::anyhow!(
                    "{:?} request failed: {e}",
                    provider.provider_type
                ));
                continue;
            }
        };

        let status = response.status();

        // Rate-limited: fail immediately to trigger fallback to next provider
        if is_rate_limited(status) {
            let error_body = scrub_error_body(&response.text().await.unwrap_or_default());
            anyhow::bail!(
                "{:?} rate-limited (429): {error_body}",
                provider.provider_type
            );
        }

        if !status.is_success() {
            let error_body = scrub_error_body(&response.text().await.unwrap_or_default());
            if is_retryable(status) && attempt < 2 {
                tracing::warn!(
                    "{:?} returned {status} (attempt {}), retrying",
                    provider.provider_type,
                    attempt + 1
                );
                last_err = Some(anyhow::anyhow!(
                    "{:?} returned {status}: {error_body}",
                    provider.provider_type
                ));
                continue;
            }
            anyhow::bail!(
                "{:?} returned {status}: {error_body}",
                provider.provider_type
            );
        }

        // Parse response based on provider type
        let content = match provider.provider_type {
            LlmProviderType::Ollama => {
                let parsed: OllamaResponse = response
                    .json()
                    .await
                    .context("Failed to parse Ollama response")?;
                parsed.message.content
            }
            _ => {
                let parsed: OpenAiResponse = response.json().await.with_context(|| {
                    format!("Failed to parse {:?} response", provider.provider_type)
                })?;
                parsed
                    .choices
                    .into_iter()
                    .next()
                    .map(|c| c.message.content)
                    .context("Empty choices array in LLM response")?
            }
        };

        return Ok(content);
    }

    Err(last_err.unwrap_or_else(|| {
        anyhow::anyhow!("{:?} request failed after retries", provider.provider_type)
    }))
}

/// Call the LLM with automatic provider fallback.
///
/// Reads the provider chain from `LLM_PROVIDERS` env var (comma-separated).
/// Tries providers in round-robin order; if one fails or is rate-limited,
/// falls through to the next provider in the chain.
///
/// Defaults to Ollama for backward compatibility.
pub async fn call_llm(system_prompt: &str, user_message: &str) -> Result<String> {
    let _permit = llm_semaphore()
        .acquire()
        .await
        .context("Failed to acquire LLM concurrency permit")?;

    let providers = provider_chain();
    let count = providers.len();

    // Round-robin: start from the next index in rotation
    let start = ROUND_ROBIN_INDEX.fetch_add(1, Ordering::Relaxed) % count;

    let mut errors = Vec::new();

    for i in 0..count {
        let idx = (start + i) % count;
        let provider = &providers[idx];

        if i > 0 {
            tracing::info!(
                "Falling back to {:?} (provider {}/{})",
                provider.provider_type,
                i + 1,
                count
            );
        }

        match call_provider(provider, system_prompt, user_message).await {
            Ok(content) => return Ok(content),
            Err(e) => {
                tracing::warn!("{:?} failed: {e:#}", provider.provider_type);
                errors.push(format!("{:?}: {e}", provider.provider_type));
            }
        }
    }

    anyhow::bail!("All LLM providers failed:\n{}", errors.join("\n"))
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_chain_defaults_to_ollama() {
        // When LLM_PROVIDERS is not set, should default to Ollama
        // (This test works because the env var is not typically set in test env)
        let providers = provider_chain();
        assert!(!providers.is_empty());
        // First provider should be Ollama by default
        assert_eq!(providers[0].provider_type, LlmProviderType::Ollama);
    }

    #[test]
    fn ollama_response_deserializes() {
        let json = r#"{"message": {"content": "Hello world"}}"#;
        let parsed: OllamaResponse = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.message.content, "Hello world");
    }

    #[test]
    fn openai_response_deserializes() {
        let json = r#"{
            "choices": [{
                "message": {"content": "Hello from OpenAI-compatible API"},
                "finish_reason": "stop",
                "index": 0
            }]
        }"#;
        let parsed: OpenAiResponse = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.choices.len(), 1);
        assert_eq!(
            parsed.choices[0].message.content,
            "Hello from OpenAI-compatible API"
        );
    }

    #[test]
    fn openai_response_with_multiple_choices() {
        let json = r#"{
            "choices": [
                {"message": {"content": "First"}, "finish_reason": "stop", "index": 0},
                {"message": {"content": "Second"}, "finish_reason": "stop", "index": 1}
            ]
        }"#;
        let parsed: OpenAiResponse = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.choices.len(), 2);
        assert_eq!(parsed.choices[0].message.content, "First");
    }

    #[test]
    fn is_rate_limited_detects_429() {
        assert!(is_rate_limited(reqwest::StatusCode::TOO_MANY_REQUESTS));
        assert!(!is_rate_limited(reqwest::StatusCode::OK));
        assert!(!is_rate_limited(reqwest::StatusCode::INTERNAL_SERVER_ERROR));
    }

    #[test]
    fn is_retryable_detects_5xx() {
        assert!(is_retryable(reqwest::StatusCode::INTERNAL_SERVER_ERROR));
        assert!(is_retryable(reqwest::StatusCode::BAD_GATEWAY));
        assert!(is_retryable(reqwest::StatusCode::SERVICE_UNAVAILABLE));
        assert!(!is_retryable(reqwest::StatusCode::OK));
        assert!(!is_retryable(reqwest::StatusCode::BAD_REQUEST));
        assert!(!is_retryable(reqwest::StatusCode::TOO_MANY_REQUESTS));
    }

    #[test]
    fn llm_provider_type_debug_format() {
        assert_eq!(format!("{:?}", LlmProviderType::Ollama), "Ollama");
        assert_eq!(format!("{:?}", LlmProviderType::Groq), "Groq");
        assert_eq!(format!("{:?}", LlmProviderType::Gemini), "Gemini");
        assert_eq!(format!("{:?}", LlmProviderType::HuggingFace), "HuggingFace");
    }

    #[test]
    fn scrub_error_body_redacts_bearer_token() {
        let body = r#"{"error": "Invalid auth", "header": "Bearer gsk_abc123secret456"}"#;
        let scrubbed = scrub_error_body(body);
        assert!(!scrubbed.contains("gsk_abc123secret456"));
        assert!(scrubbed.contains("[REDACTED]"));
    }

    #[test]
    fn scrub_error_body_redacts_api_key_param() {
        let body = "error: unauthorized, api_key=sk-proj-verysecretkey123 invalid";
        let scrubbed = scrub_error_body(body);
        assert!(!scrubbed.contains("sk-proj-verysecretkey123"));
        assert!(scrubbed.contains("[REDACTED]"));
    }

    #[test]
    fn scrub_error_body_redacts_key_equals() {
        let body = "Request failed: key=AIzaSySecretGeminiKey, status=401";
        let scrubbed = scrub_error_body(body);
        assert!(!scrubbed.contains("AIzaSySecretGeminiKey"));
        assert!(scrubbed.contains("[REDACTED]"));
    }

    #[test]
    fn scrub_error_body_redacts_authorization_header() {
        let body = "authorization: Bearer hf_ABCsecrettoken123 was rejected";
        let scrubbed = scrub_error_body(body);
        assert!(!scrubbed.contains("hf_ABCsecrettoken123"));
        assert!(scrubbed.contains("[REDACTED]"));
    }

    #[test]
    fn scrub_error_body_truncates_long_body() {
        let body = "a]".repeat(150); // 300 chars
        let scrubbed = scrub_error_body(&body);
        assert!(scrubbed.len() <= 204); // 200 chars + "..."
        assert!(scrubbed.ends_with("..."));
    }

    #[test]
    fn scrub_error_body_preserves_safe_content() {
        let body = "rate limit exceeded, retry after 30s";
        let scrubbed = scrub_error_body(body);
        assert_eq!(scrubbed, body);
    }

    #[test]
    fn scrub_error_body_case_insensitive() {
        let body = "BEARER my_secret_token was invalid";
        let scrubbed = scrub_error_body(body);
        assert!(!scrubbed.contains("my_secret_token"));
        assert!(scrubbed.contains("[REDACTED]"));
    }
}
