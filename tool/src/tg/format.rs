//! Pure text helpers: chunking, Markdown flattening, keyword
//! normalisation. No I/O except `read_stdin`; everything else is unit-
//! testable without a network.

use anyhow::{Context, Result};
use std::io::{self, Read};
use std::sync::LazyLock;

use crate::render::DECISION_MARKER;

/// Words that mean "give me the brief" — short-circuit to the rendered
/// brief without ever calling the model.
pub const ASK_WORDS: &[&str] = &["brief", "what's up", "status", "pulse", "liveness"];

/// Chat keywords handled locally, mapped to a help line for /help.
pub const KEYWORD_HELP: &[(&str, &str)] = &[
    ("/status", "show current status (brief)"),
    ("/collab", "find OMP collab session, send control URL"),
    ("/help", "show this help"),
];

pub fn read_stdin() -> Result<String> {
    let mut buf = String::new();
    io::stdin()
        .read_to_string(&mut buf)
        .context("Failed to read stdin")?;
    Ok(buf)
}

/// A CLI text argument of `-` means "read stdin" — the classic Unix
/// convention the spec's `zolly telegram-send -` uses. `None` also means
/// stdin, so both spellings work.
pub fn is_stdin_marker(text: &Option<String>) -> bool {
    matches!(text.as_deref(), None | Some("-"))
}

/// Split `text` into chunks no longer than `max_len` **bytes**, never
/// splitting inside a multi-byte codepoint. Prefers the last `\n`, then the
/// last space, then a hard cut at the largest safe char boundary.
pub fn split_message(text: &str, max_len: usize) -> Vec<String> {
    if text.len() <= max_len {
        return vec![text.to_string()];
    }

    let mut parts = Vec::new();
    let mut remaining = text;

    while !remaining.is_empty() {
        if remaining.len() <= max_len {
            parts.push(remaining.to_string());
            break;
        }

        // Largest char boundary at or below max_len.
        let mut hard = max_len.min(remaining.len());
        while hard > 0 && !remaining.is_char_boundary(hard) {
            hard -= 1;
        }
        if hard == 0 {
            // Pathological: max_len smaller than the first codepoint.
            // Advance by exactly one codepoint to guarantee progress.
            let next = remaining
                .char_indices()
                .nth(1)
                .map(|(i, _)| i)
                .unwrap_or(remaining.len());
            parts.push(remaining[..next].to_string());
            remaining = &remaining[next..];
            continue;
        }

        let slice = &remaining[..hard];
        let split_at = slice
            .rfind('\n')
            .or_else(|| slice.rfind(' '))
            .unwrap_or(hard);

        // Avoid pushing an empty part if the only break is at index 0.
        let split_at = if split_at == 0 { hard } else { split_at };

        parts.push(remaining[..split_at].to_string());
        remaining = &remaining[split_at..];
        // Don't trim_start — leading whitespace in the next chunk is fine
        // and trimming can eat a deliberate indent.
    }

    parts
}

fn link_regex() -> &'static regex::Regex {
    static RE: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"\[([^\]]+)\]\(([^)]+)\)").unwrap());
    &RE
}

/// Strip common Markdown decorations so the text reads cleanly in a plain
/// Telegram message (we don't set `parse_mode`). Bold/italic double markers
/// are removed; single `*` and `_` are left alone so `snake_case` and math
/// survive intact.
pub fn strip_markdown(text: &str) -> String {
    let mut result = String::new();
    let mut in_code_block = false;

    for line in text.lines() {
        let trimmed = line.trim_start();

        if trimmed.starts_with("```") {
            in_code_block = !in_code_block;
            result.push_str(line);
            result.push('\n');
            continue;
        }

        if in_code_block {
            result.push_str(line);
            result.push('\n');
            continue;
        }

        // Heading: `## Title` -> `Title`
        let line = if let Some(stripped) = trimmed.strip_prefix('#') {
            let level = trimmed.chars().take_while(|&c| c == '#').count();
            if stripped.chars().nth(level - 1) == Some(' ') {
                stripped[level..].to_string()
            } else {
                line.to_string()
            }
        } else {
            line.to_string()
        };

        // Bullet / blockquote -> indent
        let line = if let Some(stripped) = line.trim_start().strip_prefix(['-', '*', '•']) {
            format!("  {}", stripped.trim_start())
        } else if let Some(stripped) = line.trim_start().strip_prefix("> ") {
            format!("  {}", stripped)
        } else {
            line
        };

        // Only strip *paired* markers, so `snake_case` stays intact.
        let line = line.replace("**", "").replace("__", "");

        // [text](url) -> text (url)
        let line = link_regex().replace_all(&line, "$1 ($2)").to_string();

        result.push_str(&line);
        result.push('\n');
    }

    result
}

/// Move the persona's decision section to the top so it isn't buried under
/// status noise. No-op if the marker isn't present.
pub fn hoist_decision(text: &str) -> String {
    if let Some(pos) = text.find(DECISION_MARKER) {
        let before = text[..pos].trim_end();
        let decision_and_after = &text[pos..];
        if before.is_empty() {
            decision_and_after.trim().to_string()
        } else {
            format!("{}\n\n{}", decision_and_after.trim(), before)
        }
    } else {
        text.to_string()
    }
}

/// Normalise an inbound message to a keyword: strip a leading `/`, cut an
/// `@botname` suffix, drop trailing punctuation, lowercase. `"/Status@MyBot."`
/// becomes `"status"`.
pub fn normalise_keyword(text: &str) -> String {
    let t = text.trim();
    let t = t.strip_prefix('/').unwrap_or(t);
    let t = match t.find('@') {
        Some(idx) => &t[..idx],
        None => t,
    };
    let end = t
        .char_indices()
        .rev()
        .find(|(_, c)| c.is_alphanumeric() || *c == '-' || *c == '_')
        .map(|(i, _)| i + 1)
        .unwrap_or(t.len());
    t[..end].to_lowercase()
}

/// True when the (normalised) message asks for the brief. Accepts the
/// exact phrase anywhere in ASK_WORDS, or the keyword form
/// ("status", "what's up") as the first or last word of a short sentence.
pub fn is_ask(keyword: &str) -> bool {
    ASK_WORDS.iter().any(|w| {
        keyword == *w
            || keyword.starts_with(&format!("{} ", w))
            || keyword.ends_with(&format!(" {}", w))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_respects_max_len_and_multibyte() {
        let text = "héllo wörld — ünïcödé tëxt thät ädds üp".repeat(20);
        let parts = split_message(&text, 32);
        assert!(parts.len() > 1);
        for p in &parts {
            assert!(p.len() <= 32);
            assert!(std::str::from_utf8(p.as_bytes()).is_ok(), "split broke a codepoint");
        }
        assert_eq!(parts.concat(), text);
    }

    #[test]
    fn split_short_text_is_single_chunk() {
        assert_eq!(split_message("hi", 4096), vec!["hi".to_string()]);
    }

    #[test]
    fn strip_markdown_keeps_code_blocks_and_snake_case() {
        let input = "## Title\n- **bold** item\nsnake_case stays\n```\n**raw**\n```";
        let out = strip_markdown(input);
        assert!(out.contains("Title\n"));
        assert!(out.contains("  bold item\n"));
        assert!(out.contains("snake_case stays\n"));
        assert!(out.contains("```\n**raw**\n```"));
    }

    #[test]
    fn hoist_moves_decision_to_front() {
        let input = "status noise\nmore noise\n\nWhat to do now: review results\nrest";
        let out = hoist_decision(input);
        assert!(out.starts_with("What to do now: review results"));
        assert!(out.contains("status noise"));
    }

    #[test]
    fn normalise_strips_slash_botname_and_punctuation() {
        assert_eq!(normalise_keyword("/Status@MyBot."), "status");
        assert_eq!(normalise_keyword("  YES!  "), "yes");
        assert_eq!(normalise_keyword("what's up?"), "what's up");
    }

    #[test]
    fn is_ask_matches_known_words() {
        assert!(is_ask("brief"));
        assert!(is_ask("what's up"));
        assert!(is_ask("status please"));
        assert!(is_ask("give me the pulse"));
        assert!(!is_ask("deploy now"));
    }

    #[test]
    fn stdin_marker_accepts_dash_and_none() {
        assert!(is_stdin_marker(&None));
        assert!(is_stdin_marker(&Some("-".to_string())));
        assert!(!is_stdin_marker(&Some("hello".to_string())));
    }
}
