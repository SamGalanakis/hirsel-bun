//! Shell completions for hirsel CLI.
//!
//! Generates and installs shell completion scripts for bash, zsh, and fish.

use crate::cli::{Cli, CompletionsArgs, WorkerCli};
use clap::CommandFactory;
use clap_complete::{generate, Shell};
use std::fs;
use std::path::PathBuf;

/// Detect the current shell from environment
fn detect_shell() -> Option<Shell> {
    std::env::var("SHELL")
        .ok()
        .and_then(|s| {
            let shell_name = PathBuf::from(&s)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())?;

            match shell_name.as_str() {
                "bash" => Some(Shell::Bash),
                "zsh" => Some(Shell::Zsh),
                "fish" => Some(Shell::Fish),
                "elvish" => Some(Shell::Elvish),
                "powershell" | "pwsh" => Some(Shell::PowerShell),
                _ => None,
            }
        })
}

/// Get the completion file path for a given shell
fn get_completion_path(shell: Shell, command_name: &str) -> Option<PathBuf> {
    let home = dirs::home_dir()?;

    match shell {
        Shell::Bash => {
            // Try ~/.local/share/bash-completion/completions/ first
            let local = home.join(".local/share/bash-completion/completions");
            if local.exists() || fs::create_dir_all(&local).is_ok() {
                return Some(local.join(command_name));
            }
            // Fall back to ~/.bash_completion.d/
            let fallback = home.join(".bash_completion.d");
            fs::create_dir_all(&fallback).ok()?;
            Some(fallback.join(command_name))
        }
        Shell::Zsh => {
            // ~/.zfunc/ is a common location
            let zfunc = home.join(".zfunc");
            fs::create_dir_all(&zfunc).ok()?;
            Some(zfunc.join(format!("_{}", command_name)))
        }
        Shell::Fish => {
            let fish_completions = home.join(".config/fish/completions");
            fs::create_dir_all(&fish_completions).ok()?;
            Some(fish_completions.join(format!("{}.fish", command_name)))
        }
        Shell::Elvish => {
            let elvish = home.join(".elvish/lib");
            fs::create_dir_all(&elvish).ok()?;
            Some(elvish.join(format!("{}.elv", command_name)))
        }
        Shell::PowerShell => {
            // PowerShell completions go in the profile directory
            let ps = home.join("Documents/PowerShell");
            fs::create_dir_all(&ps).ok()?;
            Some(ps.join(format!("{}_completion.ps1", command_name)))
        }
        _ => None,
    }
}

/// Generate completion script for a shell
pub fn generate_completions(shell: Shell, for_worker: bool) -> String {
    let mut buf = Vec::new();

    if for_worker {
        let mut cmd = WorkerCli::command();
        generate(shell, &mut cmd, "hirsel-worker", &mut buf);
    } else {
        let mut cmd = Cli::command();
        generate(shell, &mut cmd, "hirsel", &mut buf);
    }

    String::from_utf8(buf).unwrap_or_default()
}

/// Parse shell name string to Shell enum
fn parse_shell(name: &str) -> Option<Shell> {
    match name.to_lowercase().as_str() {
        "bash" => Some(Shell::Bash),
        "zsh" => Some(Shell::Zsh),
        "fish" => Some(Shell::Fish),
        "elvish" => Some(Shell::Elvish),
        "powershell" | "pwsh" => Some(Shell::PowerShell),
        _ => None,
    }
}

/// Install completions for the detected shell
pub fn run_completions(args: &CompletionsArgs) -> anyhow::Result<()> {
    // If a shell argument is provided, print completions to stdout and exit
    if let Some(shell_name) = &args.shell {
        let shell = parse_shell(shell_name).ok_or_else(|| {
            anyhow::anyhow!(
                "Unknown shell '{}'. Supported: bash, zsh, fish, elvish, powershell",
                shell_name
            )
        })?;
        print_completions(shell, false);
        return Ok(());
    }

    // Otherwise, detect shell and install completions
    let shell = detect_shell().ok_or_else(|| {
        anyhow::anyhow!(
            "Could not detect shell. Set $SHELL or specify shell: hirsel completions bash"
        )
    })?;

    println!("Detected shell: {:?}", shell);

    // Install completions for both hirsel and hirsel-worker
    for (name, for_worker) in [("hirsel", false), ("hirsel-worker", true)] {
        let path = get_completion_path(shell, name)
            .ok_or_else(|| anyhow::anyhow!("Could not determine completion path for {}", name))?;

        // Check if already exists and not forcing
        if path.exists() && !args.force {
            println!("Completions already exist at: {}", path.display());
            println!("Use --force to overwrite.");
            continue;
        }

        let completions = generate_completions(shell, for_worker);
        fs::write(&path, &completions)?;
        println!("Installed {} completions to: {}", name, path.display());
    }

    // Print shell-specific instructions
    match shell {
        Shell::Bash => {
            println!("\nTo enable completions, add to your ~/.bashrc:");
            println!("  source ~/.local/share/bash-completion/completions/hirsel");
            println!("  source ~/.local/share/bash-completion/completions/hirsel-worker");
            println!("\nOr restart your shell.");
        }
        Shell::Zsh => {
            println!("\nTo enable completions, add to your ~/.zshrc:");
            println!("  fpath=(~/.zfunc $fpath)");
            println!("  autoload -Uz compinit && compinit");
            println!("\nThen restart your shell or run: compinit");
        }
        Shell::Fish => {
            println!("\nFish completions are automatically loaded from:");
            println!("  ~/.config/fish/completions/");
            println!("\nRestart your shell or run: source ~/.config/fish/config.fish");
        }
        _ => {
            println!("\nCompletions installed. Restart your shell to enable.");
        }
    }

    Ok(())
}

/// Print completions to stdout (for manual installation)
pub fn print_completions(shell: Shell, for_worker: bool) {
    let completions = generate_completions(shell, for_worker);
    print!("{}", completions);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_bash_completions() {
        let completions = generate_completions(Shell::Bash, false);
        assert!(completions.contains("hirsel"));
        assert!(completions.contains("go"));
        assert!(completions.contains("view"));
    }

    #[test]
    fn test_generate_worker_completions() {
        let completions = generate_completions(Shell::Bash, true);
        assert!(completions.contains("hirsel-worker"));
        assert!(completions.contains("task"));
        assert!(completions.contains("msg"));
    }

    #[test]
    fn test_generate_zsh_completions() {
        let completions = generate_completions(Shell::Zsh, false);
        assert!(completions.contains("hirsel"));
        // Zsh completions have different format
        assert!(completions.contains("compdef") || completions.contains("_hirsel"));
    }

    #[test]
    fn test_generate_fish_completions() {
        let completions = generate_completions(Shell::Fish, false);
        assert!(completions.contains("hirsel"));
        assert!(completions.contains("complete"));
    }
}
