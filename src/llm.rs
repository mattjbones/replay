use crate::auth::AuthManager;
use crate::config::LlmConfig;
use crate::models::{Activity, Digest, Period};

/// Build the period label for the system prompt (e.g. "today", "this week").
fn period_label(period: &Period) -> String {
    match period {
        Period::Day(d) => format!("the day of {d}"),
        Period::Week(d) => format!("the week starting {d}"),
        Period::Month(d) => format!("the month starting {d}"),
    }
}

/// Format a single activity as a compact one-liner for the prompt.
fn format_activity(a: &Activity) -> String {
    let project = a
        .project
        .as_deref()
        .map(|p| format!(" ({p})"))
        .unwrap_or_default();
    let time = a.occurred_at.format("%H:%M");
    format!("[{}] {}: {}{} - {}", a.source, a.kind, a.title, project, time)
}

/// Call the Anthropic Messages API to generate a summary of the digest.
pub async fn generate_summary(
    config: &LlmConfig,
    digest: &Digest,
) -> Result<String, String> {
    let api_key = AuthManager::get_anthropic_key()?
        .ok_or_else(|| "Anthropic API key not configured or invalid".to_string())?;

    let label = period_label(&digest.period);
    let system_prompt = format!(
        "You are summarizing my work activity for {label}. \
         Be concise and highlight what matters: shipped work, key decisions, \
         and collaboration patterns. Group by theme, not by tool. Skip noise. \
         Use markdown formatting. Keep it under 300 words."
    );

    let formatted_activities: String = digest
        .activities
        .iter()
        .map(format_activity)
        .collect::<Vec<_>>()
        .join("\n");

    let user_content = format!("{system_prompt}\n\nActivities:\n{formatted_activities}");

    let body = serde_json::json!({
        "model": config.model,
        "max_tokens": 1024,
        "messages": [
            {
                "role": "user",
                "content": user_content
            }
        ]
    });

    let client = reqwest::Client::new();
    let response = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", &api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("failed to call Anthropic API: {e}"))?;

    let status = response.status();
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return Err("Anthropic API key not configured or invalid".to_string());
    }
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return Err("Rate limited".to_string());
    }
    if !status.is_success() {
        let body_text = response
            .text()
            .await
            .unwrap_or_else(|_| "<no body>".to_string());
        return Err(format!("Anthropic API error {status}: {body_text}"));
    }

    let json: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("failed to parse Anthropic response: {e}"))?;

    let text = json["content"]
        .as_array()
        .and_then(|arr| arr.first())
        .and_then(|block| block["text"].as_str())
        .ok_or_else(|| {
            format!(
                "unexpected Anthropic response structure: {}",
                serde_json::to_string_pretty(&json).unwrap_or_default()
            )
        })?;

    Ok(text.to_string())
}
