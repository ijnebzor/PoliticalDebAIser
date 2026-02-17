use std::time::Duration;

use scraper::{Html, Selector};

use crate::models::{ArticleCache, ArticleContent};

/// Maximum article body length in characters before truncation.
const MAX_CONTENT_LENGTH: usize = 50_000;

/// Common paywall indicator strings found in page content.
const PAYWALL_INDICATORS: &[&str] = &[
    "subscribe to continue reading",
    "this article is for subscribers",
    "sign in to read the full article",
    "you've reached your free article limit",
    "become a member to read",
    "subscribe to unlock",
];

/// Scraper-specific errors for classifying failure modes.
#[derive(Debug)]
pub enum ScrapeError {
    InvalidUrl(String),
    Timeout(String),
    FetchFailed(String),
    NotFound,
    Paywall,
    NotHtml(String),
    EmptyContent,
}

impl std::fmt::Display for ScrapeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScrapeError::InvalidUrl(msg) => write!(f, "Invalid URL: {msg}"),
            ScrapeError::Timeout(msg) => write!(f, "Request timed out: {msg}"),
            ScrapeError::FetchFailed(msg) => write!(f, "Fetch failed: {msg}"),
            ScrapeError::NotFound => write!(f, "Page not found (404)"),
            ScrapeError::Paywall => write!(f, "Article appears to be behind a paywall"),
            ScrapeError::NotHtml(ct) => write!(f, "Not an HTML page (content-type: {ct})"),
            ScrapeError::EmptyContent => write!(f, "Article has no extractable text content"),
        }
    }
}

impl std::error::Error for ScrapeError {}

/// Fetch and extract an article, using the cache to avoid repeat fetches.
pub async fn scrape_article(
    url: &str,
    cache: &ArticleCache,
) -> Result<ArticleContent, ScrapeError> {
    // Validate URL format
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err(ScrapeError::InvalidUrl(
            "URL must start with http:// or https://".to_string(),
        ));
    }

    // Check cache
    {
        let cache_read = cache.read().await;
        if let Some(cached) = cache_read.get(url) {
            return Ok(cached.clone());
        }
    }

    // Fetch with a 30-second timeout
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| ScrapeError::FetchFailed(e.to_string()))?;

    let response = client.get(url).send().await.map_err(|e| {
        if e.is_timeout() {
            ScrapeError::Timeout(e.to_string())
        } else {
            ScrapeError::FetchFailed(e.to_string())
        }
    })?;

    // Check HTTP status
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(ScrapeError::NotFound);
    }
    if response.status() == reqwest::StatusCode::FORBIDDEN
        || response.status() == reqwest::StatusCode::PAYMENT_REQUIRED
    {
        return Err(ScrapeError::Paywall);
    }
    if !response.status().is_success() {
        return Err(ScrapeError::FetchFailed(format!(
            "HTTP {}",
            response.status()
        )));
    }

    // Check content type is HTML
    if let Some(ct) = response.headers().get(reqwest::header::CONTENT_TYPE) {
        if let Ok(ct_str) = ct.to_str() {
            if !ct_str.contains("text/html") && !ct_str.contains("application/xhtml") {
                return Err(ScrapeError::NotHtml(ct_str.to_string()));
            }
        }
    }

    let html_text = response
        .text()
        .await
        .map_err(|e| ScrapeError::FetchFailed(e.to_string()))?;

    // Parse synchronously so the non-Send Html type doesn't span an await
    let article = parse_html(&html_text, url)?;

    // Store in cache
    {
        let mut cache_write = cache.write().await;
        cache_write.insert(url.to_string(), article.clone());
    }

    Ok(article)
}

/// Parse raw HTML into an ArticleContent (synchronous, no await points).
fn parse_html(html_text: &str, url: &str) -> Result<ArticleContent, ScrapeError> {
    let document = Html::parse_document(html_text);

    let title = extract_title(&document).unwrap_or_else(|| "Untitled".to_string());
    let body_text = extract_body_text(&document);
    let meta_description = extract_meta_description(&document);

    if body_text.is_empty() {
        return Err(ScrapeError::EmptyContent);
    }

    // Check for paywall indicators in body content
    let body_lower = body_text.to_lowercase();
    let paywall_hits = PAYWALL_INDICATORS
        .iter()
        .filter(|indicator| body_lower.contains(**indicator))
        .count();
    // Require 2+ indicators, or 1+ on very short content, to reduce false positives
    if paywall_hits >= 2 || (paywall_hits >= 1 && body_text.len() < 500) {
        return Err(ScrapeError::Paywall);
    }

    // Truncate overly long content to avoid overwhelming the LLM
    let body_text = if body_text.len() > MAX_CONTENT_LENGTH {
        tracing::info!(
            "Truncated article body from {} to {} chars",
            body_text.len(),
            MAX_CONTENT_LENGTH
        );
        body_text.chars().take(MAX_CONTENT_LENGTH).collect()
    } else {
        body_text
    };

    Ok(ArticleContent {
        title,
        body_text,
        meta_description,
        source_url: url.to_string(),
    })
}

fn extract_title(document: &Html) -> Option<String> {
    // Try <title> tag first
    if let Some(title) = select_text(document, "title") {
        if !title.is_empty() {
            return Some(title);
        }
    }
    // Fall back to first <h1>
    select_text(document, "h1")
}

fn extract_body_text(document: &Html) -> String {
    // Try common article container selectors, fall back to <body>
    let selectors = ["article", "main", ".article-body", ".post-content", "body"];

    for sel_str in &selectors {
        if let Ok(selector) = Selector::parse(sel_str) {
            if let Some(element) = document.select(&selector).next() {
                let text: String = element
                    .text()
                    .collect::<Vec<_>>()
                    .join(" ");
                let cleaned = collapse_whitespace(&text);
                if !cleaned.is_empty() {
                    return cleaned;
                }
            }
        }
    }

    String::new()
}

fn extract_meta_description(document: &Html) -> Option<String> {
    let selector = Selector::parse(r#"meta[name="description"]"#).ok()?;
    document
        .select(&selector)
        .next()
        .and_then(|el| el.value().attr("content"))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn select_text(document: &Html, sel: &str) -> Option<String> {
    let selector = Selector::parse(sel).ok()?;
    document
        .select(&selector)
        .next()
        .map(|el| el.text().collect::<String>().trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Collapse runs of whitespace (spaces, newlines, tabs) into single spaces.
fn collapse_whitespace(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_title_from_title_tag() {
        let html = Html::parse_document("<html><head><title>Test Title</title></head><body></body></html>");
        assert_eq!(extract_title(&html), Some("Test Title".to_string()));
    }

    #[test]
    fn extract_title_falls_back_to_h1() {
        let html = Html::parse_document("<html><body><h1>Heading Title</h1></body></html>");
        assert_eq!(extract_title(&html), Some("Heading Title".to_string()));
    }

    #[test]
    fn extract_title_prefers_title_tag_over_h1() {
        let html = Html::parse_document(
            "<html><head><title>Title Tag</title></head><body><h1>H1 Tag</h1></body></html>",
        );
        assert_eq!(extract_title(&html), Some("Title Tag".to_string()));
    }

    #[test]
    fn extract_title_returns_none_for_empty() {
        let html = Html::parse_document("<html><head><title></title></head><body></body></html>");
        assert_eq!(extract_title(&html), None);
    }

    #[test]
    fn extract_body_text_from_article_tag() {
        let html = Html::parse_document(
            "<html><body><article><p>Article content here.</p></article></body></html>",
        );
        let text = extract_body_text(&html);
        assert!(text.contains("Article content here."));
    }

    #[test]
    fn extract_body_text_from_main_tag() {
        let html = Html::parse_document(
            "<html><body><main><p>Main content.</p></main></body></html>",
        );
        let text = extract_body_text(&html);
        assert!(text.contains("Main content."));
    }

    #[test]
    fn extract_body_text_falls_back_to_body() {
        let html = Html::parse_document(
            "<html><body><div><p>Body fallback content.</p></div></body></html>",
        );
        let text = extract_body_text(&html);
        assert!(text.contains("Body fallback content."));
    }

    #[test]
    fn extract_body_text_collapses_whitespace() {
        let html = Html::parse_document(
            "<html><body>  lots   of   spaces\n\nand\nnewlines  </body></html>",
        );
        let text = extract_body_text(&html);
        assert_eq!(text, "lots of spaces and newlines");
    }

    #[test]
    fn extract_body_text_returns_empty_for_no_content() {
        let html = Html::parse_document("<html><head></head></html>");
        let text = extract_body_text(&html);
        assert!(text.is_empty());
    }

    #[test]
    fn extract_meta_description_present() {
        let html = Html::parse_document(
            r#"<html><head><meta name="description" content="A test article about things."></head><body></body></html>"#,
        );
        assert_eq!(
            extract_meta_description(&html),
            Some("A test article about things.".to_string())
        );
    }

    #[test]
    fn extract_meta_description_missing() {
        let html = Html::parse_document("<html><head></head><body></body></html>");
        assert_eq!(extract_meta_description(&html), None);
    }

    #[test]
    fn extract_meta_description_empty_content() {
        let html = Html::parse_document(
            r#"<html><head><meta name="description" content="   "></head><body></body></html>"#,
        );
        assert_eq!(extract_meta_description(&html), None);
    }

    #[test]
    fn collapse_whitespace_works() {
        assert_eq!(collapse_whitespace("  hello   world  "), "hello world");
        assert_eq!(collapse_whitespace("no\nnewlines\there"), "no newlines here");
        assert_eq!(collapse_whitespace(""), "");
        assert_eq!(collapse_whitespace("   "), "");
    }

    #[test]
    fn select_text_returns_trimmed() {
        let html = Html::parse_document("<html><body><p>  trimmed  </p></body></html>");
        assert_eq!(select_text(&html, "p"), Some("trimmed".to_string()));
    }

    #[test]
    fn select_text_returns_none_for_missing_element() {
        let html = Html::parse_document("<html><body></body></html>");
        assert_eq!(select_text(&html, "h2"), None);
    }

    #[test]
    fn parse_html_extracts_article() {
        let html = r#"<html><head><title>Test</title></head><body><article><p>Content here</p></article></body></html>"#;
        let result = parse_html(html, "https://example.com");
        assert!(result.is_ok());
        let article = result.unwrap();
        assert_eq!(article.title, "Test");
        assert!(article.body_text.contains("Content here"));
        assert_eq!(article.source_url, "https://example.com");
    }

    #[test]
    fn parse_html_returns_empty_content_error() {
        let html = "<html><head></head></html>";
        let result = parse_html(html, "https://example.com");
        assert!(result.is_err());
        match result.unwrap_err() {
            ScrapeError::EmptyContent => {}
            other => panic!("Expected EmptyContent, got {other}"),
        }
    }

    #[test]
    fn parse_html_uses_untitled_for_missing_title() {
        let html = "<html><body><article><p>Some text</p></article></body></html>";
        let result = parse_html(html, "https://example.com").unwrap();
        assert_eq!(result.title, "Untitled");
    }

    #[test]
    fn scrape_error_display_messages() {
        assert!(ScrapeError::InvalidUrl("bad".to_string()).to_string().contains("Invalid URL"));
        assert!(ScrapeError::Timeout("slow".to_string()).to_string().contains("timed out"));
        assert!(ScrapeError::FetchFailed("nope".to_string()).to_string().contains("Fetch failed"));
        assert!(ScrapeError::EmptyContent.to_string().contains("no extractable text"));
        assert!(ScrapeError::NotFound.to_string().contains("404"));
        assert!(ScrapeError::Paywall.to_string().contains("paywall"));
        assert!(ScrapeError::NotHtml("application/pdf".to_string()).to_string().contains("non-HTML") || ScrapeError::NotHtml("application/pdf".to_string()).to_string().contains("Not an HTML"));
    }

    #[test]
    fn parse_html_detects_paywall_short_content() {
        let html = "<html><body><article><p>Subscribe to continue reading.</p></article></body></html>";
        let result = parse_html(html, "https://example.com");
        assert!(result.is_err());
        match result.unwrap_err() {
            ScrapeError::Paywall => {}
            other => panic!("Expected Paywall, got {other}"),
        }
    }

    #[test]
    fn parse_html_detects_paywall_multiple_indicators() {
        let filler = "Normal article text. ".repeat(50);
        let html = format!(
            "<html><body><article><p>{filler} Subscribe to continue reading. This article is for subscribers only.</p></article></body></html>"
        );
        let result = parse_html(&html, "https://example.com");
        assert!(result.is_err());
        match result.unwrap_err() {
            ScrapeError::Paywall => {}
            other => panic!("Expected Paywall, got {other}"),
        }
    }

    #[test]
    fn parse_html_no_false_paywall_on_normal_content() {
        let html = "<html><body><article><p>This is a perfectly normal article about politics and policy. It discusses various perspectives on current events and does not restrict access.</p></article></body></html>";
        let result = parse_html(html, "https://example.com");
        assert!(result.is_ok());
    }

    #[test]
    fn parse_html_truncates_long_content() {
        let long_text = "a ".repeat(MAX_CONTENT_LENGTH + 1000);
        let html = format!(
            "<html><body><article><p>{long_text}</p></article></body></html>"
        );
        let result = parse_html(&html, "https://example.com").unwrap();
        assert!(result.body_text.len() <= MAX_CONTENT_LENGTH);
    }

    #[test]
    fn invalid_url_rejected() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let cache: ArticleCache = std::sync::Arc::new(tokio::sync::RwLock::new(
                std::collections::HashMap::new(),
            ));
            let result = scrape_article("not-a-url", &cache).await;
            assert!(result.is_err());
            match result.unwrap_err() {
                ScrapeError::InvalidUrl(_) => {}
                other => panic!("Expected InvalidUrl, got {other}"),
            }
        });
    }

    #[test]
    fn ftp_url_rejected() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let cache: ArticleCache = std::sync::Arc::new(tokio::sync::RwLock::new(
                std::collections::HashMap::new(),
            ));
            let result = scrape_article("ftp://example.com/file", &cache).await;
            assert!(result.is_err());
            match result.unwrap_err() {
                ScrapeError::InvalidUrl(_) => {}
                other => panic!("Expected InvalidUrl for ftp://, got {other}"),
            }
        });
    }

    #[test]
    fn empty_url_rejected() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let cache: ArticleCache = std::sync::Arc::new(tokio::sync::RwLock::new(
                std::collections::HashMap::new(),
            ));
            let result = scrape_article("", &cache).await;
            assert!(result.is_err());
            match result.unwrap_err() {
                ScrapeError::InvalidUrl(_) => {}
                other => panic!("Expected InvalidUrl for empty string, got {other}"),
            }
        });
    }

    #[test]
    fn javascript_url_rejected() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let cache: ArticleCache = std::sync::Arc::new(tokio::sync::RwLock::new(
                std::collections::HashMap::new(),
            ));
            let result = scrape_article("javascript:alert(1)", &cache).await;
            assert!(result.is_err());
            match result.unwrap_err() {
                ScrapeError::InvalidUrl(_) => {}
                other => panic!("Expected InvalidUrl for javascript:, got {other}"),
            }
        });
    }

    #[test]
    fn cache_returns_cached_article() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let cache: ArticleCache = std::sync::Arc::new(tokio::sync::RwLock::new(
                std::collections::HashMap::new(),
            ));

            // Pre-populate the cache
            let article = ArticleContent {
                title: "Cached Title".to_string(),
                body_text: "Cached body text".to_string(),
                meta_description: None,
                source_url: "https://example.com/cached".to_string(),
            };
            {
                let mut cache_write = cache.write().await;
                cache_write.insert("https://example.com/cached".to_string(), article.clone());
            }

            // Fetch should hit the cache (no network call)
            let result = scrape_article("https://example.com/cached", &cache).await;
            assert!(result.is_ok());
            let cached = result.unwrap();
            assert_eq!(cached.title, "Cached Title");
            assert_eq!(cached.body_text, "Cached body text");
        });
    }
}
