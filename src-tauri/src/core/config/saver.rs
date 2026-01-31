//! Configuration file saving and TOML generation.

use std::fs;
use std::path::Path;

use super::{
    AuthMethod, Config, ConfigError, GitProvider, OrchestratorAccess, OrchestratorMode, S3Config,
    StorageBackend, StorageProvider,
};
use crate::core::runner::{HostConfig, HostConfigOrShortcut};

/// Save the configuration to a TOML file.
///
/// This serializes the config back to TOML format and writes it to disk.
/// Note: Comments in the original file will be lost.
pub fn save_config(config: &Config, config_path: &Path) -> Result<(), ConfigError> {
    let mut output = String::new();

    // Agent section
    output.push_str("[agent]\n");
    let cmd_parts: Vec<String> = config
        .agent
        .command
        .iter()
        .map(|s| format!("\"{}\"", s))
        .collect();
    output.push_str(&format!("command = [{}]\n\n", cmd_parts.join(", ")));

    // Top-level settings
    output.push_str(&format!("eval_timeout = {}\n", config.eval_timeout));
    output.push_str(&format!("auto_learn = {}\n", config.auto_learn));
    output.push_str(&format!(
        "user_message_pause = \"{}\"\n",
        config.user_message_pause
    ));
    output.push_str(&format!(
        "human_in_the_loop = {}\n",
        config.human_in_the_loop
    ));
    output.push_str(&format!(
        "context_warning_threshold = {}\n",
        config.context_warning_threshold
    ));
    output.push_str(&format!("coordinator_port = {}\n", config.coordinator_port));
    if let Some(ref runner) = config.default_runner {
        output.push_str(&format!("default_runner = \"{}\"\n", runner));
    }
    output.push_str(&format!(
        "default_profile = \"{}\"\n",
        config.default_profile
    ));
    output.push('\n');

    // Auth section
    write_auth_section(&mut output, config);

    // Runners section
    write_runners_section(&mut output, config);

    // Profiles section
    write_profiles_section(&mut output, config);

    // Git section
    if let Some(ref provider) = config.git.default_provider {
        output.push_str("[git]\n");
        output.push_str(&format!(
            "default_provider = \"{}\"\n",
            match provider {
                GitProvider::Github => "github",
            }
        ));
        output.push('\n');
    }

    // Storage section (only write if non-default)
    write_storage_section(&mut output, config);

    // Scribe docs settings (only write if non-default)
    if config.scribe_docs_path != "docs" || !config.scribe_persist_docs_changes {
        output.push_str(&format!(
            "scribe_docs_path = \"{}\"\n",
            config.scribe_docs_path
        ));
        output.push_str(&format!(
            "scribe_persist_docs_changes = {}\n",
            config.scribe_persist_docs_changes
        ));
        output.push('\n');
    }

    // Service workers section (only write if configured)
    write_service_workers_section(&mut output, config);

    // Ensure parent directory exists
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent).map_err(|e| ConfigError::ReadError {
            path: parent.to_path_buf(),
            message: e.to_string(),
        })?;
    }

    // Write config file
    fs::write(config_path, output).map_err(|e| {
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            ConfigError::PermissionDenied {
                path: config_path.to_path_buf(),
            }
        } else {
            ConfigError::ReadError {
                path: config_path.to_path_buf(),
                message: e.to_string(),
            }
        }
    })?;

    Ok(())
}

fn write_auth_section(output: &mut String, config: &Config) {
    output.push_str("[auth]\n");
    output.push_str(&format!(
        "default_method = \"{}\"\n",
        match config.auth.default_method {
            AuthMethod::Env => "env",
            AuthMethod::ApiKey => "api_key",
            AuthMethod::OAuth => "oauth",
        }
    ));

    // Agent-specific auth
    for (name, auth) in [
        ("claude", &config.auth.claude),
        ("gemini", &config.auth.gemini),
        ("codex", &config.auth.codex),
        ("goose", &config.auth.goose),
    ] {
        if let Some(agent_auth) = auth {
            output.push_str(&format!("\n[auth.{}]\n", name));
            output.push_str(&format!(
                "method = \"{}\"\n",
                match agent_auth.method {
                    AuthMethod::Env => "env",
                    AuthMethod::ApiKey => "api_key",
                    AuthMethod::OAuth => "oauth",
                }
            ));
            if let Some(ref key) = agent_auth.api_key {
                output.push_str(&format!("api_key = \"{}\"\n", key));
            }
            if let Some(ref var) = agent_auth.env_var {
                output.push_str(&format!("env_var = \"{}\"\n", var));
            }
        }
    }
    output.push('\n');
}

fn write_runners_section(output: &mut String, config: &Config) {
    for (name, runner_config) in &config.runners {
        output.push_str(&format!("[runners.{}]\n", name));

        // Serialize host configuration
        let host = runner_config.host.resolve();
        match &runner_config.host {
            HostConfigOrShortcut::Shortcut(s) => {
                output.push_str(&format!("host = \"{}\"\n", s));
            }
            HostConfigOrShortcut::Full(_) => {
                // Full host config needs a sub-table
                match &host {
                    HostConfig::Local => {
                        output.push_str(&format!("\n[runners.{}.host]\n", name));
                        output.push_str("type = \"local\"\n");
                    }
                    HostConfig::Client => {
                        output.push_str(&format!("\n[runners.{}.host]\n", name));
                        output.push_str("type = \"client\"\n");
                    }
                    HostConfig::Ssh(ssh) => {
                        output.push_str(&format!("\n[runners.{}.host]\n", name));
                        output.push_str("type = \"ssh\"\n");
                        output.push_str(&format!("address = \"{}\"\n", ssh.address));
                        if ssh.port != 22 {
                            output.push_str(&format!("port = {}\n", ssh.port));
                        }
                        if let Some(ref key) = ssh.ssh_key {
                            output.push_str(&format!("ssh_key = \"{}\"\n", key));
                        }
                        output.push_str(&format!("work_base = \"{}\"\n", ssh.work_base));
                        if let Some(ref loc) = ssh.location {
                            output.push_str(&format!("location = \"{}\"\n", loc));
                        }
                    }
                    HostConfig::Fly(fly) => {
                        output.push_str(&format!("\n[runners.{}.host]\n", name));
                        output.push_str("type = \"fly\"\n");
                        if let Some(ref token) = fly.api_token {
                            output.push_str(&format!("api_token = \"{}\"\n", token));
                        }
                        output.push_str(&format!("app = \"{}\"\n", fly.app));
                        if let Some(ref region) = fly.region {
                            output.push_str(&format!("region = \"{}\"\n", region));
                        }
                        if fly.cpu_kind != "shared" {
                            output.push_str(&format!("cpu_kind = \"{}\"\n", fly.cpu_kind));
                        }
                        if fly.cpus != 1 {
                            output.push_str(&format!("cpus = {}\n", fly.cpus));
                        }
                        if fly.memory_mb != 1024 {
                            output.push_str(&format!("memory_mb = {}\n", fly.memory_mb));
                        }
                        output.push_str(&format!("auto_destroy = {}\n", fly.auto_destroy));
                    }
                }
            }
        }

        // Serialize container configuration if present
        if let Some(ref container) = runner_config.container {
            output.push_str(&format!("\n[runners.{}.container]\n", name));
            output.push_str(&format!("image = \"{}\"\n", container.image));
        }

        output.push('\n');
    }
}

fn write_profiles_section(output: &mut String, config: &Config) {
    for (name, profile) in &config.profiles {
        output.push_str(&format!("[profiles.{}]\n", name));
        output.push_str(&format!(
            "mode = \"{}\"\n",
            match profile.mode {
                OrchestratorMode::Local => "local",
                OrchestratorMode::Remote => "remote",
            }
        ));
        if let Some(ref url) = profile.url {
            output.push_str(&format!("url = \"{}\"\n", url));
        }
        if let Some(ref key) = profile.api_key {
            output.push_str(&format!("api_key = \"{}\"\n", key));
        }

        // Access sub-section
        match &profile.access {
            OrchestratorAccess::Direct => {
                output.push_str("\n[profiles.");
                output.push_str(name);
                output.push_str(".access]\n");
                output.push_str("type = \"direct\"\n");
            }
            OrchestratorAccess::Tailscale {
                oauth_client_id,
                oauth_client_secret,
                tag,
            } => {
                output.push_str("\n[profiles.");
                output.push_str(name);
                output.push_str(".access]\n");
                output.push_str("type = \"tailscale\"\n");
                output.push_str(&format!("oauth_client_id = \"{}\"\n", oauth_client_id));
                output.push_str(&format!(
                    "oauth_client_secret = \"{}\"\n",
                    oauth_client_secret
                ));
                if let Some(ref t) = tag {
                    output.push_str(&format!("tag = \"{}\"\n", t));
                }
            }
        }
        output.push('\n');
    }
}

fn write_s3_config(output: &mut String, s3: &S3Config) {
    // Write provider if not default (S3)
    if s3.provider != StorageProvider::S3 {
        output.push_str(&format!(
            "provider = \"{}\"\n",
            match s3.provider {
                StorageProvider::S3 => "s3",
                StorageProvider::Tigris => "tigris",
            }
        ));
    }
    if let Some(ref endpoint) = s3.endpoint {
        output.push_str(&format!("endpoint = \"{}\"\n", endpoint));
    }
    output.push_str(&format!("bucket = \"{}\"\n", s3.bucket));
    if let Some(ref region) = s3.region {
        output.push_str(&format!("region = \"{}\"\n", region));
    }
    if let Some(ref key) = s3.access_key_id {
        output.push_str(&format!("access_key_id = \"{}\"\n", key));
    }
    if let Some(ref secret) = s3.secret_access_key {
        output.push_str(&format!("secret_access_key = \"{}\"\n", secret));
    }
}

fn write_storage_section(output: &mut String, config: &Config) {
    let has_content = config.storage.files != StorageBackend::Local
        || !config.storage.storages.is_empty()
        || config.storage.default_storage.is_some();

    if has_content {
        output.push_str("[storage]\n");
        output.push_str(&format!(
            "files = \"{}\"\n",
            match config.storage.files {
                StorageBackend::Local => "local",
                StorageBackend::S3 => "s3",
            }
        ));

        if let Some(ref default_storage) = config.storage.default_storage {
            output.push_str(&format!("default_storage = \"{}\"\n", default_storage));
        }

        // Write named storages
        for (name, s3) in &config.storage.storages {
            output.push_str(&format!("\n[storage.storages.{}]\n", name));
            write_s3_config(output, s3);
        }

        output.push('\n');
    }
}

fn write_service_workers_section(output: &mut String, config: &Config) {
    let sw = &config.service_workers;
    let has_content = sw.runner.is_some()
        || sw.scribe.runner.is_some()
        || sw.scribe.idle_timeout_seconds.is_some()
        || sw.gyp.runner.is_some()
        || sw.gyp.idle_timeout_seconds.is_some();

    if has_content {
        output.push_str("[service_workers]\n");

        if let Some(ref runner) = sw.runner {
            output.push_str(&format!("runner = \"{}\"\n", runner));
        }

        // Scribe section
        if sw.scribe.runner.is_some() || sw.scribe.idle_timeout_seconds.is_some() {
            output.push_str("\n[service_workers.scribe]\n");
            if let Some(ref runner) = sw.scribe.runner {
                output.push_str(&format!("runner = \"{}\"\n", runner));
            }
            if let Some(timeout) = sw.scribe.idle_timeout_seconds {
                output.push_str(&format!("idle_timeout_seconds = {}\n", timeout));
            }
        }

        // Gyp section
        if sw.gyp.runner.is_some() || sw.gyp.idle_timeout_seconds.is_some() {
            output.push_str("\n[service_workers.gyp]\n");
            if let Some(ref runner) = sw.gyp.runner {
                output.push_str(&format!("runner = \"{}\"\n", runner));
            }
            if let Some(timeout) = sw.gyp.idle_timeout_seconds {
                output.push_str(&format!("idle_timeout_seconds = {}\n", timeout));
            }
        }

        output.push('\n');
    }
}
