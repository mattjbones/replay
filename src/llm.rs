use crate::config::LlmConfig;
use crate::models::{Activity, Digest, Period};

/// Build the period label for the prompt.
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

/// Generate a summary using the `claude` CLI (--print mode).
/// Falls back to the Anthropic API if `claude` CLI is not available.
pub async fn generate_summary(
    _config: &LlmConfig,
    digest: &Digest,
) -> Result<String, String> {
    let label = period_label(&digest.period);

    let formatted_activities: String = digest
        .activities
        .iter()
        .map(format_activity)
        .collect::<Vec<_>>()
        .join("\n");

    let prompt = format!(
        "You are summarizing my work activity for {label}. \
         Be concise and highlight what matters: shipped work, key decisions, \
         and collaboration patterns. Group by theme, not by tool. Skip noise. \
         Use markdown formatting. Keep it under 300 words.\n\n\
         Activities:\n{formatted_activities}"
    );

    // Try `claude --print` first (uses existing Claude Code auth, no API key needed)
    tracing::info!("llm: generating summary via claude CLI");
    match generate_via_cli(&prompt).await {
        Ok(summary) => return Ok(summary),
        Err(e) => {
            tracing::warn!("llm: claude CLI failed ({e}), falling back to API");
        }
    }

    // Fallback: try Anthropic API if key is available
    match generate_via_api(_config, &prompt).await {
        Ok(summary) => Ok(summary),
        Err(e) => Err(format!("LLM generation failed: {e}"))
    }
}

/// Shell out to `claude --print` to generate the summary.
async fn generate_via_cli(prompt: &str) -> Result<String, String> {
    let prompt = prompt.to_string();
    let result = tokio::task::spawn_blocking(move || {
        std::process::Command::new("claude")
            .args(["--print", &prompt])
            .output()
    })
    .await
    .map_err(|e| format!("task join error: {e}"))?
    .map_err(|e| format!("failed to run claude CLI: {e}"))?;

    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        return Err(format!("claude CLI exited with {}: {stderr}", result.status));
    }

    let output = String::from_utf8_lossy(&result.stdout).trim().to_string();
    if output.is_empty() {
        return Err("claude CLI returned empty output".to_string());
    }

    Ok(output)
}

/// Fallback: call the Anthropic Messages API directly.
async fn generate_via_api(_config: &LlmConfig, prompt: &str) -> Result<String, String> {
    let api_key = crate::auth::AuthManager::get_anthropic_key()?
        .ok_or_else(|| "no Anthropic API key and claude CLI not available".to_string())?;

    let body = serde_json::json!({
        "model": _config.model,
        "max_tokens": 1024,
        "messages": [{ "role": "user", "content": prompt }]
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
        .map_err(|e| format!("API request failed: {e}"))?;

    if !response.status().is_success() {
        let body_text = response.text().await.unwrap_or_default();
        return Err(format!("API error: {body_text}"));
    }

    let json: serde_json::Value = response.json().await
        .map_err(|e| format!("failed to parse response: {e}"))?;

    json["content"]
        .as_array()
        .and_then(|arr| arr.first())
        .and_then(|block| block["text"].as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "unexpected API response".to_string())
}
