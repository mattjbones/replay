use crate::config::{LlmConfig, LlmProfile};
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

fn profile_summary_instructions(profile: &LlmProfile) -> &'static str {
    match profile {
        LlmProfile::Work => {
            "You are summarizing my work activity. Be concise and highlight what matters: shipped work, key decisions, and collaboration patterns. Group by theme, not by tool. Include blockers/risks only if they are explicit in the data. Skip noise."
        }
        LlmProfile::Personal => {
            "You are summarizing my personal developer activity. Focus on momentum, learning, craft, and consistency. Avoid corporate productivity framing. Mention effort signals (time invested, sustained focus, or scope progressed) without using burnout/velocity language."
        }
    }
}

/// Generate a summary using the `claude` CLI (--print mode).
/// Falls back to the Anthropic API if `claude` CLI is not available.
pub async fn generate_summary(
    _config: &LlmConfig,
    digest: &Digest,
) -> Result<String, String> {
    let label = period_label(&digest.period);

    let mut formatted_activities = String::new();
    for (i, a) in digest.activities.iter().enumerate() {
        if i > 0 {
            formatted_activities.push('\n');
        }
        formatted_activities.push_str(&format_activity(a));
    }

    let prompt = format!(
        "{} For {label}. \
         Use markdown formatting. Keep it under 300 words.\n\n\
         Activities:\n{formatted_activities}"
        ,
        profile_summary_instructions(&_config.profile),
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

/// Generate a response from a raw prompt, trying CLI then API fallback.
pub async fn generate_from_prompt(config: &LlmConfig, prompt: &str) -> Result<String, String> {
    match generate_via_cli(prompt).await {
        Ok(result) => return Ok(result),
        Err(e) => {
            tracing::warn!("llm: claude CLI failed ({e}), falling back to API");
        }
    }
    match generate_via_api(config, prompt).await {
        Ok(result) => Ok(result),
        Err(e) => Err(format!("LLM generation failed: {e}"))
    }
}

/// Find the claude binary, checking common paths since bundled .app doesn't inherit shell PATH.
fn find_claude_binary() -> Option<String> {
    // Try PATH first
    if let Ok(output) = std::process::Command::new("which").arg("claude").output() {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path.is_empty() {
                return Some(path);
            }
        }
    }
    // Common install locations
    let candidates = [
        dirs::home_dir().map(|h| h.join(".claude/bin/claude")),
        dirs::home_dir().map(|h| h.join(".local/bin/claude")),
        dirs::home_dir().map(|h| h.join(".npm-global/bin/claude")),
        Some(std::path::PathBuf::from("/usr/local/bin/claude")),
        Some(std::path::PathBuf::from("/opt/homebrew/bin/claude")),
    ];
    for candidate in candidates.into_iter().flatten() {
        if candidate.exists() {
            return Some(candidate.to_string_lossy().to_string());
        }
    }
    None
}

/// Shell out to `claude --print` to generate the summary.
async fn generate_via_cli(prompt: &str) -> Result<String, String> {
    let claude_path = find_claude_binary()
        .ok_or_else(|| "claude CLI not found".to_string())?;
    let prompt = prompt.to_string();
    let result = tokio::task::spawn_blocking(move || {
        std::process::Command::new(&claude_path)
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
