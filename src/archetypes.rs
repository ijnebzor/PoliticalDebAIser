use anyhow::{Context, Result};
use serde::Deserialize;

use crate::models::{
    AnalysisResult, Axes2D, DebiasedSummary, FactCheck, FactCheckAssessment, PersonaId,
    PersonaOutput,
};

/// Persona definition with its system prompt for LLM analysis.
struct Persona {
    id: PersonaId,
    system_prompt: &'static str,
}

/// Returns the persona definition for a given ID.
fn get_persona(id: &PersonaId) -> Persona {
    match id {
        PersonaId::ProgressiveActivist => Persona {
            id: PersonaId::ProgressiveActivist,
            system_prompt: r#"You are a progressive activist and civil rights advocate. Your analytical framework centers on:

Core lens: CIVIL RIGHTS, DISPROPORTIONATE IMPACTS, AND SPEECH CHILLING. Every policy must be evaluated by how it affects the most marginalized communities.

- Surveillance, policing, and regulation disproportionately harm communities of color, immigrants, and dissidents
- "Public safety" rhetoric often masks the expansion of state power over vulnerable populations
- Free speech protections exist precisely for unpopular, dissident, and minority viewpoints — chilling effects matter
- Systemic racism and structural inequality are embedded in institutions; reforms must address root causes
- Corporate power and government power intersect to suppress grassroots organizing
- Environmental justice is inseparable from racial and economic justice
- International solidarity connects domestic civil rights struggles to global human rights

When analyzing, ask: Who bears the disproportionate cost? Does this chill speech or organizing? Are marginalized communities disproportionately affected? What power structures does this reinforce?"#,
        },
        PersonaId::LiberalSocialDemocrat => Persona {
            id: PersonaId::LiberalSocialDemocrat,
            system_prompt: r#"You are a liberal social democrat policy analyst in the tradition of the Nordic model and EU fundamental rights framework. Your analytical framework centers on:

Core lens: PROPORTIONALITY, SAFEGUARDS, AND DATA MINIMISATION. Government action can be legitimate if it is targeted, transparent, and bounded.

- Democracy requires both security and liberty — the question is always proportionality
- Warrants, judicial oversight, and independent audits are non-negotiable safeguards
- Data minimisation: collect only what is necessary, retain it only as long as needed
- Universal public services (healthcare, education, housing) reduce the conditions that breed insecurity
- Evidence-based policy with regular review and sunset clauses prevents institutional overreach
- Workers deserve living wages, family leave, and collective bargaining power
- Diplomacy and multilateral frameworks are preferable to unilateral action

When analyzing, ask: Is this proportionate to the threat? Are safeguards robust and independently enforced? Could a less intrusive measure achieve the same goal? Is there a sunset clause?"#,
        },
        PersonaId::CentristTechnocrat => Persona {
            id: PersonaId::CentristTechnocrat,
            system_prompt: r#"You are a centrist technocrat and policy wonk focused on evidence-based governance. Your analytical framework centers on:

Core lens: KPIs, COST-BENEFIT, SUNSET CLAUSES, AND MEASURABLE OUTCOMES. Good policy is policy that works, measured by data, not ideology.

- Every policy should have clear KPIs, success metrics, and evaluation timelines
- Cost-benefit analysis must include externalities, opportunity costs, and second-order effects
- Pilot programs and phased rollouts reduce risk and generate evidence before full deployment
- Sunset clauses force periodic re-evaluation and prevent institutional inertia
- Error rates, false positives, and unintended consequences must be transparently reported
- Long-term fiscal sustainability is non-negotiable
- Both over-regulation and under-regulation are costly failures

When analyzing, ask: What is the evidence base? What are the measurable KPIs? Has a cost-benefit analysis been done? Are there sunset clauses? What does the pilot data show?"#,
        },
        PersonaId::LibertarianCivil => Persona {
            id: PersonaId::LibertarianCivil,
            system_prompt: r#"You are a libertarian civil liberties advocate. Your analytical framework centers on:

Core lens: PRIVACY AS FUNDAMENTAL LIBERTY, MISSION CREEP, AND POWER ASYMMETRY. The default should be freedom; every restriction requires extraordinary justification.

- Privacy is not about having something to hide — it is the right to be left alone
- Government powers, once granted, expand inexorably (mission creep is not a bug, it is a feature of state power)
- The asymmetry between individual and state power means even "reasonable" regulations tilt the balance dangerously
- Consent, not compliance, should be the basis of data collection and surveillance
- Free markets and voluntary association solve most coordination problems better than coercion
- Due process and presumption of innocence must never be eroded, even for security
- The burden of proof must always be on the entity seeking to restrict liberty

When analyzing, ask: Does this expand state power over individuals? Is there genuine consent? What is the mission creep risk? Could this be achieved without coercion? Who holds the power asymmetry?"#,
        },
        PersonaId::ConservativeFiscal => Persona {
            id: PersonaId::ConservativeFiscal,
            system_prompt: r#"You are a fiscal conservative focused on cost discipline and law-and-order. Your analytical framework centers on:

Core lens: COST DISCIPLINE, LAW AND ORDER, AND PENALTIES FOR MISUSE. Government must be efficient, laws must be enforced, and abuse must be punished.

- Fiscal responsibility: every program must justify its cost to taxpayers with measurable returns
- Law and order is the foundation of a functioning society — without enforcement, rights are meaningless
- Government programs tend to expand and entrench; strict oversight prevents waste and bureaucratic bloat
- Penalties for misuse of power must be severe and consistently enforced to deter abuse
- Personal responsibility, not government programs, is the path to human flourishing
- Regulatory burden should be minimized — excessive regulation stifles growth and innovation
- A strong military and secure borders are non-negotiable for national sovereignty

When analyzing, ask: What does this cost? Is the spending justified by measurable outcomes? Are there penalties for misuse and abuse? Does this expand government beyond its core mandate?"#,
        },
        PersonaId::NationalSecurityHawk => Persona {
            id: PersonaId::NationalSecurityHawk,
            system_prompt: r#"You are a national security hawk and defense policy analyst. Your analytical framework centers on:

Core lens: THREAT LANDSCAPE, INTELLIGENCE GAPS, AND RAPID RESPONSE. Security is the precondition for all other rights and freedoms.

- The threat landscape is constantly evolving — adversaries exploit every vulnerability
- Intelligence gaps get people killed; tools that close gaps save lives, even with trade-offs
- Rapid response capability is essential — bureaucratic delays in the face of threats are unacceptable
- Operational secrecy is sometimes necessary; full transparency can compromise sources and methods
- Internal compliance and inspector general oversight provide accountability without public exposure
- Deterrence requires credible capability and the will to use it
- Allied cooperation and intelligence sharing multiply national security capacity

When analyzing, ask: What threats does this address? What intelligence gaps does it close? Is the response capability fast enough? Are internal compliance mechanisms adequate? What happens if we don't act?"#,
        },
        PersonaId::EnvironmentalistGreen => Persona {
            id: PersonaId::EnvironmentalistGreen,
            system_prompt: r#"You are an environmentalist green policy analyst. Your analytical framework centers on:

Core lens: ENERGY FOOTPRINT, ACTIVISM CHILL, AND SUPPLY-CHAIN ACCOUNTABILITY. Every policy has environmental dimensions that are usually ignored.

- Climate change is the existential crisis of our time — all policy must be evaluated through this lens
- Energy footprint matters: data centers, surveillance infrastructure, and digital systems consume enormous energy
- Environmental activism is increasingly criminalized and surveilled — any expansion of state power chills green organizing
- Supply chains for technology (rare earth mining, e-waste) have devastating environmental and human rights impacts
- Precautionary principle: when environmental harm is possible, the burden of proof falls on the proponent
- Environmental justice connects ecological destruction to racism, colonialism, and economic exploitation
- Long-term ecological sustainability must outweigh short-term economic or security gains

When analyzing, ask: What is the energy and resource footprint? Does this chill environmental activism? Are supply-chain impacts considered? Does this prioritize short-term gains over long-term sustainability?"#,
        },
        PersonaId::PopulistAntiElite => Persona {
            id: PersonaId::PopulistAntiElite,
            system_prompt: r#"You are a populist, anti-establishment analyst suspicious of elites, big tech, and captured institutions. Your analytical framework centers on:

Core lens: ELITE CAPTURE, CORPORATE INFLUENCE, AND EQUAL APPLICATION. Rules that apply to ordinary people must apply equally to the powerful.

- Elites (political, corporate, tech) write rules that benefit themselves while burdening ordinary citizens
- "Expert consensus" is often manufactured by think tanks and lobbyists serving corporate interests
- Big Tech companies have more power than many governments but face less accountability
- Any new government power will be captured by insiders and used against outsiders
- Equal application of the law is the minimum standard — if elites are exempt, the law is illegitimate
- Ordinary people's concerns about crime, economic insecurity, and institutional failure are legitimate
- Transparency and accountability must start at the top, not at the bottom

When analyzing, ask: Who really benefits? Are elites exempt from this? Is there corporate capture or revolving-door influence? Would this apply equally to a senator and a truck driver?"#,
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

/// Parsed persona analysis from the LLM's JSON response.
#[derive(Deserialize)]
struct ParsedPersonaOutput {
    stance_score: f64,
    confidence: f64,
    summary: String,
    key_claims: Vec<String>,
    fact_checks: Vec<ParsedFactCheck>,
    #[serde(default)]
    caveats: Vec<String>,
    #[serde(default)]
    axes: Option<ParsedAxes>,
}

#[derive(Deserialize)]
struct ParsedFactCheck {
    claim: String,
    assessment: String,
    rationale: String,
}

#[derive(Deserialize)]
struct ParsedAxes {
    economic: f64,
    social: f64,
}

/// Parsed debiased synthesis from the LLM.
/// Note: spectrum_score is calculated server-side (confidence-weighted mean),
/// NOT generated by the LLM. The LLM only provides spectrum_explain.
#[derive(Deserialize)]
struct ParsedDebiased {
    consensus_points: Vec<String>,
    disagreements: Vec<String>,
    likely_bias_drivers: Vec<String>,
    truth_seeking_summary: String,
    spectrum_explain: String,
}

/// Calculate the confidence-weighted mean of persona stance scores.
fn weighted_spectrum_score(personas: &[PersonaOutput]) -> f64 {
    let weight_sum: f64 = personas.iter().map(|p| p.confidence).sum();
    if weight_sum > 0.0 {
        let weighted_sum: f64 = personas
            .iter()
            .map(|p| p.stance_score * p.confidence)
            .sum();
        let raw = weighted_sum / weight_sum;
        (raw * 100.0).round() / 100.0 // round to 2 decimal places
    } else {
        0.0
    }
}

/// Strip markdown code fences from LLM responses and trim whitespace.
fn extract_json(raw: &str) -> &str {
    let trimmed = raw.trim();
    let stripped = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .unwrap_or(trimmed);
    let stripped = stripped.strip_suffix("```").unwrap_or(stripped);
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

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .context("Failed to build HTTP client for Ollama")?;
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

/// Analyze an article from the perspective of a single persona.
pub async fn analyze_persona(content: &str, persona_id: &PersonaId) -> Result<PersonaOutput> {
    let persona = get_persona(persona_id);

    let user_message = format!(
        r#"Analyze the following article from your political perspective.

Respond with ONLY valid JSON in this exact format (no markdown, no code fences):
{{
  "stance_score": 0.0,
  "confidence": 0.8,
  "summary": "A 2-4 sentence summary from your perspective",
  "key_claims": ["Claim 1", "Claim 2", "Claim 3"],
  "fact_checks": [
    {{
      "claim": "A specific factual claim in the article",
      "assessment": "supported",
      "rationale": "Brief explanation of why"
    }}
  ],
  "caveats": ["Any blind spots or limitations of your perspective on this topic"],
  "axes": {{
    "economic": 0.0,
    "social": 0.0
  }}
}}

Field definitions:
- stance_score: Liberty-Order axis, -3.0 (maximum liberty) to +3.0 (maximum order)
- confidence: 0.0 to 1.0, how confident you are in your analysis
- key_claims: 2-5 key claims or observations from your perspective
- fact_checks: 1-3 fact checks of claims made in the article. Assessment must be one of: "supported", "contested", "unsupported", "unclear"
- caveats: 1-2 honest admissions about blind spots in your perspective
- axes.economic: -3 (more government intervention) to +3 (more free market)
- axes.social: -3 (more libertarian/permissive) to +3 (more authoritarian/restrictive)

Article:
{content}"#
    );

    let response_text = call_ollama(persona.system_prompt, &user_message).await?;
    let json_text = extract_json(&response_text);

    let parsed: ParsedPersonaOutput = serde_json::from_str(json_text).with_context(|| {
        format!(
            "Failed to parse {} analysis response as JSON: {response_text}",
            persona.id.title()
        )
    })?;

    let fact_checks: Vec<FactCheck> = parsed
        .fact_checks
        .into_iter()
        .map(|fc| FactCheck {
            claim: fc.claim,
            assessment: match fc.assessment.to_lowercase().as_str() {
                "supported" => FactCheckAssessment::Supported,
                "contested" => FactCheckAssessment::Contested,
                "unsupported" => FactCheckAssessment::Unsupported,
                _ => FactCheckAssessment::Unclear,
            },
            rationale: fc.rationale,
        })
        .collect();

    let axes = parsed.axes.map(|a| Axes2D {
        economic: a.economic.clamp(-3.0, 3.0),
        social: a.social.clamp(-3.0, 3.0),
    });

    Ok(PersonaOutput {
        id: persona.id,
        title: persona_id.title().to_string(),
        stance_score: parsed.stance_score.clamp(-3.0, 3.0),
        confidence: parsed.confidence.clamp(0.0, 1.0),
        summary: parsed.summary,
        key_claims: parsed.key_claims,
        fact_checks,
        caveats: parsed.caveats,
        axes,
    })
}

/// Run analysis across all 8 political personas concurrently.
/// Returns successful analyses even if some personas fail.
pub async fn analyze_all_personas(content: &str) -> Result<Vec<PersonaOutput>> {
    let content = content.to_string();
    let mut handles = Vec::with_capacity(8);

    for persona_id in PersonaId::all() {
        let content = content.clone();
        let persona_id = persona_id.clone();
        handles.push(tokio::spawn(async move {
            analyze_persona(&content, &persona_id).await
        }));
    }

    let mut outputs = Vec::with_capacity(8);
    for handle in handles {
        match handle.await {
            Ok(Ok(output)) => outputs.push(output),
            Ok(Err(e)) => tracing::error!("Persona analysis failed: {e}"),
            Err(e) => tracing::error!("Persona analysis task panicked: {e}"),
        }
    }

    if outputs.is_empty() {
        anyhow::bail!("All persona analyses failed");
    }

    Ok(outputs)
}

/// Synthesize a debiased summary from all persona perspectives.
/// The spectrum_score is calculated as a confidence-weighted mean of stance scores,
/// NOT generated by the LLM.
pub async fn synthesize_debiased(personas: &[PersonaOutput]) -> Result<DebiasedSummary> {
    let spectrum_score = weighted_spectrum_score(personas);

    let perspectives: Vec<String> = personas
        .iter()
        .map(|p| {
            format!(
                "**{} perspective** (stance: {:.1}, confidence: {:.0}%):\n{}\nKey claims: {}\nCaveats: {}",
                p.title,
                p.stance_score,
                p.confidence * 100.0,
                p.summary,
                p.key_claims.join("; "),
                p.caveats.join("; ")
            )
        })
        .collect();

    let system_prompt = r#"You are a balanced, non-partisan political analyst. Your role is to synthesize multiple political perspectives into a fair, nuanced, debiased overview. Do not favor any viewpoint. Identify where perspectives agree and disagree, and seek the truth that cuts across partisan lines."#;

    let user_message = format!(
        r#"Below are analyses of the same article from 8 different political perspectives. The calculated spectrum placement (confidence-weighted mean of stance scores) is {spectrum_score:.2} on a -3 (Liberty) to +3 (Order) axis.

Produce a debiased synthesis. Respond with ONLY valid JSON in this exact format (no markdown, no code fences):
{{
  "consensus_points": ["Point where multiple perspectives agree", "Another shared observation"],
  "disagreements": ["Key area of disagreement between perspectives"],
  "likely_bias_drivers": ["Factor that may be driving biased framing in the original article"],
  "truth_seeking_summary": "A 2-3 paragraph balanced summary that seeks truth across perspectives...",
  "spectrum_explain": "Brief explanation of why the article lands at {spectrum_score:.2} on the Liberty-Order spectrum"
}}

Field definitions:
- consensus_points: 3-5 points where at least half the perspectives agree
- disagreements: 2-4 key areas where perspectives diverge
- likely_bias_drivers: 1-3 factors that may bias the original article's framing
- truth_seeking_summary: A balanced 2-3 paragraph narrative summary
- spectrum_explain: Brief explanation of the spectrum placement (the score {spectrum_score:.2} is already calculated)

{perspectives}"#,
        perspectives = perspectives.join("\n\n")
    );

    let response_text = call_ollama(system_prompt, &user_message).await?;
    let json_text = extract_json(&response_text);

    let parsed: ParsedDebiased = serde_json::from_str(json_text).with_context(|| {
        format!("Failed to parse debiased synthesis response as JSON: {response_text}")
    })?;

    Ok(DebiasedSummary {
        consensus_points: parsed.consensus_points,
        disagreements: parsed.disagreements,
        likely_bias_drivers: parsed.likely_bias_drivers,
        truth_seeking_summary: parsed.truth_seeking_summary,
        spectrum_score,
        spectrum_explain: parsed.spectrum_explain,
    })
}

/// Full analysis pipeline: analyze with all personas, then debias.
/// Returns a complete AnalysisResult ready for the client.
pub async fn analyze_full(
    content: &str,
    title: &str,
    source_url: Option<&str>,
) -> Result<AnalysisResult> {
    let personas = analyze_all_personas(content).await?;

    let debiaser = synthesize_debiased(&personas).await.unwrap_or_else(|e| {
        tracing::warn!("Debiased summary generation failed, using fallback: {e}");
        let (weighted_sum, weight_sum) = personas.iter().fold((0.0_f64, 0.0_f64), |(ws, wt), p| {
            (ws + p.stance_score * p.confidence, wt + p.confidence)
        });
        let spectrum_score = if weight_sum > 0.0 {
            weighted_sum / weight_sum
        } else {
            0.0
        };
        DebiasedSummary {
            consensus_points: vec![],
            disagreements: vec![],
            likely_bias_drivers: vec![],
            truth_seeking_summary: "Debiased summary could not be generated.".to_string(),
            spectrum_score,
            spectrum_explain: "Fallback: simple weighted mean of persona stance scores.".to_string(),
        }
    });

    Ok(AnalysisResult {
        title: title.to_string(),
        source_url: source_url.map(|s| s.to_string()),
        personas,
        debiaser,
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

    // --- Persona prompt tests ---

    #[test]
    fn all_personas_have_prompts() {
        for persona_id in PersonaId::all() {
            let persona = get_persona(persona_id);
            assert!(
                !persona.system_prompt.is_empty(),
                "{:?} has empty system prompt",
                persona_id
            );
            assert!(
                persona.system_prompt.len() > 200,
                "{:?} system prompt too short ({})",
                persona_id,
                persona.system_prompt.len()
            );
        }
    }

    #[test]
    fn persona_prompts_are_unique() {
        let prompts: Vec<&str> = PersonaId::all()
            .iter()
            .map(|id| get_persona(id).system_prompt)
            .collect();
        let mut deduped = prompts.clone();
        deduped.sort();
        deduped.dedup();
        assert_eq!(
            prompts.len(),
            deduped.len(),
            "Duplicate persona prompts found"
        );
    }

    #[test]
    fn persona_ids_match_definitions() {
        for persona_id in PersonaId::all() {
            let persona = get_persona(persona_id);
            assert_eq!(
                persona.id, *persona_id,
                "Persona ID mismatch for {:?}",
                persona_id
            );
        }
    }

    // --- Parsed struct deserialization tests ---

    #[test]
    fn parsed_persona_output_deserializes() {
        let json = r#"{
            "stance_score": -1.5,
            "confidence": 0.8,
            "summary": "Test summary.",
            "key_claims": ["Claim 1"],
            "fact_checks": [{"claim": "X", "assessment": "supported", "rationale": "Because Y"}],
            "caveats": ["May miss Z"],
            "axes": {"economic": -0.5, "social": 1.2}
        }"#;
        let parsed: ParsedPersonaOutput = serde_json::from_str(json).unwrap();
        assert!((parsed.stance_score - (-1.5)).abs() < f64::EPSILON);
        assert!((parsed.confidence - 0.8).abs() < f64::EPSILON);
        assert_eq!(parsed.key_claims.len(), 1);
        assert_eq!(parsed.fact_checks.len(), 1);
        assert_eq!(parsed.caveats.len(), 1);
        assert!(parsed.axes.is_some());
    }

    #[test]
    fn parsed_persona_output_without_optional_fields() {
        let json = r#"{
            "stance_score": 2.0,
            "confidence": 0.6,
            "summary": "Security first.",
            "key_claims": [],
            "fact_checks": []
        }"#;
        let parsed: ParsedPersonaOutput = serde_json::from_str(json).unwrap();
        assert!(parsed.axes.is_none());
        assert!(parsed.caveats.is_empty());
    }

    #[test]
    fn parsed_debiased_deserializes() {
        let json = r#"{
            "consensus_points": ["Point A"],
            "disagreements": ["Disagree B"],
            "likely_bias_drivers": ["Bias C"],
            "truth_seeking_summary": "Balanced view.",
            "spectrum_explain": "Leans liberty."
        }"#;
        let parsed: ParsedDebiased = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.consensus_points.len(), 1);
        assert_eq!(parsed.disagreements.len(), 1);
        assert_eq!(parsed.likely_bias_drivers.len(), 1);
        assert_eq!(parsed.spectrum_explain, "Leans liberty.");
    }

    // --- Spectrum score calculation tests ---

    fn make_persona(id: PersonaId, stance: f64, confidence: f64) -> PersonaOutput {
        PersonaOutput {
            id,
            title: "Test".to_string(),
            stance_score: stance,
            confidence,
            summary: String::new(),
            key_claims: vec![],
            fact_checks: vec![],
            caveats: vec![],
            axes: None,
        }
    }

    #[test]
    fn weighted_spectrum_score_symmetric() {
        let personas = vec![
            make_persona(PersonaId::ProgressiveActivist, -2.0, 0.5),
            make_persona(PersonaId::NationalSecurityHawk, 2.0, 0.5),
        ];
        // (-2.0 * 0.5 + 2.0 * 0.5) / (0.5 + 0.5) = 0.0
        assert!((weighted_spectrum_score(&personas) - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn weighted_spectrum_score_asymmetric_confidence() {
        let personas = vec![
            make_persona(PersonaId::ProgressiveActivist, -2.0, 0.8),
            make_persona(PersonaId::NationalSecurityHawk, 2.0, 0.2),
        ];
        // (-2.0 * 0.8 + 2.0 * 0.2) / (0.8 + 0.2) = (-1.6 + 0.4) / 1.0 = -1.2
        assert!((weighted_spectrum_score(&personas) - (-1.2)).abs() < f64::EPSILON);
    }

    #[test]
    fn weighted_spectrum_score_single_persona() {
        let personas = vec![
            make_persona(PersonaId::CentristTechnocrat, 0.1, 0.9),
        ];
        assert!((weighted_spectrum_score(&personas) - 0.1).abs() < 0.01);
    }

    #[test]
    fn weighted_spectrum_score_zero_confidence_returns_zero() {
        let personas = vec![
            make_persona(PersonaId::ProgressiveActivist, -3.0, 0.0),
            make_persona(PersonaId::NationalSecurityHawk, 3.0, 0.0),
        ];
        assert!((weighted_spectrum_score(&personas) - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn weighted_spectrum_score_empty_returns_zero() {
        assert!((weighted_spectrum_score(&[]) - 0.0).abs() < f64::EPSILON);
    }

    // --- Fact check assessment parsing ---

    #[test]
    fn fact_check_assessment_from_llm_strings() {
        // Test the inline match in analyze_persona
        assert_eq!(
            match "supported".to_lowercase().as_str() {
                "supported" => FactCheckAssessment::Supported,
                "contested" => FactCheckAssessment::Contested,
                "unsupported" => FactCheckAssessment::Unsupported,
                _ => FactCheckAssessment::Unclear,
            },
            FactCheckAssessment::Supported
        );
        assert_eq!(
            match "Contested".to_lowercase().as_str() {
                "supported" => FactCheckAssessment::Supported,
                "contested" => FactCheckAssessment::Contested,
                "unsupported" => FactCheckAssessment::Unsupported,
                _ => FactCheckAssessment::Unclear,
            },
            FactCheckAssessment::Contested
        );
        assert_eq!(
            match "UNSUPPORTED".to_lowercase().as_str() {
                "supported" => FactCheckAssessment::Supported,
                "contested" => FactCheckAssessment::Contested,
                "unsupported" => FactCheckAssessment::Unsupported,
                _ => FactCheckAssessment::Unclear,
            },
            FactCheckAssessment::Unsupported
        );
        assert_eq!(
            match "unknown".to_lowercase().as_str() {
                "supported" => FactCheckAssessment::Supported,
                "contested" => FactCheckAssessment::Contested,
                "unsupported" => FactCheckAssessment::Unsupported,
                _ => FactCheckAssessment::Unclear,
            },
            FactCheckAssessment::Unclear
        );
    }
}
