use std::net::{IpAddr, ToSocketAddrs};
use std::time::Duration;

use scraper::{ElementRef, Html, Selector};
use serde::{Deserialize, Serialize};

use crate::models::{ArticleCache, ArticleContent};

/// Maximum article body length in characters before truncation.
const MAX_CONTENT_LENGTH: usize = 50_000;

// =============================================================================
// Source metadata extraction
// =============================================================================

/// Source metadata extracted from an article's URL/domain.
/// This is the scraper-level extraction; routes.rs converts to models::SourceMeta for the API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScrapedSourceMeta {
    /// Publication name (e.g., "The New York Times").
    pub publication: String,
    /// Domain of the source URL.
    pub domain: String,
    /// Known political leaning/bias indicator, if identified.
    pub known_bias: Option<String>,
    /// Type of media outlet (e.g., "mainstream", "wire_service", "public_media").
    pub media_type: Option<String>,
}

/// Entry in the known publications database.
struct KnownPublication {
    name: &'static str,
    bias: &'static str,
    media_type: &'static str,
}

/// Static database of known news publications mapped by domain.
/// Bias labels follow the Ad Fontes / AllSides spectrum:
/// "left", "center-left", "center", "center-right", "right", "libertarian".
fn lookup_known_publication(domain: &str) -> Option<&'static KnownPublication> {
    static KNOWN: &[(&str, KnownPublication)] = &[
        // Wire services
        (
            "reuters.com",
            KnownPublication {
                name: "Reuters",
                bias: "center",
                media_type: "wire_service",
            },
        ),
        (
            "apnews.com",
            KnownPublication {
                name: "Associated Press",
                bias: "center",
                media_type: "wire_service",
            },
        ),
        // Public media
        (
            "bbc.com",
            KnownPublication {
                name: "BBC",
                bias: "center",
                media_type: "public_media",
            },
        ),
        (
            "bbc.co.uk",
            KnownPublication {
                name: "BBC",
                bias: "center",
                media_type: "public_media",
            },
        ),
        (
            "npr.org",
            KnownPublication {
                name: "NPR",
                bias: "center-left",
                media_type: "public_media",
            },
        ),
        (
            "abc.net.au",
            KnownPublication {
                name: "ABC News (Australia)",
                bias: "center",
                media_type: "public_media",
            },
        ),
        (
            "pbs.org",
            KnownPublication {
                name: "PBS",
                bias: "center-left",
                media_type: "public_media",
            },
        ),
        // Mainstream (left / center-left)
        (
            "nytimes.com",
            KnownPublication {
                name: "The New York Times",
                bias: "center-left",
                media_type: "mainstream",
            },
        ),
        (
            "washingtonpost.com",
            KnownPublication {
                name: "The Washington Post",
                bias: "center-left",
                media_type: "mainstream",
            },
        ),
        (
            "cnn.com",
            KnownPublication {
                name: "CNN",
                bias: "center-left",
                media_type: "mainstream",
            },
        ),
        (
            "msnbc.com",
            KnownPublication {
                name: "MSNBC",
                bias: "left",
                media_type: "mainstream",
            },
        ),
        (
            "theguardian.com",
            KnownPublication {
                name: "The Guardian",
                bias: "center-left",
                media_type: "mainstream",
            },
        ),
        (
            "huffpost.com",
            KnownPublication {
                name: "HuffPost",
                bias: "left",
                media_type: "mainstream",
            },
        ),
        (
            "vox.com",
            KnownPublication {
                name: "Vox",
                bias: "left",
                media_type: "mainstream",
            },
        ),
        (
            "slate.com",
            KnownPublication {
                name: "Slate",
                bias: "center-left",
                media_type: "mainstream",
            },
        ),
        (
            "theatlantic.com",
            KnownPublication {
                name: "The Atlantic",
                bias: "center-left",
                media_type: "mainstream",
            },
        ),
        // Mainstream (center)
        (
            "politico.com",
            KnownPublication {
                name: "Politico",
                bias: "center",
                media_type: "mainstream",
            },
        ),
        (
            "thehill.com",
            KnownPublication {
                name: "The Hill",
                bias: "center",
                media_type: "mainstream",
            },
        ),
        (
            "axios.com",
            KnownPublication {
                name: "Axios",
                bias: "center",
                media_type: "mainstream",
            },
        ),
        (
            "bloomberg.com",
            KnownPublication {
                name: "Bloomberg",
                bias: "center",
                media_type: "mainstream",
            },
        ),
        (
            "usatoday.com",
            KnownPublication {
                name: "USA Today",
                bias: "center",
                media_type: "mainstream",
            },
        ),
        // Mainstream (center-right / right)
        (
            "wsj.com",
            KnownPublication {
                name: "The Wall Street Journal",
                bias: "center-right",
                media_type: "mainstream",
            },
        ),
        (
            "foxnews.com",
            KnownPublication {
                name: "Fox News",
                bias: "right",
                media_type: "mainstream",
            },
        ),
        (
            "nypost.com",
            KnownPublication {
                name: "New York Post",
                bias: "right",
                media_type: "tabloid",
            },
        ),
        (
            "dailymail.co.uk",
            KnownPublication {
                name: "Daily Mail",
                bias: "right",
                media_type: "tabloid",
            },
        ),
        // Independent (left)
        (
            "theintercept.com",
            KnownPublication {
                name: "The Intercept",
                bias: "left",
                media_type: "independent",
            },
        ),
        (
            "jacobin.com",
            KnownPublication {
                name: "Jacobin",
                bias: "left",
                media_type: "independent",
            },
        ),
        (
            "motherjones.com",
            KnownPublication {
                name: "Mother Jones",
                bias: "left",
                media_type: "independent",
            },
        ),
        // Independent (right)
        (
            "breitbart.com",
            KnownPublication {
                name: "Breitbart",
                bias: "right",
                media_type: "independent",
            },
        ),
        (
            "dailywire.com",
            KnownPublication {
                name: "The Daily Wire",
                bias: "right",
                media_type: "independent",
            },
        ),
        (
            "nationalreview.com",
            KnownPublication {
                name: "National Review",
                bias: "right",
                media_type: "independent",
            },
        ),
        (
            "thefederalist.com",
            KnownPublication {
                name: "The Federalist",
                bias: "right",
                media_type: "independent",
            },
        ),
        // Libertarian
        (
            "reason.com",
            KnownPublication {
                name: "Reason",
                bias: "libertarian",
                media_type: "independent",
            },
        ),
        // State-affiliated
        (
            "rt.com",
            KnownPublication {
                name: "RT (Russia Today)",
                bias: "right",
                media_type: "state_affiliated",
            },
        ),
        (
            "aljazeera.com",
            KnownPublication {
                name: "Al Jazeera",
                bias: "center",
                media_type: "state_affiliated",
            },
        ),
    ];

    KNOWN
        .iter()
        .find(|(d, _)| *d == domain)
        .map(|(_, pub_info)| pub_info)
}

/// Extract source metadata from an article URL.
///
/// Looks up the domain against a database of known publications to identify
/// the publication name, political leaning, and media type. For unknown domains,
/// derives a publication name from the domain itself.
pub fn extract_source_meta(url: &str) -> ScrapedSourceMeta {
    let domain = extract_domain(url);

    if let Some(known) = lookup_known_publication(&domain) {
        return ScrapedSourceMeta {
            publication: known.name.to_string(),
            domain: domain.clone(),
            known_bias: Some(known.bias.to_string()),
            media_type: Some(known.media_type.to_string()),
        };
    }

    // Unknown domain — derive a readable name from it
    let publication = domain_to_publication_name(&domain);

    ScrapedSourceMeta {
        publication,
        domain,
        known_bias: None,
        media_type: None,
    }
}

/// Extract the registrable domain from a URL, stripping "www." prefix.
fn extract_domain(url: &str) -> String {
    let parsed = match url::Url::parse(url) {
        Ok(u) => u,
        Err(_) => return String::new(),
    };
    let host = match parsed.host_str() {
        Some(h) => h.to_lowercase(),
        None => return String::new(),
    };
    host.strip_prefix("www.").unwrap_or(&host).to_string()
}

/// Derive a human-readable publication name from a domain.
/// E.g., "example-news.com" -> "Example News", "thedailybeast.com" -> "Thedailybeast".
fn domain_to_publication_name(domain: &str) -> String {
    // Strip TLD
    let base = domain.split('.').next().unwrap_or(domain);
    // Split on hyphens, capitalize each word
    base.split('-')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(c) => {
                    let upper: String = c.to_uppercase().collect();
                    format!("{upper}{}", chars.as_str())
                }
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

// =============================================================================
// SSRF protection
// =============================================================================

/// Check if an IP address is private, loopback, or otherwise non-public.
fn is_private_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()           // 127.0.0.0/8
                || v4.is_private()     // 10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16
                || v4.is_link_local()  // 169.254.0.0/16
                || v4.is_unspecified() // 0.0.0.0
                || v4.is_broadcast()   // 255.255.255.255
                || v4.octets()[0] == 100 && (v4.octets()[1] & 0xC0) == 64 // 100.64.0.0/10 (CGNAT)
        }
        IpAddr::V6(v6) => {
            v6.is_loopback()           // ::1
                || v6.is_unspecified() // ::
                // Unique local addresses fc00::/7
                || (v6.segments()[0] & 0xfe00) == 0xfc00
                // Link-local fe80::/10
                || (v6.segments()[0] & 0xffc0) == 0xfe80
        }
    }
}

/// Resolved URL target: hostname + validated IP address to pin requests against DNS rebinding.
struct ResolvedTarget {
    host: String,
    port: u16,
    ip: IpAddr,
}

/// Validate that a URL does not target private/internal network addresses (SSRF protection).
/// Returns the resolved IP so callers can pin the connection, preventing TOCTOU DNS rebinding.
fn validate_url_target(url: &str) -> Result<ResolvedTarget, ScrapeError> {
    let parsed =
        url::Url::parse(url).map_err(|_| ScrapeError::InvalidUrl("Malformed URL".to_string()))?;

    let host = parsed
        .host_str()
        .ok_or_else(|| ScrapeError::InvalidUrl("URL has no host".to_string()))?;

    validate_hostname(host)?;

    let port = parsed.port_or_known_default().unwrap_or(80);
    let ip = resolve_and_validate(host, port)?;

    Ok(ResolvedTarget {
        host: host.to_string(),
        port,
        ip,
    })
}

/// Block common internal/private hostnames.
fn validate_hostname(host: &str) -> Result<(), ScrapeError> {
    let host_lower = host.to_lowercase();
    if host_lower == "localhost"
        || host_lower.ends_with(".local")
        || host_lower.ends_with(".internal")
        || host_lower.ends_with(".corp")
        || host_lower == "metadata.google.internal"
    {
        return Err(ScrapeError::InvalidUrl(
            "URLs targeting internal/private hosts are not allowed".to_string(),
        ));
    }
    Ok(())
}

/// Resolve hostname to IP and validate none are private. Returns the first public IP.
fn resolve_and_validate(host: &str, port: u16) -> Result<IpAddr, ScrapeError> {
    let addr_str = format!("{host}:{port}");
    let addrs: Vec<_> = addr_str
        .to_socket_addrs()
        .map_err(|_| ScrapeError::InvalidUrl(format!("Could not resolve hostname: {host}")))?
        .collect();

    if addrs.is_empty() {
        return Err(ScrapeError::InvalidUrl(format!(
            "Hostname resolved to no addresses: {host}"
        )));
    }

    for addr in &addrs {
        if is_private_ip(&addr.ip()) {
            return Err(ScrapeError::InvalidUrl(
                "URLs resolving to private/internal IP addresses are not allowed".to_string(),
            ));
        }
    }

    Ok(addrs[0].ip())
}

/// Build a reqwest client pinned to a specific resolved IP, preventing DNS rebinding.
/// Also installs a redirect policy that validates each redirect target against SSRF rules.
fn build_pinned_client(target: &ResolvedTarget) -> Result<reqwest::Client, ScrapeError> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        // Pin DNS: force this hostname to resolve to the validated IP
        .resolve(
            &target.host,
            std::net::SocketAddr::new(target.ip, target.port),
        )
        // Custom redirect policy: validate each redirect target against SSRF rules
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 5 {
                attempt.error("too many redirects")
            } else if let Some(redirect_host) = attempt.url().host_str() {
                // Validate the redirect hostname
                if validate_hostname(redirect_host).is_err() {
                    return attempt.error("redirect to private/internal host blocked");
                }
                // Validate the redirect IP
                let port = attempt.url().port_or_known_default().unwrap_or(80);
                if resolve_and_validate(redirect_host, port).is_err() {
                    return attempt.error("redirect to private/internal IP blocked");
                }
                attempt.follow()
            } else {
                attempt.error("redirect URL has no host")
            }
        }))
        .build()
        .map_err(|e| ScrapeError::FetchFailed(e.to_string()))
}

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

const ARCHIVE_DISCLAIMER: &str = "This article was accessed via archive.ph for analysis purposes only. No copyright infringement intended.";

/// Fetch and extract an article, using the cache to avoid repeat fetches.
/// If the article is behind a paywall, automatically retries via archive.ph.
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

    // SSRF protection: resolve DNS once and pin the IP to prevent TOCTOU rebinding
    let target = validate_url_target(url)?;

    // Check cache (write lock needed for LRU access-order update)
    {
        let mut cache_lock = cache.write().await;
        if let Some(cached) = cache_lock.get(url) {
            return Ok(cached.clone());
        }
    }

    // Try fetching the article directly first (pinned to validated IP)
    match fetch_and_parse(url, &target).await {
        Ok(article) => {
            // Store in cache
            let mut cache_write = cache.write().await;
            cache_write.put(url.to_string(), article.clone());
            Ok(article)
        }
        Err(ScrapeError::Paywall) => {
            // Paywall detected — try archive.ph fallback
            tracing::info!("Paywall detected for {url}, trying archive.ph fallback");
            match try_archive_ph(url).await {
                Ok(mut article) => {
                    article.source_url = url.to_string();
                    article.paywalled = true;
                    article.disclaimer = Some(ARCHIVE_DISCLAIMER.to_string());
                    // Cache the archive.ph result under the original URL
                    let mut cache_write = cache.write().await;
                    cache_write.put(url.to_string(), article.clone());
                    Ok(article)
                }
                Err(archive_err) => {
                    tracing::warn!("archive.ph fallback failed for {url}: {archive_err}");
                    Err(ScrapeError::Paywall)
                }
            }
        }
        Err(e) => Err(e),
    }
}

/// Fetch a URL and parse its HTML into ArticleContent.
/// Uses a pinned client to prevent DNS rebinding TOCTOU attacks.
async fn fetch_and_parse(
    url: &str,
    target: &ResolvedTarget,
) -> Result<ArticleContent, ScrapeError> {
    let client = build_pinned_client(target)?;

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
    if let Some(ct) = response.headers().get(reqwest::header::CONTENT_TYPE)
        && let Ok(ct_str) = ct.to_str()
        && !ct_str.contains("text/html")
        && !ct_str.contains("application/xhtml")
    {
        return Err(ScrapeError::NotHtml(ct_str.to_string()));
    }

    let html_text = response
        .text()
        .await
        .map_err(|e| ScrapeError::FetchFailed(e.to_string()))?;

    parse_html(&html_text, url)
}

/// Try fetching an article via archive.ph as a paywall bypass.
/// Uses the same SSRF protections (DNS pinning, redirect validation) as the primary scraper.
async fn try_archive_ph(original_url: &str) -> Result<ArticleContent, ScrapeError> {
    let archive_url = format!("https://archive.ph/newest/{original_url}");

    // SSRF protection: validate and pin the archive.ph target IP
    let target = validate_url_target(&archive_url)?;
    let client = build_pinned_client(&target)?;

    let response = client.get(&archive_url).send().await.map_err(|e| {
        if e.is_timeout() {
            ScrapeError::Timeout(e.to_string())
        } else {
            ScrapeError::FetchFailed(e.to_string())
        }
    })?;

    if !response.status().is_success() {
        return Err(ScrapeError::FetchFailed(format!(
            "archive.ph returned HTTP {}",
            response.status()
        )));
    }

    let html_text = response
        .text()
        .await
        .map_err(|e| ScrapeError::FetchFailed(e.to_string()))?;

    parse_html(&html_text, original_url)
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
        paywalled: false,
        disclaimer: None,
    })
}

fn extract_title(document: &Html) -> Option<String> {
    // 1. Try og:title meta tag (most reliable for articles)
    if let Some(og_title) = extract_og_title(document) {
        return Some(og_title);
    }
    // 2. Try <title> tag
    if let Some(title) = select_text(document, "title")
        && !title.is_empty()
    {
        return Some(title);
    }
    // 3. Fall back to first <h1>
    select_text(document, "h1")
}

fn extract_og_title(document: &Html) -> Option<String> {
    let selector = Selector::parse(r#"meta[property="og:title"]"#).ok()?;
    document
        .select(&selector)
        .next()
        .and_then(|el| el.value().attr("content"))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Tags whose subtrees should be skipped during text extraction.
const STRIP_TAGS: &[&str] = &[
    "nav", "header", "footer", "aside", "script", "style", "noscript", "svg", "form",
];

/// Class substrings that indicate non-content elements.
const STRIP_CLASSES: &[&str] = &[
    "sidebar",
    "comment",
    "nav",
    "menu",
    "footer",
    "header",
    "social-share",
    "share",
    "related",
    "ad-",
    "advertisement",
    "promo",
    "newsletter",
    "popup",
];

/// Container selectors to try, in priority order. First match with enough text wins.
const CONTENT_SELECTORS: &[&str] = &[
    "[itemprop=articleBody]",
    "[role=main]",
    "article",
    ".article-body",
    ".entry-content",
    ".story-body",
    ".post-body",
    ".post-content",
    "main",
    "body",
];

fn extract_body_text(document: &Html) -> String {
    // Try each content selector; score all candidates and pick the best
    let mut best_text = String::new();
    let mut best_score: usize = 0;

    for sel_str in CONTENT_SELECTORS {
        if let Ok(selector) = Selector::parse(sel_str) {
            for element in document.select(&selector) {
                let score = score_content_node(element);
                if score > best_score {
                    let text = extract_clean_text(element);
                    if !text.is_empty() {
                        best_score = score;
                        best_text = text;
                    }
                }
            }
        }
    }

    best_text
}

/// Recursively extract text from an element, skipping non-content subtrees.
fn extract_clean_text(element: ElementRef) -> String {
    let mut parts = Vec::new();
    collect_text_recursive(element, &mut parts);
    collapse_whitespace(&parts.join(" "))
}

fn collect_text_recursive(element: ElementRef, parts: &mut Vec<String>) {
    for child in element.children() {
        if let Some(text) = child.value().as_text() {
            let t = text.trim();
            if !t.is_empty() {
                parts.push(t.to_string());
            }
        } else if let Some(child_el) = ElementRef::wrap(child) {
            let tag = child_el.value().name();

            // Skip stripped tags entirely
            if STRIP_TAGS.contains(&tag) {
                continue;
            }

            // Skip elements with non-content class names
            if let Some(classes) = child_el.value().attr("class") {
                let classes_lower = classes.to_lowercase();
                if STRIP_CLASSES.iter().any(|c| classes_lower.contains(c)) {
                    continue;
                }
            }

            collect_text_recursive(child_el, parts);
        }
    }
}

/// Score a content node by text density and paragraph count.
/// Higher score = more likely to be the main article body.
fn score_content_node(element: ElementRef) -> usize {
    let text = extract_clean_text(element);
    let text_len = text.len();

    if text_len < 50 {
        return 0;
    }

    // Count <p> tags as a signal of article prose
    let p_count = Selector::parse("p")
        .ok()
        .map(|sel| element.select(&sel).count())
        .unwrap_or(0);

    // Score = text length + bonus for paragraph density
    text_len + (p_count * 50)
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

/// Wrap raw plain text into an ArticleContent without HTML parsing.
/// For the plain-text input feature where users paste article text directly.
pub fn extract_from_text(text: &str, title: Option<&str>) -> ArticleContent {
    let body_text = collapse_whitespace(text.trim());
    let title = title
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| "Untitled".to_string());

    ArticleContent {
        title,
        body_text,
        meta_description: None,
        source_url: String::new(),
        paywalled: false,
        disclaimer: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_title_from_title_tag() {
        let html = Html::parse_document(
            "<html><head><title>Test Title</title></head><body></body></html>",
        );
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
            "<html><body><article><p>Article content here with enough text to pass the minimum scoring threshold for extraction.</p></article></body></html>",
        );
        let text = extract_body_text(&html);
        assert!(text.contains("Article content here"));
    }

    #[test]
    fn extract_body_text_from_main_tag() {
        let html = Html::parse_document(
            "<html><body><main><p>Main content with sufficient text length to meet the scoring threshold for content extraction.</p></main></body></html>",
        );
        let text = extract_body_text(&html);
        assert!(text.contains("Main content"));
    }

    #[test]
    fn extract_body_text_falls_back_to_body() {
        let html = Html::parse_document(
            "<html><body><div><p>Body fallback content with enough text to pass the scoring threshold for the content extraction algorithm.</p></div></body></html>",
        );
        let text = extract_body_text(&html);
        assert!(text.contains("Body fallback content"));
    }

    #[test]
    fn extract_body_text_collapses_whitespace() {
        let html = Html::parse_document(
            "<html><body><article><p>  lots   of   spaces\n\nand\nnewlines   in this article text that needs to be long enough  </p></article></body></html>",
        );
        let text = extract_body_text(&html);
        assert!(text.contains("lots of spaces and newlines"));
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
        assert_eq!(
            collapse_whitespace("no\nnewlines\there"),
            "no newlines here"
        );
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
        let html = r#"<html><head><title>Test</title></head><body><article><p>Content here with enough text to pass the minimum scoring threshold for content extraction in the new algorithm.</p></article></body></html>"#;
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
        let html = "<html><body><article><p>Some text that is long enough to pass the minimum scoring threshold for content extraction in the algorithm.</p></article></body></html>";
        let result = parse_html(html, "https://example.com").unwrap();
        assert_eq!(result.title, "Untitled");
    }

    #[test]
    fn scrape_error_display_messages() {
        assert!(
            ScrapeError::InvalidUrl("bad".to_string())
                .to_string()
                .contains("Invalid URL")
        );
        assert!(
            ScrapeError::Timeout("slow".to_string())
                .to_string()
                .contains("timed out")
        );
        assert!(
            ScrapeError::FetchFailed("nope".to_string())
                .to_string()
                .contains("Fetch failed")
        );
        assert!(
            ScrapeError::EmptyContent
                .to_string()
                .contains("no extractable text")
        );
        assert!(ScrapeError::NotFound.to_string().contains("404"));
        assert!(ScrapeError::Paywall.to_string().contains("paywall"));
        assert!(
            ScrapeError::NotHtml("application/pdf".to_string())
                .to_string()
                .contains("non-HTML")
                || ScrapeError::NotHtml("application/pdf".to_string())
                    .to_string()
                    .contains("Not an HTML")
        );
    }

    #[test]
    fn parse_html_detects_paywall_short_content() {
        // Content must be long enough (50+ chars) to pass scoring but short enough (<500 chars)
        // to trigger paywall detection with a single indicator
        let html = "<html><body><article><p>Subscribe to continue reading. Please sign up for a premium account to access this exclusive content and all future articles.</p></article></body></html>";
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
        let html = format!("<html><body><article><p>{long_text}</p></article></body></html>");
        let result = parse_html(&html, "https://example.com").unwrap();
        assert!(result.body_text.len() <= MAX_CONTENT_LENGTH);
    }

    #[test]
    fn invalid_url_rejected() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let cache: ArticleCache = std::sync::Arc::new(tokio::sync::RwLock::new(
                lru::LruCache::new(std::num::NonZeroUsize::new(10).unwrap()),
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
                lru::LruCache::new(std::num::NonZeroUsize::new(10).unwrap()),
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
                lru::LruCache::new(std::num::NonZeroUsize::new(10).unwrap()),
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
                lru::LruCache::new(std::num::NonZeroUsize::new(10).unwrap()),
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
                lru::LruCache::new(std::num::NonZeroUsize::new(10).unwrap()),
            ));

            // Pre-populate the cache
            let article = ArticleContent {
                title: "Cached Title".to_string(),
                body_text: "Cached body text".to_string(),
                meta_description: None,
                source_url: "https://example.com/cached".to_string(),
                paywalled: false,
                disclaimer: None,
            };
            {
                let mut cache_write = cache.write().await;
                cache_write.put("https://example.com/cached".to_string(), article.clone());
            }

            // Fetch should hit the cache (no network call)
            let result = scrape_article("https://example.com/cached", &cache).await;
            assert!(result.is_ok());
            let cached = result.unwrap();
            assert_eq!(cached.title, "Cached Title");
            assert_eq!(cached.body_text, "Cached body text");
        });
    }

    // --- Content node scoring tests ---

    #[test]
    fn scoring_prefers_article_body_over_nav() {
        let html = Html::parse_document(
            r#"
            <html><body>
                <nav><a>Home</a> <a>About</a> <a>Contact</a></nav>
                <article>
                    <p>This is the main article content with plenty of text to ensure it scores higher than navigation links.</p>
                    <p>Another paragraph with meaningful article content about political analysis.</p>
                </article>
            </body></html>
        "#,
        );
        let text = extract_body_text(&html);
        assert!(text.contains("main article content"));
        assert!(!text.contains("Home"));
    }

    #[test]
    fn scoring_prefers_high_paragraph_density() {
        let html = Html::parse_document(
            r#"
            <html><body>
                <div class="sidebar"><p>Short sidebar text here.</p></div>
                <article>
                    <p>First paragraph of the article with substantial content for analysis.</p>
                    <p>Second paragraph continuing the discussion of important political events.</p>
                    <p>Third paragraph wrapping up with a conclusion on the matter at hand.</p>
                </article>
            </body></html>
        "#,
        );
        let text = extract_body_text(&html);
        assert!(text.contains("First paragraph"));
        assert!(!text.contains("Short sidebar text"));
    }

    // --- Element stripping tests ---

    #[test]
    fn strips_nav_elements() {
        let html = Html::parse_document(
            r#"
            <html><body><article>
                <nav><a>Menu Item 1</a> <a>Menu Item 2</a></nav>
                <p>Actual article content that should be extracted and kept in the output.</p>
            </article></body></html>
        "#,
        );
        let text = extract_body_text(&html);
        assert!(text.contains("Actual article content"));
        assert!(!text.contains("Menu Item"));
    }

    #[test]
    fn strips_footer_elements() {
        let html = Html::parse_document(
            r#"
            <html><body><article>
                <p>Article content that should remain in the extracted text for analysis.</p>
                <footer>Copyright 2026 Example Corp. All rights reserved.</footer>
            </article></body></html>
        "#,
        );
        let text = extract_body_text(&html);
        assert!(text.contains("Article content"));
        assert!(!text.contains("Copyright 2026"));
    }

    #[test]
    fn strips_elements_with_ad_class() {
        let html = Html::parse_document(
            r#"
            <html><body><article>
                <p>Real news article content about current political events and policy decisions.</p>
                <div class="ad-container">Buy our product! Special offer inside!</div>
                <p>More article content continuing the story about political developments today.</p>
            </article></body></html>
        "#,
        );
        let text = extract_body_text(&html);
        assert!(text.contains("Real news article"));
        assert!(text.contains("More article content"));
        assert!(!text.contains("Buy our product"));
    }

    #[test]
    fn strips_sidebar_class_elements() {
        let html = Html::parse_document(
            r#"
            <html><body><article>
                <p>Main article text with enough content to be extracted as the primary body.</p>
                <div class="sidebar-widget">Related stories and sidebar content here.</div>
            </article></body></html>
        "#,
        );
        let text = extract_body_text(&html);
        assert!(text.contains("Main article text"));
        assert!(!text.contains("Related stories and sidebar"));
    }

    #[test]
    fn strips_script_and_style_tags() {
        let html = Html::parse_document(
            r#"
            <html><body><article>
                <script>var x = "should not appear in output";</script>
                <style>.article { color: red; }</style>
                <p>Only this article content should appear in the final extracted text output.</p>
            </article></body></html>
        "#,
        );
        let text = extract_body_text(&html);
        assert!(text.contains("Only this article content"));
        assert!(!text.contains("should not appear"));
        assert!(!text.contains("color: red"));
    }

    // --- og:title extraction tests ---

    #[test]
    fn extract_title_prefers_og_title() {
        let html = Html::parse_document(
            r#"
            <html>
            <head>
                <meta property="og:title" content="OG Title Here">
                <title>HTML Title Here</title>
            </head>
            <body><h1>H1 Title Here</h1></body>
            </html>
        "#,
        );
        assert_eq!(extract_title(&html), Some("OG Title Here".to_string()));
    }

    #[test]
    fn extract_title_falls_back_from_empty_og_title() {
        let html = Html::parse_document(
            r#"
            <html>
            <head>
                <meta property="og:title" content="">
                <title>Fallback Title</title>
            </head>
            <body></body>
            </html>
        "#,
        );
        assert_eq!(extract_title(&html), Some("Fallback Title".to_string()));
    }

    // --- extract_from_text tests ---

    #[test]
    fn extract_from_text_with_title() {
        let article = extract_from_text("Some article body text here.", Some("My Title"));
        assert_eq!(article.title, "My Title");
        assert_eq!(article.body_text, "Some article body text here.");
        assert_eq!(article.source_url, "");
        assert_eq!(article.meta_description, None);
    }

    #[test]
    fn extract_from_text_without_title() {
        let article = extract_from_text("Body text only.", None);
        assert_eq!(article.title, "Untitled");
        assert_eq!(article.body_text, "Body text only.");
    }

    #[test]
    fn extract_from_text_with_empty_title() {
        let article = extract_from_text("Body text.", Some("  "));
        assert_eq!(article.title, "Untitled");
    }

    #[test]
    fn extract_from_text_collapses_whitespace() {
        let article = extract_from_text("  lots   of   spaces\n\nand\nnewlines  ", Some("Title"));
        assert_eq!(article.body_text, "lots of spaces and newlines");
    }

    // --- Container selector tests ---

    #[test]
    fn extracts_from_itemprop_article_body() {
        let html = Html::parse_document(
            r#"
            <html><body>
                <div itemprop="articleBody">
                    <p>This content is marked with the articleBody itemprop and should be extracted.</p>
                </div>
            </body></html>
        "#,
        );
        let text = extract_body_text(&html);
        assert!(text.contains("articleBody itemprop"));
    }

    #[test]
    fn extracts_from_entry_content_class() {
        let html = Html::parse_document(
            r#"
            <html><body>
                <div class="entry-content">
                    <p>WordPress-style entry content that should be properly extracted from the page.</p>
                </div>
            </body></html>
        "#,
        );
        let text = extract_body_text(&html);
        assert!(text.contains("WordPress-style entry content"));
    }

    #[test]
    fn extracts_from_role_main() {
        let html = Html::parse_document(
            r#"
            <html><body>
                <div role="main">
                    <p>Content inside a role=main element should be found and extracted properly.</p>
                </div>
            </body></html>
        "#,
        );
        let text = extract_body_text(&html);
        assert!(text.contains("role=main element"));
    }

    // --- Source metadata extraction tests ---

    #[test]
    fn source_meta_known_publication() {
        let meta = extract_source_meta("https://www.nytimes.com/2026/01/15/politics/article.html");
        assert_eq!(meta.publication, "The New York Times");
        assert_eq!(meta.domain, "nytimes.com");
        assert_eq!(meta.known_bias.as_deref(), Some("center-left"));
        assert_eq!(meta.media_type.as_deref(), Some("mainstream"));
    }

    #[test]
    fn source_meta_wire_service() {
        let meta = extract_source_meta("https://reuters.com/world/some-story");
        assert_eq!(meta.publication, "Reuters");
        assert_eq!(meta.known_bias.as_deref(), Some("center"));
        assert_eq!(meta.media_type.as_deref(), Some("wire_service"));
    }

    #[test]
    fn source_meta_state_affiliated() {
        let meta = extract_source_meta("https://rt.com/news/article");
        assert_eq!(meta.publication, "RT (Russia Today)");
        assert_eq!(meta.media_type.as_deref(), Some("state_affiliated"));
    }

    #[test]
    fn source_meta_unknown_domain() {
        let meta = extract_source_meta("https://www.unknown-news-site.com/article");
        assert_eq!(meta.publication, "Unknown News Site");
        assert_eq!(meta.domain, "unknown-news-site.com");
        assert!(meta.known_bias.is_none());
        assert!(meta.media_type.is_none());
    }

    #[test]
    fn source_meta_strips_www() {
        let meta = extract_source_meta("https://www.foxnews.com/politics/story");
        assert_eq!(meta.domain, "foxnews.com");
        assert_eq!(meta.publication, "Fox News");
    }

    #[test]
    fn source_meta_handles_invalid_url() {
        let meta = extract_source_meta("not a url");
        assert_eq!(meta.domain, "");
        assert!(meta.known_bias.is_none());
    }

    #[test]
    fn source_meta_handles_empty_url() {
        let meta = extract_source_meta("");
        assert_eq!(meta.domain, "");
    }

    #[test]
    fn extract_domain_strips_www() {
        assert_eq!(
            extract_domain("https://www.example.com/path"),
            "example.com"
        );
    }

    #[test]
    fn extract_domain_no_www() {
        assert_eq!(extract_domain("https://example.com/path"), "example.com");
    }

    #[test]
    fn extract_domain_preserves_subdomain() {
        assert_eq!(
            extract_domain("https://news.bbc.co.uk/article"),
            "news.bbc.co.uk"
        );
    }

    #[test]
    fn domain_to_publication_name_simple() {
        assert_eq!(domain_to_publication_name("example.com"), "Example");
    }

    #[test]
    fn domain_to_publication_name_hyphenated() {
        assert_eq!(domain_to_publication_name("daily-beast.com"), "Daily Beast");
    }

    #[test]
    fn source_meta_serialization_roundtrip() {
        let meta = extract_source_meta("https://www.washingtonpost.com/article");
        let json = serde_json::to_string(&meta).unwrap();
        let deserialized: ScrapedSourceMeta = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.publication, "The Washington Post");
        assert_eq!(deserialized.known_bias.as_deref(), Some("center-left"));
    }

    // --- SSRF protection unit tests ---

    #[test]
    fn validate_hostname_blocks_localhost() {
        assert!(validate_hostname("localhost").is_err());
    }

    #[test]
    fn validate_hostname_blocks_dot_local() {
        assert!(validate_hostname("myserver.local").is_err());
    }

    #[test]
    fn validate_hostname_blocks_dot_internal() {
        assert!(validate_hostname("service.internal").is_err());
    }

    #[test]
    fn validate_hostname_blocks_dot_corp() {
        assert!(validate_hostname("intranet.corp").is_err());
    }

    #[test]
    fn validate_hostname_blocks_metadata_google() {
        assert!(validate_hostname("metadata.google.internal").is_err());
    }

    #[test]
    fn validate_hostname_allows_public_domains() {
        assert!(validate_hostname("archive.ph").is_ok());
        assert!(validate_hostname("example.com").is_ok());
        assert!(validate_hostname("nytimes.com").is_ok());
    }

    #[test]
    fn is_private_ip_detects_loopback_v4() {
        let ip: IpAddr = "127.0.0.1".parse().unwrap();
        assert!(is_private_ip(&ip));
    }

    #[test]
    fn is_private_ip_detects_10_range() {
        let ip: IpAddr = "10.0.0.1".parse().unwrap();
        assert!(is_private_ip(&ip));
    }

    #[test]
    fn is_private_ip_detects_192_168_range() {
        let ip: IpAddr = "192.168.1.1".parse().unwrap();
        assert!(is_private_ip(&ip));
    }

    #[test]
    fn is_private_ip_detects_172_16_range() {
        let ip: IpAddr = "172.16.0.1".parse().unwrap();
        assert!(is_private_ip(&ip));
    }

    #[test]
    fn is_private_ip_detects_cgnat() {
        let ip: IpAddr = "100.64.0.1".parse().unwrap();
        assert!(is_private_ip(&ip));
    }

    #[test]
    fn is_private_ip_detects_link_local() {
        let ip: IpAddr = "169.254.1.1".parse().unwrap();
        assert!(is_private_ip(&ip));
    }

    #[test]
    fn is_private_ip_detects_loopback_v6() {
        let ip: IpAddr = "::1".parse().unwrap();
        assert!(is_private_ip(&ip));
    }

    #[test]
    fn is_private_ip_detects_unspecified_v6() {
        let ip: IpAddr = "::".parse().unwrap();
        assert!(is_private_ip(&ip));
    }

    #[test]
    fn is_private_ip_allows_public_ip() {
        let ip: IpAddr = "8.8.8.8".parse().unwrap();
        assert!(!is_private_ip(&ip));
    }

    #[test]
    fn validate_url_target_blocks_localhost_url() {
        let result = validate_url_target("http://localhost/secret");
        assert!(result.is_err());
    }

    #[test]
    fn validate_url_target_blocks_private_ip_url() {
        let result = validate_url_target("http://127.0.0.1/admin");
        assert!(result.is_err());
    }

    #[test]
    fn validate_url_target_allows_archive_ph() {
        // archive.ph should pass hostname validation (DNS may fail in CI)
        let result = validate_url_target("https://archive.ph/newest/https://example.com");
        // Either succeeds or fails on DNS resolution (not hostname validation)
        match result {
            Ok(target) => assert!(!is_private_ip(&target.ip)),
            Err(ScrapeError::InvalidUrl(msg)) => {
                assert!(
                    msg.contains("resolve") || msg.contains("hostname"),
                    "Expected DNS resolution failure, got: {msg}"
                );
            }
            Err(other) => panic!("Unexpected error: {other}"),
        }
    }
}
