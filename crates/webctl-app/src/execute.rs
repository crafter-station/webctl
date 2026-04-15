use std::path::Path;

use anyhow::{Context, anyhow};
use serde::{Deserialize, Serialize};
use tokio::process::Command;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecResult {
    pub status: u16,
    pub url: String,
    pub title: String,
    pub content: String,
    pub word_count: u64,
}

pub async fn fetch_page(url: &str) -> anyhow::Result<ExecResult> {
    let output = Command::new("defuddle")
        .args(["parse", url, "--json"])
        .output()
        .await
        .context("failed to run defuddle. Install with: npm install -g @anthropic-ai/defuddle")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("defuddle failed: {stderr}"));
    }

    let parsed: DefuddleOutput = serde_json::from_slice(&output.stdout)
        .context("failed to parse defuddle JSON output")?;

    let content = html_to_text(&parsed.content);

    Ok(ExecResult {
        status: 200,
        url: url.to_string(),
        title: parsed.title,
        content,
        word_count: parsed.word_count,
    })
}

pub fn format_human(result: &ExecResult, site_name: &str, command: &str) -> String {
    let mut output = String::new();
    output.push_str(&format!("\n  {} — {}\n\n", site_name, result.title));

    let lines: Vec<&str> = result.content.lines().collect();
    let page_size = 30;
    let total_lines = lines.len();
    let display_lines = lines.iter().take(page_size);

    for line in display_lines {
        if line.trim().is_empty() {
            continue;
        }
        output.push_str(&format!("  {line}\n"));
    }

    if total_lines > page_size {
        output.push_str(&format!(
            "\n  ... ({} more lines, showing first {})\n",
            total_lines - page_size,
            page_size
        ));
    }

    output.push('\n');
    output.push_str("  Next steps:\n");
    output.push_str(&format!("    {site_name} {command} --json       Machine-readable output\n"));
    output.push_str(&format!("    {site_name} --help                 All commands\n"));

    output
}

pub fn format_json(result: &ExecResult) -> anyhow::Result<String> {
    serde_json::to_string_pretty(result).context("failed to serialize exec result")
}

fn html_to_text(html: &str) -> String {
    let mut text = html.to_string();

    text = text.replace("<br>", "\n").replace("<br/>", "\n").replace("<br />", "\n");
    text = text.replace("</p>", "\n").replace("</div>", "\n").replace("</li>", "\n");
    text = text.replace("</tr>", "\n").replace("</h1>", "\n").replace("</h2>", "\n");
    text = text.replace("</h3>", "\n").replace("</h4>", "\n");

    let mut result = String::with_capacity(text.len());
    let mut in_tag = false;
    for ch in text.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => result.push(ch),
            _ => {}
        }
    }

    result = result.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#x27;", "'")
        .replace("&nbsp;", " ");

    let lines: Vec<String> = result
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();

    lines.join("\n")
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DefuddleOutput {
    content: String,
    title: String,
    #[serde(default)]
    word_count: u64,
}
