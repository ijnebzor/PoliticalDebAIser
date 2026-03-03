use std::num::NonZeroUsize;
use std::sync::Arc;

use lru::LruCache;
use rand::Rng;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

// =============================================================================
// Core persona types (v3 — 8 political personas)
// =============================================================================

/// The 8 political persona identifiers used for analysis.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PersonaId {
    ProgressiveActivist,
    LiberalSocialDemocrat,
    CentristTechnocrat,
    LibertarianCivil,
    ConservativeFiscal,
    NationalSecurityHawk,
    EnvironmentalistGreen,
    PopulistAntiElite,
}

impl PersonaId {
    /// Returns all persona variants.
    pub fn all() -> &'static [PersonaId] {
        &[
            PersonaId::ProgressiveActivist,
            PersonaId::LiberalSocialDemocrat,
            PersonaId::CentristTechnocrat,
            PersonaId::LibertarianCivil,
            PersonaId::ConservativeFiscal,
            PersonaId::NationalSecurityHawk,
            PersonaId::EnvironmentalistGreen,
            PersonaId::PopulistAntiElite,
        ]
    }

    /// Human-readable display title for this persona.
    pub fn title(&self) -> &'static str {
        match self {
            PersonaId::ProgressiveActivist => "Progressive Activist",
            PersonaId::LiberalSocialDemocrat => "Liberal Social Democrat",
            PersonaId::CentristTechnocrat => "Centrist Technocrat",
            PersonaId::LibertarianCivil => "Libertarian, Civil Liberties",
            PersonaId::ConservativeFiscal => "Conservative, Fiscal",
            PersonaId::NationalSecurityHawk => "National Security Hawk",
            PersonaId::EnvironmentalistGreen => "Environmentalist Green",
            PersonaId::PopulistAntiElite => "Populist, Anti-elite",
        }
    }
}

/// Fact-check assessment levels.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum FactCheckAssessment {
    Supported,
    Contested,
    Unsupported,
    Unclear,
}

/// A single fact-check entry within a persona's analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FactCheck {
    pub claim: String,
    pub assessment: FactCheckAssessment,
    pub rationale: String,
}

/// Optional 2D political axes for economic vs social placement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Axes2D {
    /// Economic axis: -3 (more intervention) to +3 (more market).
    pub economic: f64,
    /// Social axis: -3 (more libertarian) to +3 (more authoritarian).
    pub social: f64,
}

/// Analysis result from a single persona perspective.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonaOutput {
    pub id: PersonaId,
    pub title: String,
    /// Liberty-Order axis: -3 (liberty) to +3 (order).
    pub stance_score: f64,
    /// Confidence level: 0.0 to 1.0.
    pub confidence: f64,
    /// 2-4 sentence summary from this persona's viewpoint.
    pub summary: String,
    pub key_claims: Vec<String>,
    pub fact_checks: Vec<FactCheck>,
    pub caveats: Vec<String>,
    /// Optional 2D axes for economic vs social placement.
    pub axes: Option<Axes2D>,
}

/// The debiased synthesis produced by combining all persona analyses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebiasedSummary {
    pub consensus_points: Vec<String>,
    pub disagreements: Vec<String>,
    pub likely_bias_drivers: Vec<String>,
    pub truth_seeking_summary: String,
    /// Weighted spectrum score on Liberty-Order axis: -3 to +3.
    pub spectrum_score: f64,
    pub spectrum_explain: String,
}

/// Tone and framing analysis of the article's writing style.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToneAnalysis {
    /// Rhetorical devices detected (e.g., "appeal to fear", "loaded language").
    pub rhetorical_devices: Vec<String>,
    /// Overall emotional tone (e.g., "alarmist", "measured", "inflammatory").
    pub emotional_tone: String,
    /// Framing strategy used by the author (e.g., "conflict frame", "human interest").
    pub framing_strategy: String,
    /// Objectivity score: 0.0 (highly subjective) to 1.0 (highly objective).
    pub objectivity_score: f64,
}

/// Metadata about the article's source/publication.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceMeta {
    /// Publication or outlet name (e.g., "The Guardian", "Fox News").
    pub publication: String,
    /// Known editorial bias direction, if any (e.g., "left-leaning", "right-leaning", "centrist").
    pub known_bias: Option<String>,
    /// Ownership structure type (e.g., "corporate", "non-profit", "state-owned").
    pub ownership_type: Option<String>,
}

/// Full analysis result returned to the client (v3).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisResult {
    pub title: String,
    pub source_url: Option<String>,
    pub personas: Vec<PersonaOutput>,
    pub debiaser: DebiasedSummary,
    /// Tone and framing analysis of the article's writing style.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tone_analysis: Option<ToneAnalysis>,
    /// Metadata about the article's source/publication.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_meta: Option<SourceMeta>,
    /// Warnings from partial failures (e.g., "2/8 personas failed").
    /// Empty in the happy path; present when some personas failed but
    /// enough succeeded to produce a useful result.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

// =============================================================================
// Scraped content
// =============================================================================

/// Scraped article content extracted from a URL.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArticleContent {
    pub title: String,
    pub body_text: String,
    pub meta_description: Option<String>,
    pub source_url: String,
    /// True if the article was retrieved via archive.ph due to a paywall.
    #[serde(default)]
    pub paywalled: bool,
    /// Disclaimer text when content was accessed via archive.ph.
    #[serde(default)]
    pub disclaimer: Option<String>,
}

// =============================================================================
// API request/response types
// =============================================================================

/// Incoming request to analyze an article URL.
#[derive(Debug, Deserialize)]
pub struct AnalysisRequest {
    pub url: String,
}

/// Request to analyze raw article text (no URL scraping).
#[derive(Debug, Deserialize)]
pub struct TextAnalysisRequest {
    pub text: String,
    pub title: Option<String>,
}

/// Structured JSON error response.
#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: String,
    pub details: Option<String>,
}

/// Request to update runtime LLM provider API keys via POST /config.
#[derive(Debug, Deserialize)]
pub struct ConfigRequest {
    #[serde(default)]
    pub groq_api_key: Option<String>,
    #[serde(default)]
    pub gemini_api_key: Option<String>,
    #[serde(default)]
    pub hf_api_key: Option<String>,
}

/// Response from GET /config showing which keys are configured.
#[derive(Debug, Serialize)]
pub struct ConfigResponse {
    pub groq_configured: bool,
    pub gemini_configured: bool,
    pub hf_configured: bool,
}

// =============================================================================
// Shared state (bounded LRU caches)
// =============================================================================

/// Default max entries for article cache.
pub const DEFAULT_CACHE_SIZE: usize = 100;
/// Default max entries for analysis history store.
pub const DEFAULT_STORE_SIZE: usize = 500;

/// Shared article cache: URL -> ArticleContent (LRU-bounded).
pub type ArticleCache = Arc<RwLock<LruCache<String, ArticleContent>>>;

/// Shared analysis history store: short ID -> StoredAnalysis (LRU-bounded).
pub type AnalysisStore = Arc<RwLock<LruCache<String, StoredAnalysis>>>;

/// Combined application state shared across handlers.
#[derive(Clone)]
pub struct AppState {
    pub cache: ArticleCache,
    pub store: AnalysisStore,
}

impl AppState {
    /// Create a new AppState with configurable cache sizes.
    pub fn new(cache_size: usize, store_size: usize) -> Self {
        Self {
            cache: Arc::new(RwLock::new(LruCache::new(
                NonZeroUsize::new(cache_size)
                    .unwrap_or(NonZeroUsize::new(DEFAULT_CACHE_SIZE).unwrap()),
            ))),
            store: Arc::new(RwLock::new(LruCache::new(
                NonZeroUsize::new(store_size)
                    .unwrap_or(NonZeroUsize::new(DEFAULT_STORE_SIZE).unwrap()),
            ))),
        }
    }
}

// =============================================================================
// History / storage types
// =============================================================================

/// A stored analysis result, keyed by a short hash ID for URL sharing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredAnalysis {
    pub id: String,
    pub created_at: String,
    pub source_url: String,
    pub response: AnalysisResult,
}

/// Summary item for the history listing endpoint (GET /history).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryListItem {
    pub id: String,
    pub article_title: String,
    pub source_url: String,
    pub created_at: String,
}

/// Request body for POST /history — stores a completed analysis.
#[derive(Debug, Deserialize)]
pub struct StoreHistoryRequest {
    pub source_url: String,
    pub result: AnalysisResult,
}

/// Response from POST /history.
#[derive(Debug, Serialize)]
pub struct StoreHistoryResponse {
    pub id: String,
    pub share_url: String,
}

/// Generate an 8-character URL-safe short ID using cryptographic randomness.
/// The `_url` and `_timestamp` parameters are retained for API compatibility
/// but are no longer used — IDs are now fully random and unpredictable.
pub fn generate_short_id(_url: &str, _timestamp: &str) -> String {
    let mut rng = rand::thread_rng();
    let bytes: u64 = rng.r#gen();
    format!("{bytes:016x}")[..8].to_string()
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persona_id_all_returns_eight() {
        assert_eq!(PersonaId::all().len(), 8);
    }

    #[test]
    fn persona_id_all_variants_are_unique() {
        let all = PersonaId::all();
        let titles: Vec<&str> = all.iter().map(|p| p.title()).collect();
        let mut deduped = titles.clone();
        deduped.sort();
        deduped.dedup();
        assert_eq!(
            titles.len(),
            deduped.len(),
            "Duplicate persona variants found"
        );
    }

    #[test]
    fn persona_id_titles_are_nonempty() {
        for persona in PersonaId::all() {
            assert!(!persona.title().is_empty(), "{:?} has empty title", persona);
        }
    }

    #[test]
    fn persona_id_serialization_roundtrip() {
        for persona in PersonaId::all() {
            let json = serde_json::to_string(persona).unwrap();
            let deserialized: PersonaId = serde_json::from_str(&json).unwrap();
            assert_eq!(*persona, deserialized);
        }
    }

    #[test]
    fn persona_id_serializes_snake_case() {
        let json = serde_json::to_string(&PersonaId::ProgressiveActivist).unwrap();
        assert_eq!(json, r#""progressive_activist""#);

        let json = serde_json::to_string(&PersonaId::NationalSecurityHawk).unwrap();
        assert_eq!(json, r#""national_security_hawk""#);

        let json = serde_json::to_string(&PersonaId::PopulistAntiElite).unwrap();
        assert_eq!(json, r#""populist_anti_elite""#);
    }

    #[test]
    fn fact_check_assessment_serialization_roundtrip() {
        let variants = [
            FactCheckAssessment::Supported,
            FactCheckAssessment::Contested,
            FactCheckAssessment::Unsupported,
            FactCheckAssessment::Unclear,
        ];
        for v in &variants {
            let json = serde_json::to_string(v).unwrap();
            let deserialized: FactCheckAssessment = serde_json::from_str(&json).unwrap();
            assert_eq!(*v, deserialized);
        }
    }

    #[test]
    fn fact_check_assessment_serializes_lowercase() {
        assert_eq!(
            serde_json::to_string(&FactCheckAssessment::Supported).unwrap(),
            r#""supported""#
        );
        assert_eq!(
            serde_json::to_string(&FactCheckAssessment::Contested).unwrap(),
            r#""contested""#
        );
    }

    #[test]
    fn persona_output_serialization_roundtrip() {
        let output = PersonaOutput {
            id: PersonaId::CentristTechnocrat,
            title: "Centrist Technocrat".to_string(),
            stance_score: 0.1,
            confidence: 0.8,
            summary: "Seeks measurable outcomes.".to_string(),
            key_claims: vec!["KPIs missing".to_string()],
            fact_checks: vec![FactCheck {
                claim: "Costs are modest".to_string(),
                assessment: FactCheckAssessment::Unclear,
                rationale: "No transparent TCO provided.".to_string(),
            }],
            caveats: vec!["May appear aloof to rights framing".to_string()],
            axes: Some(Axes2D {
                economic: 0.0,
                social: 0.0,
            }),
        };

        let json = serde_json::to_string(&output).unwrap();
        let deserialized: PersonaOutput = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.id, PersonaId::CentristTechnocrat);
        assert_eq!(deserialized.title, "Centrist Technocrat");
        assert!((deserialized.stance_score - 0.1).abs() < f64::EPSILON);
        assert!((deserialized.confidence - 0.8).abs() < f64::EPSILON);
        assert_eq!(deserialized.key_claims.len(), 1);
        assert_eq!(deserialized.fact_checks.len(), 1);
        assert_eq!(
            deserialized.fact_checks[0].assessment,
            FactCheckAssessment::Unclear
        );
        assert!(deserialized.axes.is_some());
    }

    #[test]
    fn persona_output_without_axes() {
        let output = PersonaOutput {
            id: PersonaId::LibertarianCivil,
            title: "Libertarian".to_string(),
            stance_score: -2.6,
            confidence: 0.76,
            summary: "Privacy as fundamental liberty.".to_string(),
            key_claims: vec![],
            fact_checks: vec![],
            caveats: vec![],
            axes: None,
        };
        let json = serde_json::to_string(&output).unwrap();
        let deserialized: PersonaOutput = serde_json::from_str(&json).unwrap();
        assert!(deserialized.axes.is_none());
    }

    #[test]
    fn debiased_summary_serialization_roundtrip() {
        let summary = DebiasedSummary {
            consensus_points: vec!["Evidence is mixed".to_string()],
            disagreements: vec!["Weight of liberty vs safety".to_string()],
            likely_bias_drivers: vec!["Security-first framing".to_string()],
            truth_seeking_summary: "On balance, cautious.".to_string(),
            spectrum_score: -0.42,
            spectrum_explain: "Placement reflects persona-weighted views.".to_string(),
        };
        let json = serde_json::to_string(&summary).unwrap();
        let deserialized: DebiasedSummary = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.consensus_points.len(), 1);
        assert!((deserialized.spectrum_score - (-0.42)).abs() < f64::EPSILON);
    }

    #[test]
    fn analysis_result_serializes() {
        let result = AnalysisResult {
            title: "Test Article".to_string(),
            source_url: Some("https://example.com".to_string()),
            personas: vec![],
            debiaser: DebiasedSummary {
                consensus_points: vec![],
                disagreements: vec![],
                likely_bias_drivers: vec![],
                truth_seeking_summary: "Summary.".to_string(),
                spectrum_score: 0.0,
                spectrum_explain: "Neutral.".to_string(),
            },
            tone_analysis: None,
            source_meta: None,
            warnings: vec![],
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("title"));
        assert!(json.contains("personas"));
        assert!(json.contains("debiaser"));
        assert!(json.contains("spectrum_score"));
        // warnings should be omitted when empty (skip_serializing_if)
        assert!(!json.contains("warnings"));
    }

    #[test]
    fn analysis_result_with_none_source_url() {
        let result = AnalysisResult {
            title: "Pasted text".to_string(),
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
        let json = serde_json::to_string(&result).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed["source_url"].is_null());
    }

    #[test]
    fn analysis_result_with_warnings_serializes() {
        let result = AnalysisResult {
            title: "Partial".to_string(),
            source_url: None,
            personas: vec![],
            debiaser: DebiasedSummary {
                consensus_points: vec![],
                disagreements: vec![],
                likely_bias_drivers: vec![],
                truth_seeking_summary: "Partial.".to_string(),
                spectrum_score: 0.0,
                spectrum_explain: "N/A".to_string(),
            },
            tone_analysis: None,
            source_meta: None,
            warnings: vec![
                "2/8 personas failed: Progressive Activist, Centrist Technocrat".to_string(),
            ],
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("warnings"));
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["warnings"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn analysis_result_deserializes_without_warnings_field() {
        let json = r#"{
            "title": "Old format",
            "source_url": null,
            "personas": [],
            "debiaser": {
                "consensus_points": [],
                "disagreements": [],
                "likely_bias_drivers": [],
                "truth_seeking_summary": "Test.",
                "spectrum_score": 0.0,
                "spectrum_explain": "Test."
            }
        }"#;
        let result: AnalysisResult = serde_json::from_str(json).unwrap();
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn analysis_request_deserializes() {
        let json = r#"{"url": "https://example.com/article"}"#;
        let req: AnalysisRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.url, "https://example.com/article");
    }

    #[test]
    fn article_content_serialization_roundtrip() {
        let article = ArticleContent {
            title: "Test Article".to_string(),
            body_text: "Some body text".to_string(),
            meta_description: Some("A description".to_string()),
            source_url: "https://example.com".to_string(),
            paywalled: false,
            disclaimer: None,
        };
        let json = serde_json::to_string(&article).unwrap();
        let deserialized: ArticleContent = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.title, "Test Article");
        assert_eq!(
            deserialized.meta_description,
            Some("A description".to_string())
        );
    }

    #[test]
    fn article_content_with_none_meta() {
        let article = ArticleContent {
            title: "Title".to_string(),
            body_text: "Body".to_string(),
            meta_description: None,
            source_url: "https://example.com".to_string(),
            paywalled: false,
            disclaimer: None,
        };
        let json = serde_json::to_string(&article).unwrap();
        let deserialized: ArticleContent = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.meta_description, None);
    }

    #[test]
    fn error_response_serializes_with_error_and_details() {
        let resp = ErrorResponse {
            error: "Bad request".to_string(),
            details: Some("Missing URL field".to_string()),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["error"], "Bad request");
        assert_eq!(parsed["details"], "Missing URL field");
    }

    #[test]
    fn error_response_serializes_with_null_details() {
        let resp = ErrorResponse {
            error: "Server error".to_string(),
            details: None,
        };
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["error"], "Server error");
        assert!(parsed["details"].is_null());
    }

    #[test]
    fn error_response_has_required_fields() {
        let json = r#"{"error": "test", "details": null}"#;
        let parsed: serde_json::Value = serde_json::from_str(json).unwrap();
        assert!(parsed.get("error").is_some(), "error field must exist");
        assert!(parsed.get("details").is_some(), "details field must exist");
    }

    #[test]
    fn article_cache_type_is_arc_rwlock() {
        let cache: ArticleCache = std::sync::Arc::new(tokio::sync::RwLock::new(LruCache::new(
            NonZeroUsize::new(10).unwrap(),
        )));
        assert!(std::sync::Arc::strong_count(&cache) == 1);
    }

    #[test]
    fn generate_short_id_produces_8_chars() {
        let id = generate_short_id("https://example.com", "1234567890");
        assert_eq!(id.len(), 8);
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn generate_short_id_is_random() {
        // With cryptographic randomness, two IDs should (almost certainly) differ
        let id1 = generate_short_id("https://example.com", "123");
        let id2 = generate_short_id("https://example.com", "123");
        // Not asserting equality — IDs are now random, not deterministic
        assert_eq!(id1.len(), 8);
        assert_eq!(id2.len(), 8);
        assert!(id1.chars().all(|c| c.is_ascii_hexdigit()));
        assert!(id2.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn tone_analysis_serialization_roundtrip() {
        let tone = ToneAnalysis {
            rhetorical_devices: vec!["appeal to fear".to_string(), "loaded language".to_string()],
            emotional_tone: "alarmist".to_string(),
            framing_strategy: "conflict frame".to_string(),
            objectivity_score: 0.35,
        };
        let json = serde_json::to_string(&tone).unwrap();
        let deserialized: ToneAnalysis = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.rhetorical_devices.len(), 2);
        assert_eq!(deserialized.emotional_tone, "alarmist");
        assert_eq!(deserialized.framing_strategy, "conflict frame");
        assert!((deserialized.objectivity_score - 0.35).abs() < f64::EPSILON);
    }

    #[test]
    fn tone_analysis_empty_devices() {
        let tone = ToneAnalysis {
            rhetorical_devices: vec![],
            emotional_tone: "measured".to_string(),
            framing_strategy: "neutral reporting".to_string(),
            objectivity_score: 0.92,
        };
        let json = serde_json::to_string(&tone).unwrap();
        let deserialized: ToneAnalysis = serde_json::from_str(&json).unwrap();
        assert!(deserialized.rhetorical_devices.is_empty());
    }

    #[test]
    fn source_meta_serialization_roundtrip() {
        let meta = SourceMeta {
            publication: "The Guardian".to_string(),
            known_bias: Some("left-leaning".to_string()),
            ownership_type: Some("corporate".to_string()),
        };
        let json = serde_json::to_string(&meta).unwrap();
        let deserialized: SourceMeta = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.publication, "The Guardian");
        assert_eq!(deserialized.known_bias, Some("left-leaning".to_string()));
        assert_eq!(deserialized.ownership_type, Some("corporate".to_string()));
    }

    #[test]
    fn source_meta_with_none_fields() {
        let meta = SourceMeta {
            publication: "Unknown Blog".to_string(),
            known_bias: None,
            ownership_type: None,
        };
        let json = serde_json::to_string(&meta).unwrap();
        let deserialized: SourceMeta = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.publication, "Unknown Blog");
        assert!(deserialized.known_bias.is_none());
        assert!(deserialized.ownership_type.is_none());
    }

    #[test]
    fn analysis_result_with_tone_and_source_serializes() {
        let result = AnalysisResult {
            title: "Test".to_string(),
            source_url: Some("https://example.com".to_string()),
            personas: vec![],
            debiaser: DebiasedSummary {
                consensus_points: vec![],
                disagreements: vec![],
                likely_bias_drivers: vec![],
                truth_seeking_summary: "Test.".to_string(),
                spectrum_score: 0.0,
                spectrum_explain: "Test.".to_string(),
            },
            tone_analysis: Some(ToneAnalysis {
                rhetorical_devices: vec!["loaded language".to_string()],
                emotional_tone: "inflammatory".to_string(),
                framing_strategy: "conflict frame".to_string(),
                objectivity_score: 0.2,
            }),
            source_meta: Some(SourceMeta {
                publication: "Fox News".to_string(),
                known_bias: Some("right-leaning".to_string()),
                ownership_type: Some("corporate".to_string()),
            }),
            warnings: vec![],
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("tone_analysis"));
        assert!(json.contains("source_meta"));
        assert!(json.contains("rhetorical_devices"));
        assert!(json.contains("objectivity_score"));
        assert!(json.contains("publication"));
        assert!(json.contains("known_bias"));
    }

    #[test]
    fn analysis_result_without_tone_and_source_omits_fields() {
        let result = AnalysisResult {
            title: "Test".to_string(),
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
            tone_analysis: None,
            source_meta: None,
            warnings: vec![],
        };
        let json = serde_json::to_string(&result).unwrap();
        // tone_analysis and source_meta should be omitted when None
        assert!(!json.contains("tone_analysis"));
        assert!(!json.contains("source_meta"));
    }

    #[test]
    fn analysis_result_deserializes_without_tone_and_source() {
        // Backward-compat: old JSON without tone_analysis/source_meta should deserialize
        let json = r#"{
            "title": "Old format",
            "source_url": null,
            "personas": [],
            "debiaser": {
                "consensus_points": [],
                "disagreements": [],
                "likely_bias_drivers": [],
                "truth_seeking_summary": "Test.",
                "spectrum_score": 0.0,
                "spectrum_explain": "Test."
            }
        }"#;
        let result: AnalysisResult = serde_json::from_str(json).unwrap();
        assert!(result.tone_analysis.is_none());
        assert!(result.source_meta.is_none());
        assert!(result.warnings.is_empty());
    }
}
