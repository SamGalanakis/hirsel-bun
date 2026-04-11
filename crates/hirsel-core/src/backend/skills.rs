use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

use lash::{collect_skill_mentions, SkillCatalog};
use serde::Serialize;

use crate::backend::ProjectStore;
use crate::backend::shepherd_runtime::types::{ShepherdMessageChunk, ShepherdScope};

const MAX_FILE_REF_BYTES: usize = 120_000;
const DEFAULT_FILE_REF_LINES: usize = 240;
const MAX_FILE_REF_LINES: usize = 400;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiSkillSummary {
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct FileMention {
    path: String,
    line_start: Option<usize>,
    line_end: Option<usize>,
}

pub async fn list_project_skills(project_id: i64) -> Result<Vec<ApiSkillSummary>, String> {
    let catalog = load_project_skill_catalog(project_id).await?;
    Ok(catalog
        .iter()
        .map(|skill| ApiSkillSummary {
            name: skill.name.clone(),
            description: skill.description.clone(),
        })
        .collect())
}

pub async fn enrich_chat_message_chunks(
    project_id: i64,
    raw_content: String,
    allow_file_refs: bool,
) -> Result<Vec<ShepherdMessageChunk>, String> {
    let content = raw_content.trim().to_string();
    if content.is_empty() {
        return Err("message content is empty".to_string());
    }

    let mut chunks = vec![ShepherdMessageChunk::Text {
        content: content.clone(),
    }];

    let catalog = load_project_skill_catalog(project_id).await?;
    let mut seen_skills = HashSet::new();
    for name in collect_skill_mentions(&content) {
        if !seen_skills.insert(name.clone()) {
            continue;
        }
        let Some(skill) = catalog.get(&name) else {
            continue;
        };
        chunks.push(ShepherdMessageChunk::Skill {
            name: skill.name.clone(),
            description: if skill.description.trim().is_empty() {
                None
            } else {
                Some(skill.description.clone())
            },
            path: skill.path_to_skill_md.display().to_string(),
        });
    }

    if allow_file_refs {
        let root = resolve_project_root(project_id).await?;
        let mut seen_mentions = HashSet::new();
        for mention in collect_file_mentions(&content) {
            if !seen_mentions.insert(mention.clone()) {
                continue;
            }
            let Ok(target) = resolve_relative_path(&root, &mention.path) else {
                continue;
            };
            if !target.exists() || !target.is_file() {
                continue;
            }
            chunks.push(ShepherdMessageChunk::FileRef {
                root_id: "main".to_string(),
                path: mention.path,
                line_start: mention.line_start,
                line_end: mention.line_end,
            });
        }
    }

    Ok(chunks)
}

pub async fn build_user_turn_text(
    scope: &ShepherdScope,
    chunks: &[ShepherdMessageChunk],
) -> Result<String, String> {
    let mut sections = Vec::new();
    let text = chunks
        .iter()
        .filter_map(|chunk| match chunk {
            ShepherdMessageChunk::Text { content } => Some(content.trim()),
            _ => None,
        })
        .filter(|content| !content.is_empty())
        .collect::<Vec<_>>()
        .join("\n");

    if !text.is_empty() {
        sections.push(text);
    }

    for chunk in chunks {
        match chunk {
            ShepherdMessageChunk::Skill { name, path, .. } => {
                let block = render_skill_block(name, path)?;
                sections.push(block);
            }
            ShepherdMessageChunk::FileRef {
                root_id,
                path,
                line_start,
                line_end,
            } => {
                let block =
                    render_file_ref_block(scope, root_id, path, *line_start, *line_end).await?;
                sections.push(block);
            }
            _ => {}
        }
    }

    if sections.is_empty() {
        return Ok("Continue.".to_string());
    }

    Ok(sections.join("\n\n"))
}

fn hirsel_skill_dirs(project_root: &Path) -> Vec<PathBuf> {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    vec![
        home.join(".lash").join("skills"),
        home.join(".hirsel").join("skills"),
        project_root.join(".lash").join("skills"),
        project_root.join(".hirsel").join("skills"),
        project_root.join(".agents").join("lash").join("skills"),
        project_root.join(".agents").join("skills"),
    ]
}

async fn resolve_project_root(project_id: i64) -> Result<PathBuf, String> {
    let store = ProjectStore::open()
        .await
        .map_err(|error| format!("failed to open project store: {}", error))?;
    let project = store
        .get_project(project_id)
        .await
        .map_err(|error| format!("failed to load project {}: {}", project_id, error))?;
    if let Some(cwd) = project.shepherd_cwd.as_deref().filter(|p| !p.trim().is_empty()) {
        let path = PathBuf::from(cwd);
        if path.is_dir() {
            return Ok(path);
        }
    }
    for ws in &project.workspaces {
        if let Some(path_str) = ws.path.as_deref().filter(|p| !p.trim().is_empty()) {
            let path = PathBuf::from(path_str);
            if path.is_dir() {
                return Ok(path);
            }
        }
    }
    Ok(dirs::home_dir().unwrap_or_else(|| PathBuf::from("/")))
}

async fn load_project_skill_catalog(project_id: i64) -> Result<SkillCatalog, String> {
    let root = resolve_project_root(project_id).await?;
    Ok(SkillCatalog::from_dirs(&hirsel_skill_dirs(&root)))
}

async fn scope_workspace_root(scope: &ShepherdScope) -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("HIRSEL_SCOPE_WORKDIR")
        .map(PathBuf::from)
        .filter(|path| path.exists() && path.is_dir())
    {
        return Some(path);
    }

    let project_id = match scope {
        ShepherdScope::General => return None,
        ShepherdScope::Shepherd { project_id, .. }
        | ShepherdScope::Thread { project_id, .. }
        | ShepherdScope::Librarian { project_id, .. } => *project_id,
    };
    let path = resolve_project_root(project_id).await.ok()?;
    if path.is_dir() {
        Some(path)
    } else {
        None
    }
}

fn resolve_relative_path(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let trimmed = relative.trim();
    if trimmed.is_empty() {
        return Err("path is empty".to_string());
    }

    let path = Path::new(trimmed);
    if path.is_absolute() {
        return Err("workspace paths must be relative".to_string());
    }

    for component in path.components() {
        match component {
            Component::Normal(_) | Component::CurDir => {}
            _ => return Err("workspace paths may not escape the selected workspace".to_string()),
        }
    }

    Ok(root.join(path))
}

fn parse_skill_frontmatter(text: &str) -> Result<(String, String, String), String> {
    let text = text.trim_start();
    if !text.starts_with("---") {
        return Err("skill is missing frontmatter".to_string());
    }

    let after_open = &text[3..];
    let close_idx = after_open
        .find("\n---")
        .ok_or_else(|| "skill frontmatter is not closed".to_string())?;
    let frontmatter = &after_open[..close_idx];
    let body_start = 3 + close_idx + 4;
    let body = text[body_start..].trim().to_string();

    let mut name = String::new();
    let mut description = String::new();
    for line in frontmatter.lines() {
        let line = line.trim();
        if let Some(value) = line.strip_prefix("name:") {
            name = value.trim().to_string();
        } else if let Some(value) = line.strip_prefix("description:") {
            description = value.trim().to_string();
        }
    }

    Ok((name, description, body))
}

fn render_skill_block(name: &str, path: &str) -> Result<String, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("failed to read skill '{}': {}", path, error))?;
    let (parsed_name, _description, instructions) = parse_skill_frontmatter(&text)?;
    let canonical_name = if parsed_name.trim().is_empty() {
        name.to_string()
    } else {
        parsed_name
    };
    Ok(format!(
        "<skill>\n<name>{}</name>\n<path>{}</path>\n{}\n</skill>",
        canonical_name, path, instructions
    ))
}

async fn render_file_ref_block(
    scope: &ShepherdScope,
    root_id: &str,
    relative_path: &str,
    line_start: Option<usize>,
    line_end: Option<usize>,
) -> Result<String, String> {
    if matches!(scope, ShepherdScope::General) {
        return Ok(format!(
            "<workspace-file>\n<root-id>{}</root-id>\n<path>{}</path>\n<status>unavailable</status>\n</workspace-file>",
            root_id, relative_path
        ));
    }

    if root_id != "main" {
        return Ok(format!(
            "<workspace-file>\n<root-id>{}</root-id>\n<path>{}</path>\n<status>unsupported</status>\n</workspace-file>",
            root_id, relative_path
        ));
    }

    let Some(root) = scope_workspace_root(scope).await else {
        return Ok(format!(
            "<workspace-file>\n<root-id>{}</root-id>\n<path>{}</path>\n<status>unavailable</status>\n</workspace-file>",
            root_id, relative_path
        ));
    };
    let target = resolve_relative_path(&root, relative_path)?;
    if !target.exists() || !target.is_file() {
        return Ok(format!(
            "<workspace-file>\n<root-id>{}</root-id>\n<path>{}</path>\n<status>missing</status>\n</workspace-file>",
            root_id, relative_path
        ));
    }

    let bytes = std::fs::read(&target)
        .map_err(|error| format!("failed to read '{}': {}", target.display(), error))?;
    let mime = mime_guess::from_path(&target)
        .first_raw()
        .unwrap_or("application/octet-stream");
    let is_text = !bytes.contains(&0) && std::str::from_utf8(&bytes).is_ok();
    if !is_text {
        return Ok(format!(
            "<workspace-file>\n<root-id>{}</root-id>\n<path>{}</path>\n<mime>{}</mime>\n<status>binary</status>\n</workspace-file>",
            root_id, relative_path, mime
        ));
    }

    let mut content = String::from_utf8(bytes)
        .map_err(|error| format!("failed to decode '{}': {}", target.display(), error))?;
    let mut truncated = false;
    if content.len() > MAX_FILE_REF_BYTES {
        content.truncate(MAX_FILE_REF_BYTES);
        truncated = true;
    }

    let lines = content.lines().collect::<Vec<_>>();
    let total_lines = lines.len();
    let start = line_start.unwrap_or(1).max(1);
    let requested_end = line_end.unwrap_or(start.saturating_add(DEFAULT_FILE_REF_LINES - 1));
    let capped_end = requested_end.min(start.saturating_add(MAX_FILE_REF_LINES - 1));
    let actual_slice = if start > total_lines {
        String::new()
    } else {
        lines
            .iter()
            .skip(start - 1)
            .take(capped_end.saturating_sub(start).saturating_add(1))
            .copied()
            .collect::<Vec<_>>()
            .join("\n")
    };

    let language = target
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("text");

    Ok(format!(
        "<workspace-file>\n<root-id>{}</root-id>\n<path>{}</path>\n<language>{}</language>\n<line-start>{}</line-start>\n<line-end>{}</line-end>\n<truncated>{}</truncated>\n<content>\n{}\n</content>\n</workspace-file>",
        root_id,
        relative_path,
        language,
        start,
        capped_end.min(total_lines),
        if truncated || capped_end < requested_end { "true" } else { "false" },
        actual_slice
    ))
}

fn parse_line_suffix(raw: &str) -> (String, Option<usize>, Option<usize>) {
    let Some((path, suffix)) = raw.rsplit_once(':') else {
        return (raw.to_string(), None, None);
    };
    let suffix = suffix.trim();
    if suffix.is_empty() || !suffix.chars().all(|ch| ch.is_ascii_digit() || ch == '-') {
        return (raw.to_string(), None, None);
    }
    if let Some((start, end)) = suffix.split_once('-') {
        let Ok(start) = start.parse::<usize>() else {
            return (raw.to_string(), None, None);
        };
        let Ok(end) = end.parse::<usize>() else {
            return (raw.to_string(), None, None);
        };
        return (path.to_string(), Some(start), Some(end));
    }
    let Ok(line) = suffix.parse::<usize>() else {
        return (raw.to_string(), None, None);
    };
    (path.to_string(), Some(line), Some(line))
}

fn is_valid_file_mention_start(bytes: &[u8], idx: usize) -> bool {
    if idx == 0 {
        return true;
    }
    matches!(
        bytes[idx - 1],
        b' ' | b'\n' | b'\r' | b'\t' | b'(' | b'[' | b'{' | b'<' | b'"' | b'\'' | b'`'
    )
}

fn collect_file_mentions(text: &str) -> Vec<FileMention> {
    let bytes = text.as_bytes();
    let mut mentions = Vec::new();
    let mut idx = 0usize;

    while idx < bytes.len() {
        if bytes[idx] != b'@' {
            idx += 1;
            continue;
        }

        if !is_valid_file_mention_start(bytes, idx) {
            idx += 1;
            continue;
        }

        let start = idx + 1;
        let mut end = start;
        while end < bytes.len() && !bytes[end].is_ascii_whitespace() {
            end += 1;
        }
        if end == start {
            idx += 1;
            continue;
        }

        let raw =
            text[start..end].trim_matches(|ch: char| matches!(ch, ',' | '.' | ')' | ']' | '}'));
        if raw.is_empty() {
            idx = end;
            continue;
        }
        let (path, line_start, line_end) = parse_line_suffix(raw);
        if path.is_empty() {
            idx = end;
            continue;
        }
        mentions.push(FileMention {
            path,
            line_start,
            line_end,
        });
        idx = end;
    }

    mentions
}

#[cfg(test)]
mod tests {
    use super::{collect_file_mentions, parse_line_suffix};

    #[test]
    fn parses_file_mentions_with_optional_line_suffix() {
        let mentions = collect_file_mentions("check @src/main.rs and @src/lib.rs:10-20 please");
        assert_eq!(mentions.len(), 2);
        assert_eq!(mentions[0].path, "src/main.rs");
        assert_eq!(mentions[0].line_start, None);
        assert_eq!(mentions[1].path, "src/lib.rs");
        assert_eq!(mentions[1].line_start, Some(10));
        assert_eq!(mentions[1].line_end, Some(20));
    }

    #[test]
    fn ignores_embedded_email_like_at_signs() {
        let mentions = collect_file_mentions("mail me at test@example.com and check @src/main.rs");
        assert_eq!(mentions.len(), 1);
        assert_eq!(mentions[0].path, "src/main.rs");
    }

    #[test]
    fn allows_wrapped_file_mentions() {
        let mentions = collect_file_mentions("review (@src/main.rs) before continuing");
        assert_eq!(mentions.len(), 1);
        assert_eq!(mentions[0].path, "src/main.rs");
    }

    #[test]
    fn parses_single_line_suffix() {
        let (path, line_start, line_end) = parse_line_suffix("src/main.rs:42");
        assert_eq!(path, "src/main.rs");
        assert_eq!(line_start, Some(42));
        assert_eq!(line_end, Some(42));
    }
}
