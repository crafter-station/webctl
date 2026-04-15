use std::io::IsTerminal;

use anyhow::{Context, anyhow};
use owo_colors::OwoColorize;
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

pub fn use_color() -> bool {
    std::env::var("NO_COLOR").is_err() && std::io::stdout().is_terminal()
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

pub fn format_human(
    result: &ExecResult,
    site_name: &str,
    command: &str,
    descriptor: &webctl_ir::SiteDescriptor,
) -> String {
    let color = use_color();
    let mut output = String::new();

    let header = format!("{} — {}", site_name, result.title);
    if color {
        output.push_str(&format!("\n  {}\n\n", header.bold()));
    } else {
        output.push_str(&format!("\n  {header}\n\n"));
    }

    let lines: Vec<&str> = result.content.lines().collect();
    let page_size = 30;
    let total_lines = lines.len();

    for line in lines.iter().take(page_size) {
        if line.trim().is_empty() {
            continue;
        }
        let formatted = format_content_line(line, color);
        output.push_str(&format!("  {formatted}\n"));
    }

    if total_lines > page_size {
        let more_msg = format!("... ({} more lines, showing first {})", total_lines - page_size, page_size);
        if color {
            output.push_str(&format!("\n  {}\n", more_msg.dimmed()));
        } else {
            output.push_str(&format!("\n  {more_msg}\n"));
        }
    }

    output.push('\n');
    output.push_str(&webctl_emit_cli::build_next_steps_after_exec(
        site_name, command, descriptor, color,
    ));

    output
}

fn format_content_line(line: &str, color: bool) -> String {
    if !color {
        return line.to_string();
    }

    let trimmed = line.trim();

    if trimmed.starts_with(|c: char| c.is_ascii_digit()) && trimmed.contains('.') {
        let parts: Vec<&str> = trimmed.splitn(2, ". ").collect();
        if parts.len() == 2 {
            return format!("{}. {}", parts[0].green().bold(), parts[1].white());
        }
    }

    if trimmed.contains("points by") || trimmed.contains("hours ago") || trimmed.contains("comments") {
        return format!("{}", trimmed.dimmed());
    }

    if trimmed.starts_with('(') && trimmed.ends_with(')') {
        return format!("{}", trimmed.cyan());
    }

    line.to_string()
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
