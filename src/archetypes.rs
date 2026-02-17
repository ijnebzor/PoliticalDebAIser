use anyhow::{Context, Result};
use serde::Deserialize;

use crate::models::{ArchetypeAnalysis, ArchetypeKind};

/// Result from synthesize_perspectives containing both the narrative and commonalities.
pub struct SynthesisResult {
    pub synthesis: String,
    pub commonalities: Vec<String>,
}

/// Archetype definition with its persona and system prompt.
struct Archetype {
    kind: ArchetypeKind,
    system_prompt: &'static str,
}

/// Returns the archetype definition for a given kind.
fn get_archetype(kind: &ArchetypeKind) -> Archetype {
    match kind {
        ArchetypeKind::Conservative => Archetype {
            kind: ArchetypeKind::Conservative,
            system_prompt: r#"You are a conservative political commentator in the tradition of Edmund Burke and William F. Buckley. Your analytical framework centers on:

Core lens: TRADITION, LIBERTY, AND ORGANIC ORDER. You see society as an inherited compact between generations — not a blank slate for social experiments.

- The Constitution as written is the supreme guardrail against government overreach
- Free markets are the engine of prosperity; regulation is a tax on human initiative
- The family, religious institutions, and local communities are the bedrock of civilization
- A strong military and secure borders are non-negotiable for national sovereignty
- Fiscal discipline today prevents debt slavery for future generations
- Cultural continuity matters — rapid social upheaval destabilizes society
- Personal responsibility, not government programs, is the path to human flourishing

When analyzing, ask: Does this expand or shrink individual liberty? Does it strengthen or erode the institutions that hold society together? Who pays — taxpayers now, or our grandchildren?"#,
        },
        ArchetypeKind::Democrat => Archetype {
            kind: ArchetypeKind::Democrat,
            system_prompt: r#"You are a progressive policy analyst in the tradition of the New Deal and Great Society. Your analytical framework centers on:

Core lens: EQUITY, INCLUSION, AND DEMOCRATIC GOVERNANCE. You see government as the essential counterbalance to the inequalities that markets and history produce.

- Democracy works best when everyone can participate — voting rights, representation, access
- Healthcare, education, and housing are rights, not privileges for the affluent
- Systemic racism, sexism, and discrimination require systemic solutions, not just goodwill
- Climate change is an existential crisis demanding bold government-led action now
- Workers deserve living wages, family leave, and collective bargaining power
- A progressive tax code where the wealthy pay their fair share funds shared prosperity
- America's strength comes from its diversity and its alliances, not from walls

When analyzing, ask: Who benefits and who is left behind? Does this advance or undermine equal opportunity? Are democratic institutions being strengthened or hollowed out?"#,
        },
        ArchetypeKind::Socialist => Archetype {
            kind: ArchetypeKind::Socialist,
            system_prompt: r#"You are a socialist political economist in the tradition of Marx, Rosa Luxemburg, and contemporary democratic socialists. Your analytical framework centers on:

Core lens: CLASS STRUGGLE AND MATERIAL CONDITIONS. Every political event is ultimately about who owns what, who works for whom, and how surplus value is distributed.

- Capitalism is not a natural order — it is a system designed to extract profit from labor
- The billionaire class exists because workers are not paid the full value of their labor
- "Bipartisan consensus" usually means both parties serving capital against working people
- Privatization of public goods (healthcare, water, education) is theft from the commons
- Imperialism abroad and austerity at home are two faces of the same coin
- Real democracy means democratic control of the economy, not just periodic voting
- International solidarity of workers transcends national borders drawn by the powerful

When analyzing, ask: Whose labor produces the wealth here? Which class benefits from this outcome? How does this maintain or challenge the ownership of productive resources?"#,
        },
        ArchetypeKind::Dictatorship => Archetype {
            kind: ArchetypeKind::Dictatorship,
            system_prompt: r#"You are a political strategist who analyzes events through the lens of authoritarian governance and state power, drawing on thinkers like Machiavelli, Carl Schmitt, and Lee Kuan Yew. Your analytical framework centers on:

Core lens: ORDER, SOVEREIGNTY, AND NATIONAL STRENGTH. A strong state with decisive leadership is the prerequisite for everything else — prosperity, security, and social harmony.

- Political stability is the foundation; without it, rights and freedoms are meaningless
- A unified national vision, enforced from the top, prevents the chaos of factionalism
- The state must direct strategic economic sectors — leaving everything to markets is weakness
- Information discipline and narrative control prevent social fragmentation
- A powerful military and intelligence apparatus deters external threats and internal subversion
- Individual dissent is acceptable; organized opposition that threatens state cohesion is not
- The measure of governance is results — GDP growth, infrastructure, security — not process

When analyzing, ask: Does this strengthen or weaken the state's capacity to act decisively? Does it promote national unity or dangerous fragmentation? Would a strong leader handle this differently?"#,
        },
        ArchetypeKind::Anarchist => Archetype {
            kind: ArchetypeKind::Anarchist,
            system_prompt: r#"You are an anarchist political thinker in the tradition of Kropotkin, Emma Goldman, and Murray Bookchin. Your analytical framework centers on:

Core lens: HIERARCHY IS THE PROBLEM. Every concentration of power — state, corporate, religious, patriarchal — must justify itself or be dismantled.

- The state is not a neutral arbiter; it is a monopoly on violence that serves the powerful
- Capitalism and the state are symbiotic — police exist to protect property, not people
- Electoral politics is a pressure valve that absorbs revolutionary energy into managed channels
- Mutual aid and community self-organization already solve problems the state claims only it can
- Borders, prisons, and armies are tools of control, not protection
- Real freedom means freedom from domination — by bosses, landlords, police, and bureaucrats
- Prefigurative politics: build the world you want now, don't wait for permission from above

When analyzing, ask: Who holds power here and over whom? Could communities solve this themselves without state or corporate intermediaries? What systems of domination does this reinforce or resist?"#,
        },
    }
}

/// Ollama API response structures.
#[derive(Deserialize)]
struct OllamaResponse {
    message: OllamaMessage,
}

#[derive(Deserialize)]
struct OllamaMessage {
    content: String,
}

/// Parsed analysis from the LLM's JSON response.
#[derive(Deserialize)]
struct ParsedAnalysis {
    summary: String,
    highlights: Vec<String>,
    alignment_score: f64,
}

/// Strip markdown code fences from LLM responses and trim whitespace.
fn extract_json(raw: &str) -> &str {
    let trimmed = raw.trim();
    let stripped = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .unwrap_or(trimmed);
    let stripped = stripped
        .strip_suffix("```")
        .unwrap_or(stripped);
    stripped.trim()
}

/// Returns true if the error is retryable (connection error or 5xx).
fn is_retryable(status: reqwest::StatusCode) -> bool {
    status.is_server_error()
}

/// Call the Ollama chat API with the given system prompt and user message.
/// Retries up to 2 times on connection errors or 5xx responses (500ms delay).
async fn call_ollama(system_prompt: &str, user_message: &str) -> Result<String> {
    let base_url =
        std::env::var("OLLAMA_URL").unwrap_or_else(|_| "http://localhost:11434".to_string());
    let model = std::env::var("OLLAMA_MODEL").unwrap_or_else(|_| "llama3.2".to_string());

    let client = reqwest::Client::new();
    let body = serde_json::json!({
        "model": model,
        "messages": [
            {
                "role": "system",
                "content": system_prompt
            },
            {
                "role": "user",
                "content": user_message
            }
        ],
        "stream": false
    });

    let mut last_err = None;

    for attempt in 0..3 {
        if attempt > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }

        let response = match client
            .post(format!("{base_url}/api/chat"))
            .json(&body)
            .send()
            .await
        {
            Ok(resp) => resp,
            Err(e) => {
                tracing::warn!("Ollama request failed (attempt {}): {e}", attempt + 1);
                last_err = Some(anyhow::anyhow!("Failed to send request to Ollama: {e}"));
                continue;
            }
        };

        if !response.status().is_success() {
            let status = response.status();
            let error_body = response.text().await.unwrap_or_default();
            if is_retryable(status) && attempt < 2 {
                tracing::warn!("Ollama returned {status} (attempt {}), retrying", attempt + 1);
                last_err = Some(anyhow::anyhow!("Ollama returned {status}: {error_body}"));
                continue;
            }
            anyhow::bail!("Ollama returned {status}: {error_body}");
        }

        let ollama_response: OllamaResponse = response
            .json()
            .await
            .context("Failed to parse Ollama response")?;

        return Ok(ollama_response.message.content);
    }

    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("Ollama request failed after retries")))
}

/// Generate a neutral, objective 2-3 sentence summary of an article.
pub async fn summarize_article(content: &str) -> Result<String> {
    let system_prompt = "You are a neutral, objective news summarizer. Summarize articles factually without any political slant, opinion, or editorializing. Be concise and accurate.";

    let user_message = format!(
        "Summarize the following article in 2-3 sentences. Be factual and neutral. Do not include any political commentary or opinion. Respond with plain text only, no JSON or markdown.\n\nArticle:\n{content}"
    );

    call_ollama(system_prompt, &user_message).await
}

/// Analyze an article from the perspective of a single archetype.
pub async fn analyze_article(
    content: &str,
    archetype: &ArchetypeKind,
) -> Result<ArchetypeAnalysis> {
    let arch = get_archetype(archetype);

    let user_message = format!(
        r#"Analyze the following article from your political perspective.

Respond with ONLY valid JSON in this exact format (no markdown, no code fences):
{{
  "summary": "A 2-3 sentence summary of the article from your perspective",
  "highlights": ["Key point 1", "Key point 2", "Key point 3"],
  "alignment_score": 0.0
}}

The alignment_score should be between 0.0 and 1.0, where:
- 0.0 = completely opposed to your political values
- 0.5 = neutral or mixed
- 1.0 = perfectly aligned with your political values

Include 3-5 highlights as key points from your perspective.

Article:
{content}"#
    );

    let response_text = call_ollama(arch.system_prompt, &user_message).await?;
    let json_text = extract_json(&response_text);

    let parsed: ParsedAnalysis = serde_json::from_str(json_text).with_context(|| {
        format!(
            "Failed to parse {} analysis response as JSON: {response_text}",
            arch.kind.label()
        )
    })?;

    let score = parsed.alignment_score.clamp(0.0, 1.0);

    Ok(ArchetypeAnalysis {
        archetype: arch.kind,
        summary: parsed.summary,
        highlights: parsed.highlights,
        alignment_score: score,
    })
}

/// Run analysis across all 5 political archetypes concurrently.
/// Returns successful analyses even if some archetypes fail.
pub async fn analyze_all_archetypes(content: &str) -> Result<Vec<ArchetypeAnalysis>> {
    let content = content.to_string();
    let mut handles = Vec::with_capacity(5);

    for kind in ArchetypeKind::all() {
        let content = content.clone();
        let kind = kind.clone();
        handles.push(tokio::spawn(async move {
            analyze_article(&content, &kind).await
        }));
    }

    let mut analyses = Vec::with_capacity(5);
    for handle in handles {
        match handle.await {
            Ok(Ok(analysis)) => analyses.push(analysis),
            Ok(Err(e)) => tracing::error!("Archetype analysis failed: {e}"),
            Err(e) => tracing::error!("Archetype analysis task panicked: {e}"),
        }
    }

    if analyses.is_empty() {
        anyhow::bail!("All archetype analyses failed");
    }

    Ok(analyses)
}

/// Parsed synthesis response from the LLM.
#[derive(Deserialize)]
struct ParsedSynthesis {
    synthesis: String,
    commonalities: Vec<String>,
}

/// Synthesize a balanced summary from all archetype perspectives.
/// Returns both a narrative synthesis and a list of cross-spectrum commonalities.
pub async fn synthesize_perspectives(analyses: &[ArchetypeAnalysis]) -> Result<SynthesisResult> {
    let perspectives: Vec<String> = analyses
        .iter()
        .map(|a| {
            format!(
                "**{} perspective** (alignment: {:.0}%):\n{}\nKey points: {}",
                a.archetype.label(),
                a.alignment_score * 100.0,
                a.summary,
                a.highlights.join("; ")
            )
        })
        .collect();

    let system_prompt = r#"You are a balanced, non-partisan political analyst. Your role is to synthesize multiple political perspectives into a fair, nuanced overview. Do not favor any viewpoint. Present the key areas of agreement and disagreement across the political spectrum."#;

    let user_message = format!(
        r#"Below are analyses of the same article from 5 different political perspectives. Produce a synthesis with two parts:

1. A balanced 2-3 paragraph narrative summary that identifies where perspectives agree and disagree, highlights tensions and trade-offs, and helps the reader understand the full political landscape.

2. A list of 3-5 "commonalities" — specific points where at least 3 of the 5 perspectives agree, even if for different reasons.

Respond with ONLY valid JSON in this exact format (no markdown, no code fences):
{{
  "synthesis": "Your 2-3 paragraph narrative here...",
  "commonalities": [
    "Point where multiple perspectives agree",
    "Another shared concern or observation",
    "A third area of cross-spectrum agreement"
  ]
}}

{}"#,
        perspectives.join("\n\n")
    );

    let response_text = call_ollama(system_prompt, &user_message).await?;
    let json_text = extract_json(&response_text);

    let parsed: ParsedSynthesis = serde_json::from_str(json_text).with_context(|| {
        format!("Failed to parse synthesis response as JSON: {response_text}")
    })?;

    Ok(SynthesisResult {
        synthesis: parsed.synthesis,
        commonalities: parsed.commonalities,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_json_strips_json_code_fence() {
        let input = "```json\n{\"summary\": \"test\"}\n```";
        assert_eq!(extract_json(input), r#"{"summary": "test"}"#);
    }

    #[test]
    fn extract_json_strips_plain_code_fence() {
        let input = "```\n{\"summary\": \"test\"}\n```";
        assert_eq!(extract_json(input), r#"{"summary": "test"}"#);
    }

    #[test]
    fn extract_json_passes_through_raw_json() {
        let input = r#"{"summary": "test", "highlights": [], "alignment_score": 0.5}"#;
        assert_eq!(extract_json(input), input);
    }

    #[test]
    fn extract_json_trims_whitespace() {
        let input = "  \n  {\"key\": \"value\"}  \n  ";
        assert_eq!(extract_json(input), r#"{"key": "value"}"#);
    }

    #[test]
    fn extract_json_handles_fence_with_trailing_whitespace() {
        let input = "```json\n{\"a\": 1}\n```\n";
        assert_eq!(extract_json(input), r#"{"a": 1}"#);
    }

    #[test]
    fn is_retryable_for_server_errors() {
        assert!(is_retryable(reqwest::StatusCode::INTERNAL_SERVER_ERROR));
        assert!(is_retryable(reqwest::StatusCode::BAD_GATEWAY));
        assert!(is_retryable(reqwest::StatusCode::SERVICE_UNAVAILABLE));
    }

    #[test]
    fn is_retryable_false_for_client_errors() {
        assert!(!is_retryable(reqwest::StatusCode::BAD_REQUEST));
        assert!(!is_retryable(reqwest::StatusCode::NOT_FOUND));
        assert!(!is_retryable(reqwest::StatusCode::UNAUTHORIZED));
    }

    #[test]
    fn is_retryable_false_for_success() {
        assert!(!is_retryable(reqwest::StatusCode::OK));
    }
}
