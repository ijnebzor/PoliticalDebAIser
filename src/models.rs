use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use serde::{Deserialize, Serialize};

/// The political archetypes used for analysis.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ArchetypeKind {
    Conservative,
    Democrat,
    Socialist,
    Dictatorship,
    Anarchist,
}

impl ArchetypeKind {
    /// Returns all archetype variants.
    pub fn all() -> &'static [ArchetypeKind] {
        &[
            ArchetypeKind::Conservative,
            ArchetypeKind::Democrat,
            ArchetypeKind::Socialist,
            ArchetypeKind::Dictatorship,
            ArchetypeKind::Anarchist,
        ]
    }

    pub fn label(&self) -> &'static str {
        match self {
            ArchetypeKind::Conservative => "Conservative",
            ArchetypeKind::Democrat => "Democrat",
            ArchetypeKind::Socialist => "Socialist",
            ArchetypeKind::Dictatorship => "Dictatorship",
            ArchetypeKind::Anarchist => "Anarchist",
        }
    }
}

/// Analysis result for a single archetype perspective.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchetypeAnalysis {
    pub archetype: ArchetypeKind,
    pub summary: String,
    pub highlights: Vec<String>,
    pub alignment_score: f64,
}

/// Scraped article content extracted from a URL.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArticleContent {
    pub title: String,
    pub body_text: String,
    pub meta_description: Option<String>,
    pub source_url: String,
}

/// Incoming request to analyze an article URL.
#[derive(Debug, Deserialize)]
pub struct AnalysisRequest {
    pub url: String,
}

/// Full analysis response returned to the client.
#[derive(Debug, Serialize)]
pub struct AnalysisResponse {
    pub article_title: String,
    pub article_summary: String,
    pub analyses: Vec<ArchetypeAnalysis>,
    pub synthesis: Option<String>,
    pub commonalities: Option<Vec<String>>,
}

/// Structured JSON error response.
#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: String,
    pub details: Option<String>,
}

/// Shared article cache: URL -> ArticleContent.
pub type ArticleCache = Arc<RwLock<HashMap<String, ArticleContent>>>;

/// Request body for the /synthesize endpoint.
#[derive(Debug, Deserialize)]
pub struct SynthesisRequest {
    pub analyses: Vec<ArchetypeAnalysis>,
}

/// Response from the /synthesize endpoint.
#[derive(Debug, Serialize, Deserialize)]
pub struct SynthesisResponse {
    pub synthesis: String,
    pub commonalities: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archetype_kind_all_returns_five() {
        assert_eq!(ArchetypeKind::all().len(), 5);
    }

    #[test]
    fn archetype_kind_all_variants_are_unique() {
        let all = ArchetypeKind::all();
        let labels: Vec<&str> = all.iter().map(|k| k.label()).collect();
        let mut deduped = labels.clone();
        deduped.sort();
        deduped.dedup();
        assert_eq!(labels.len(), deduped.len(), "Duplicate archetype variants found");
    }

    #[test]
    fn archetype_kind_labels_are_nonempty() {
        for kind in ArchetypeKind::all() {
            assert!(!kind.label().is_empty(), "{:?} has empty label", kind);
        }
    }

    #[test]
    fn archetype_kind_serialization_roundtrip() {
        for kind in ArchetypeKind::all() {
            let json = serde_json::to_string(kind).unwrap();
            let deserialized: ArchetypeKind = serde_json::from_str(&json).unwrap();
            assert_eq!(*kind, deserialized);
        }
    }

    #[test]
    fn archetype_kind_serializes_lowercase() {
        let json = serde_json::to_string(&ArchetypeKind::Conservative).unwrap();
        assert_eq!(json, r#""conservative""#);

        let json = serde_json::to_string(&ArchetypeKind::Democrat).unwrap();
        assert_eq!(json, r#""democrat""#);
    }

    #[test]
    fn analysis_request_deserializes() {
        let json = r#"{"url": "https://example.com/article"}"#;
        let req: AnalysisRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.url, "https://example.com/article");
    }

    #[test]
    fn archetype_analysis_serialization_roundtrip() {
        let analysis = ArchetypeAnalysis {
            archetype: ArchetypeKind::Socialist,
            summary: "Test summary".to_string(),
            highlights: vec!["point 1".to_string(), "point 2".to_string()],
            alignment_score: 0.75,
        };

        let json = serde_json::to_string(&analysis).unwrap();
        let deserialized: ArchetypeAnalysis = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.archetype, ArchetypeKind::Socialist);
        assert_eq!(deserialized.summary, "Test summary");
        assert_eq!(deserialized.highlights.len(), 2);
        assert!((deserialized.alignment_score - 0.75).abs() < f64::EPSILON);
    }

    #[test]
    fn analysis_response_serializes() {
        let resp = AnalysisResponse {
            article_title: "Title".to_string(),
            article_summary: "Summary...".to_string(),
            analyses: vec![],
            synthesis: Some("Balanced view".to_string()),
            commonalities: Some(vec!["Point of agreement".to_string()]),
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("article_title"));
        assert!(json.contains("synthesis"));
        assert!(json.contains("commonalities"));
    }

    #[test]
    fn synthesis_request_deserializes() {
        let json = r#"{"analyses": []}"#;
        let req: SynthesisRequest = serde_json::from_str(json).unwrap();
        assert!(req.analyses.is_empty());
    }

    #[test]
    fn synthesis_response_serializes() {
        let resp = SynthesisResponse {
            synthesis: "Balanced synthesis".to_string(),
            commonalities: vec!["All agree on X".to_string()],
        };
        let json = serde_json::to_string(&resp).unwrap();
        let deserialized: SynthesisResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.synthesis, "Balanced synthesis");
        assert_eq!(deserialized.commonalities.len(), 1);
    }

    #[test]
    fn article_content_serialization_roundtrip() {
        let article = ArticleContent {
            title: "Test Article".to_string(),
            body_text: "Some body text".to_string(),
            meta_description: Some("A description".to_string()),
            source_url: "https://example.com".to_string(),
        };
        let json = serde_json::to_string(&article).unwrap();
        let deserialized: ArticleContent = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.title, "Test Article");
        assert_eq!(deserialized.meta_description, Some("A description".to_string()));
    }

    #[test]
    fn article_content_with_none_meta() {
        let article = ArticleContent {
            title: "Title".to_string(),
            body_text: "Body".to_string(),
            meta_description: None,
            source_url: "https://example.com".to_string(),
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
        // Verify the cache can be created and used
        let cache: ArticleCache = std::sync::Arc::new(tokio::sync::RwLock::new(HashMap::new()));
        assert!(std::sync::Arc::strong_count(&cache) == 1);
    }
}
