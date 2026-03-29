//! Eval script parsing and detection.

use std::fs;
use std::path::Path;

/// Parse eval script content to find test commands
pub fn parse_eval_script(content: &str) -> Vec<String> {
    // Look for common test command patterns
    let mut commands = Vec::new();

    for line in content.lines() {
        let line = line.trim();

        // Skip comments and empty lines
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // Common test commands
        if line.starts_with("pytest")
            || line.starts_with("npm test")
            || line.starts_with("cargo test")
            || line.starts_with("go test")
            || line.starts_with("make test")
            || line.starts_with("./")
        {
            commands.push(line.to_string());
        }
    }

    commands
}

/// Check if eval script exists for a run
pub fn has_eval_script(runtime_dir: &Path) -> bool {
    let eval_md = runtime_dir.join("eval.md");
    let eval_sh = runtime_dir.join("eval.sh");
    eval_md.exists() || eval_sh.exists()
}

/// Get eval script path for a run
pub fn get_eval_script(runtime_dir: &Path) -> Option<String> {
    let eval_sh = runtime_dir.join("eval.sh");
    if eval_sh.exists() {
        return Some(eval_sh.to_string_lossy().to_string());
    }

    let eval_md = runtime_dir.join("eval.md");
    if eval_md.exists() {
        // Extract script from markdown - look for code blocks
        if let Ok(content) = fs::read_to_string(&eval_md) {
            // Look for ```bash or ```sh code blocks
            let mut in_code_block = false;
            let mut script_lines = Vec::new();

            for line in content.lines() {
                if line.starts_with("```bash") || line.starts_with("```sh") {
                    in_code_block = true;
                    continue;
                }
                if line.starts_with("```") && in_code_block {
                    break;
                }
                if in_code_block {
                    script_lines.push(line);
                }
            }

            if !script_lines.is_empty() {
                // Write extracted script to temp file
                let temp_script = runtime_dir.join("tmp").join("eval_extracted.sh");
                if let Ok(()) = fs::create_dir_all(runtime_dir.join("tmp")) {
                    if let Ok(()) = fs::write(&temp_script, script_lines.join("\n")) {
                        return Some(temp_script.to_string_lossy().to_string());
                    }
                }
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_parse_eval_script() {
        let script = r#"
#!/bin/bash
# Run tests
pytest tests/
npm test
cargo test
"#;
        let commands = parse_eval_script(script);
        assert_eq!(commands.len(), 3);
        assert!(commands[0].starts_with("pytest"));
        assert!(commands[1].starts_with("npm test"));
        assert!(commands[2].starts_with("cargo test"));
    }

    #[test]
    fn test_has_eval_script() {
        let temp = TempDir::new().unwrap();
        assert!(!has_eval_script(temp.path()));

        // Create eval.md
        fs::write(temp.path().join("eval.md"), "# Eval").unwrap();
        assert!(has_eval_script(temp.path()));
    }

    #[test]
    fn test_get_eval_script_from_sh() {
        let temp = TempDir::new().unwrap();
        let script_path = temp.path().join("eval.sh");
        fs::write(&script_path, "#!/bin/bash\npytest").unwrap();

        let result = get_eval_script(temp.path());
        assert!(result.is_some());
        assert!(result.unwrap().contains("eval.sh"));
    }

    #[test]
    fn test_get_eval_script_from_md() {
        let temp = TempDir::new().unwrap();
        let md_content = r#"# Eval

Run the tests:

```bash
pytest tests/
```
"#;
        fs::write(temp.path().join("eval.md"), md_content).unwrap();

        let result = get_eval_script(temp.path());
        assert!(result.is_some());
    }
}
