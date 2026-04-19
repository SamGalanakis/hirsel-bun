//! Heading-aware markdown chunker for KG node content.
//!
//! Port of figments' `split_markdown_into_chunks`. Splits on markdown
//! blocks, keeps heading paths with each chunk, targets
//! `CHUNK_TARGET_TOKENS` while respecting `CHUNK_MAX_TOKENS` and merging
//! stragglers below `CHUNK_MIN_TOKENS`. An emergency word-level split
//! bisects any block that exceeds the hard max alone.
//!
//! HTML content is stripped to visible text via `scraper` before chunking
//! — we don't need Markdown-perfect output because chunks are retrieval
//! tokens, not rendered prose.

use scraper::Html;

use crate::backend::embeddings::estimate_tokens;
use crate::backend::runtime_settings::{keys, Defaults, RuntimeSettings};

pub const CHUNKER_VERSION: &str = "hirsel-md-chunker-v1";

#[derive(Debug, Clone)]
pub struct ChunkingConfig {
    pub target_tokens: usize,
    pub overlap_tokens: usize,
    pub max_tokens: usize,
    pub min_tokens: usize,
}

impl ChunkingConfig {
    pub async fn from_settings() -> Self {
        Self {
            target_tokens: RuntimeSettings::get_or(
                keys::CHUNK_TARGET_TOKENS,
                Defaults::CHUNK_TARGET_TOKENS,
            )
            .await,
            overlap_tokens: RuntimeSettings::get_or(
                keys::CHUNK_OVERLAP_TOKENS,
                Defaults::CHUNK_OVERLAP_TOKENS,
            )
            .await,
            max_tokens: RuntimeSettings::get_or(keys::CHUNK_MAX_TOKENS, Defaults::CHUNK_MAX_TOKENS)
                .await,
            min_tokens: RuntimeSettings::get_or(keys::CHUNK_MIN_TOKENS, Defaults::CHUNK_MIN_TOKENS)
                .await,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ChunkDraft {
    pub chunk_index: usize,
    pub chunk_path: Vec<String>,
    pub chunk_text: String,
    pub token_count: usize,
}

/// Split a document (markdown or HTML) into retrieval chunks. Subtype
/// defaults to markdown — pass `"html"` to first strip tags.
pub fn split_into_chunks(
    content: &str,
    subtype: Option<&str>,
    cfg: &ChunkingConfig,
) -> Vec<ChunkDraft> {
    let markdown_source = match subtype {
        Some("html") => html_to_text(content),
        _ => content.to_string(),
    };
    split_markdown_into_chunks(&markdown_source, cfg)
}

fn split_markdown_into_chunks(markdown: &str, cfg: &ChunkingConfig) -> Vec<ChunkDraft> {
    let blocks = markdown_blocks(markdown);
    let mut chunks: Vec<ChunkDraft> = Vec::new();
    let mut current = String::new();
    let mut path: Vec<String> = Vec::new();
    let mut current_path: Vec<String> = Vec::new();
    for block in blocks {
        if let Some(heading) = markdown_heading(&block) {
            path.truncate(heading.0.saturating_sub(1));
            path.push(heading.1);
            current_path = path.clone();
        }
        let prospective = if current.is_empty() {
            block.clone()
        } else {
            format!("{current}\n\n{block}")
        };
        if estimate_tokens(&prospective) > cfg.target_tokens && !current.is_empty() {
            push_chunk(&mut chunks, &current, &current_path, cfg);
            current = overlap_tail(&current, cfg.overlap_tokens);
            if current.is_empty() {
                current = block;
            } else {
                current = format!("{current}\n\n{block}");
            }
        } else {
            current = prospective;
        }
        while estimate_tokens(&current) > cfg.max_tokens {
            let (head, tail) = split_at_token_estimate(&current, cfg.max_tokens);
            push_chunk(&mut chunks, &head, &current_path, cfg);
            current = tail;
        }
    }
    push_chunk(&mut chunks, &current, &current_path, cfg);
    chunks
}

fn push_chunk(chunks: &mut Vec<ChunkDraft>, text: &str, path: &[String], cfg: &ChunkingConfig) {
    let normalized = normalize_markdown(text);
    if normalized.is_empty() {
        return;
    }
    let token_count = estimate_tokens(&normalized);
    if token_count < cfg.min_tokens && !chunks.is_empty() {
        if let Some(last) = chunks.last_mut() {
            let merged = normalize_markdown(&format!("{}\n\n{}", last.chunk_text, normalized));
            last.token_count = estimate_tokens(&merged);
            last.chunk_text = merged;
        }
        return;
    }
    chunks.push(ChunkDraft {
        chunk_index: chunks.len(),
        chunk_path: path.to_vec(),
        chunk_text: normalized,
        token_count,
    });
}

fn markdown_blocks(markdown: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut current: Vec<String> = Vec::new();
    let mut in_fence = false;
    for line in markdown.lines() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
        }
        if line.trim().is_empty() && !in_fence {
            if !current.is_empty() {
                blocks.push(current.join("\n"));
                current.clear();
            }
        } else {
            current.push(line.to_string());
        }
    }
    if !current.is_empty() {
        blocks.push(current.join("\n"));
    }
    blocks
}

fn markdown_heading(block: &str) -> Option<(usize, String)> {
    let first = block.lines().next()?.trim();
    let hashes = first.chars().take_while(|c| *c == '#').count();
    if (1..=6).contains(&hashes) && first.chars().nth(hashes) == Some(' ') {
        Some((hashes, first[hashes..].trim().to_string()))
    } else {
        None
    }
}

fn normalize_markdown(markdown: &str) -> String {
    markdown
        .replace("\r\n", "\n")
        .split("\n\n\n")
        .collect::<Vec<_>>()
        .join("\n\n")
        .trim()
        .to_string()
}

fn split_at_token_estimate(text: &str, max_tokens: usize) -> (String, String) {
    let words: Vec<&str> = text.split_whitespace().collect();
    let split = ((max_tokens * 3) / 4).min(words.len()).max(1);
    (words[..split].join(" "), words[split..].join(" "))
}

fn overlap_tail(text: &str, overlap_tokens: usize) -> String {
    if overlap_tokens == 0 {
        return String::new();
    }
    let words: Vec<&str> = text.split_whitespace().collect();
    let count = ((overlap_tokens * 3) / 4).min(words.len());
    words[words.len().saturating_sub(count)..].join(" ")
}

/// Strip HTML tags down to readable text. Good enough for retrieval
/// embeddings — we're optimising for tokens, not round-trip fidelity.
pub fn html_to_text(html: &str) -> String {
    let doc = Html::parse_fragment(html);
    let mut out = String::new();
    for text in doc.root_element().text() {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(trimmed);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> ChunkingConfig {
        ChunkingConfig {
            target_tokens: 50,
            overlap_tokens: 8,
            max_tokens: 120,
            min_tokens: 5,
        }
    }

    #[test]
    fn splits_respecting_headings() {
        let md = "\
# Title

Some intro.

## Section A

body of A body of A body of A body of A body of A body of A body of A body of A.

## Section B

body of B body of B body of B body of B body of B body of B body of B.
";
        let chunks = split_markdown_into_chunks(md, &cfg());
        assert!(!chunks.is_empty());
        assert!(chunks.iter().all(|c| !c.chunk_text.is_empty()));
        // Heading path is propagated once we reach Section A.
        assert!(chunks
            .iter()
            .any(|c| c.chunk_path.iter().any(|p| p.contains("Section"))));
    }

    #[test]
    fn strips_html_tags() {
        let html = "<div><h1>Title</h1><p>Hello <b>world</b>.</p></div>";
        let text = html_to_text(html);
        assert!(text.contains("Title"));
        assert!(text.contains("Hello"));
        assert!(text.contains("world"));
        assert!(!text.contains('<'));
    }

    #[test]
    fn empty_content_produces_no_chunks() {
        let chunks = split_markdown_into_chunks("", &cfg());
        assert!(chunks.is_empty());
    }
}
