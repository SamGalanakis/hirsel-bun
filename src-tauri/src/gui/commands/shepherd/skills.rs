//! Skill loading and management for Shepherd
//!
//! Reads skills from `~/.hirsel/skills/` and `~/.lash/skills/` directories.
//! Each skill is a subdirectory containing a `SKILL.md` with YAML frontmatter.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Summary of an installed skill (for listing)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillSummary {
    pub name: String,
    pub description: String,
    pub source: String,
}

/// Full skill content
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillDetail {
    pub name: String,
    pub description: String,
    pub content: String,
    pub source_path: String,
}

/// YAML frontmatter parsed from SKILL.md
#[derive(Debug, Deserialize)]
struct SkillFrontmatter {
    name: Option<String>,
    description: Option<String>,
}

/// Get the skill directories to search (hirsel first, then lash as fallback)
fn skill_dirs() -> Vec<(PathBuf, &'static str)> {
    let mut dirs = Vec::new();

    if let Some(home) = dirs::home_dir() {
        let hirsel_skills = home.join(".hirsel").join("skills");
        if hirsel_skills.is_dir() {
            dirs.push((hirsel_skills, "hirsel"));
        }

        let lash_skills = home.join(".lash").join("skills");
        if lash_skills.is_dir() {
            dirs.push((lash_skills, "lash"));
        }
    }

    dirs
}

/// Parse a SKILL.md file into name, description, and body content
fn parse_skill_md(path: &Path) -> Option<(String, String, String)> {
    let raw = std::fs::read_to_string(path).ok()?;
    let trimmed = raw.trim_start();

    if !trimmed.starts_with("---") {
        // No frontmatter — use filename as name
        let name = path
            .parent()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        return Some((name, String::new(), raw));
    }

    // Find closing ---
    let after_opening = &trimmed[3..];
    let end = after_opening.find("\n---")?;
    let yaml_str = &after_opening[..end];
    let body = after_opening[end + 4..].trim_start().to_string();

    let fm: SkillFrontmatter = serde_yaml::from_str(yaml_str).ok()?;

    let name = fm.name.unwrap_or_else(|| {
        path.parent()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default()
    });
    let description = fm.description.unwrap_or_default();

    Some((name, description, body))
}

/// Load all skills from disk
fn load_all_skills() -> Vec<SkillDetail> {
    let mut seen = std::collections::HashSet::new();
    let mut skills = Vec::new();

    for (dir, _source) in skill_dirs() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };

        for entry in entries.flatten() {
            let entry_path = entry.path();
            if !entry_path.is_dir() {
                continue;
            }

            let skill_md = entry_path.join("SKILL.md");
            if !skill_md.exists() {
                continue;
            }

            if let Some((name, description, content)) = parse_skill_md(&skill_md) {
                if seen.contains(&name) {
                    continue; // hirsel overrides lash on name conflict
                }
                seen.insert(name.clone());
                skills.push(SkillDetail {
                    name,
                    description,
                    content,
                    source_path: skill_md.to_string_lossy().to_string(),
                });
            }
        }
    }

    skills.sort_by(|a, b| a.name.cmp(&b.name));
    skills
}

/// List installed skills (name + description)
#[tauri::command]
pub async fn list_shepherd_skills() -> Result<Vec<SkillSummary>, String> {
    let skills = load_all_skills();
    Ok(skills
        .into_iter()
        .map(|s| {
            let source = if s.source_path.contains(".hirsel") {
                "hirsel".to_string()
            } else {
                "lash".to_string()
            };
            SkillSummary {
                name: s.name,
                description: s.description,
                source,
            }
        })
        .collect())
}

/// Get full skill content by name
#[tauri::command]
pub async fn get_shepherd_skill(name: String) -> Result<SkillDetail, String> {
    let skills = load_all_skills();
    skills
        .into_iter()
        .find(|s| s.name == name)
        .ok_or_else(|| format!("Skill not found: {}", name))
}
