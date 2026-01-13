//! Template management commands.
//!
//! Provides functionality for listing and using spec templates.
//! Templates are stored in the hirsel package's templates directory
//! and contain pre-made spec.md files (and optionally eval.md files).

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// A spec template.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Template {
    /// Template name (directory name)
    pub name: String,
    /// Full path to template directory
    pub path: PathBuf,
    /// Description (first line of spec.md)
    pub description: String,
    /// Whether the template includes an eval.md file
    pub has_eval: bool,
}

/// Get the templates directory path.
///
/// Templates are stored relative to the hirsel config directory.
/// Returns `~/.hirsel/templates/` by default.
pub fn get_templates_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".hirsel")
        .join("templates")
}

/// List all available templates.
///
/// Scans the templates directory for subdirectories containing spec.md files.
/// Returns a sorted list of templates with their metadata.
pub fn list_templates() -> Vec<Template> {
    let templates_dir = get_templates_dir();

    if !templates_dir.exists() {
        return Vec::new();
    }

    let mut templates = Vec::new();

    if let Ok(entries) = std::fs::read_dir(&templates_dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_dir() {
                let spec_file = path.join("spec.md");
                if spec_file.exists() {
                    if let Some(template) = parse_template(&path, &spec_file) {
                        templates.push(template);
                    }
                }
            }
        }
    }

    // Sort by name
    templates.sort_by(|a, b| a.name.cmp(&b.name));
    templates
}

/// Parse a template from its directory and spec file.
fn parse_template(template_dir: &Path, spec_file: &Path) -> Option<Template> {
    let name = template_dir.file_name()?.to_string_lossy().to_string();

    // Read first line as description
    let content = std::fs::read_to_string(spec_file).ok()?;
    let first_line = content.lines().next().unwrap_or("");

    // Remove markdown heading prefix (# or ##)
    let description = first_line.trim_start_matches('#').trim().to_string();

    let has_eval = template_dir.join("eval.md").exists();

    Some(Template {
        name,
        path: template_dir.to_path_buf(),
        description,
        has_eval,
    })
}

/// Get a specific template by name.
pub fn get_template(name: &str) -> Option<Template> {
    let template_dir = get_templates_dir().join(name);
    let spec_file = template_dir.join("spec.md");

    if spec_file.exists() {
        parse_template(&template_dir, &spec_file)
    } else {
        None
    }
}

/// Read the spec content from a template.
pub fn read_template_spec(template: &Template) -> Result<String, TemplateError> {
    let spec_path = template.path.join("spec.md");
    std::fs::read_to_string(&spec_path).map_err(|e| TemplateError::ReadError {
        name: template.name.clone(),
        reason: e.to_string(),
    })
}

/// Read the eval content from a template, if it exists.
pub fn read_template_eval(template: &Template) -> Result<Option<String>, TemplateError> {
    if !template.has_eval {
        return Ok(None);
    }

    let eval_path = template.path.join("eval.md");
    let content =
        std::fs::read_to_string(&eval_path).map_err(|e| TemplateError::ReadError {
            name: template.name.clone(),
            reason: e.to_string(),
        })?;

    Ok(Some(content))
}

/// Execute the `hirsel templates` command.
///
/// Returns a formatted string listing all templates, or JSON if requested.
pub fn run_templates(json_output: bool) -> Result<String, TemplateError> {
    let templates = list_templates();

    if json_output {
        return serde_json::to_string_pretty(&serde_json::json!({
            "templates": templates
        }))
        .map_err(|e| TemplateError::SerializationError(e.to_string()));
    }

    if templates.is_empty() {
        return Ok("No templates found\n\nCreate templates in ~/.hirsel/templates/<name>/spec.md".to_string());
    }

    let mut output = String::from("Available Templates\n\n");

    for t in &templates {
        let eval_marker = if t.has_eval { " (+eval)" } else { "" };
        output.push_str(&format!("  {}{}\n", t.name, eval_marker));
        output.push_str(&format!("    {}\n", t.description));
        output.push_str(&format!("    path: {}\n\n", t.path.display()));
    }

    output.push_str("Usage: hirsel go my-run <template-name>\n");

    Ok(output)
}

/// Errors that can occur during template operations.
#[derive(Debug, thiserror::Error)]
pub enum TemplateError {
    #[error("Template '{name}' not found")]
    NotFound { name: String },

    #[error("Failed to read template '{name}': {reason}")]
    ReadError { name: String, reason: String },

    #[error("Failed to serialize templates: {0}")]
    SerializationError(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_template(dir: &Path, name: &str, has_eval: bool) {
        let template_dir = dir.join(name);
        std::fs::create_dir_all(&template_dir).unwrap();

        let spec_content = format!("# {} Template\n\nThis is the {} template spec.", name, name);
        std::fs::write(template_dir.join("spec.md"), spec_content).unwrap();

        if has_eval {
            std::fs::write(template_dir.join("eval.md"), "# Eval\n\nRun tests.").unwrap();
        }
    }

    #[test]
    fn test_parse_template() {
        let tmp = TempDir::new().unwrap();
        create_test_template(tmp.path(), "basic", false);

        let template_dir = tmp.path().join("basic");
        let spec_file = template_dir.join("spec.md");

        let template = parse_template(&template_dir, &spec_file).unwrap();

        assert_eq!(template.name, "basic");
        assert_eq!(template.description, "basic Template");
        assert!(!template.has_eval);
    }

    #[test]
    fn test_parse_template_with_eval() {
        let tmp = TempDir::new().unwrap();
        create_test_template(tmp.path(), "with-eval", true);

        let template_dir = tmp.path().join("with-eval");
        let spec_file = template_dir.join("spec.md");

        let template = parse_template(&template_dir, &spec_file).unwrap();

        assert_eq!(template.name, "with-eval");
        assert!(template.has_eval);
    }

    #[test]
    fn test_read_template_spec() {
        let tmp = TempDir::new().unwrap();
        create_test_template(tmp.path(), "test", false);

        let template_dir = tmp.path().join("test");
        let spec_file = template_dir.join("spec.md");
        let template = parse_template(&template_dir, &spec_file).unwrap();

        let content = read_template_spec(&template).unwrap();
        assert!(content.contains("test Template"));
    }

    #[test]
    fn test_read_template_eval() {
        let tmp = TempDir::new().unwrap();
        create_test_template(tmp.path(), "test", true);

        let template_dir = tmp.path().join("test");
        let spec_file = template_dir.join("spec.md");
        let template = parse_template(&template_dir, &spec_file).unwrap();

        let eval = read_template_eval(&template).unwrap();
        assert!(eval.is_some());
        assert!(eval.unwrap().contains("Run tests"));
    }

    #[test]
    fn test_read_template_eval_none() {
        let tmp = TempDir::new().unwrap();
        create_test_template(tmp.path(), "test", false);

        let template_dir = tmp.path().join("test");
        let spec_file = template_dir.join("spec.md");
        let template = parse_template(&template_dir, &spec_file).unwrap();

        let eval = read_template_eval(&template).unwrap();
        assert!(eval.is_none());
    }

    #[test]
    fn test_run_templates_empty() {
        // This would normally scan ~/.hirsel/templates which may not exist
        // For a proper test, we'd need to mock get_templates_dir()
        let result = run_templates(false);
        // Result should be Ok regardless of whether templates exist
        assert!(result.is_ok());
    }

    #[test]
    fn test_template_serialization() {
        let template = Template {
            name: "test".to_string(),
            path: PathBuf::from("/home/user/.hirsel/templates/test"),
            description: "Test Template".to_string(),
            has_eval: true,
        };

        let json = serde_json::to_string(&template).unwrap();
        assert!(json.contains("\"name\":\"test\""));
        assert!(json.contains("\"has_eval\":true"));
    }
}
