//! Authentication and agent type definitions.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;
use std::fs;

use super::ConfigError;

/// Type of AI agent
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AgentType {
    Claude,
    Gemini,
    Codex,
    Goose,
    #[default]
    Unknown,
}

impl AgentType {
    /// Detect agent type from command
    pub fn from_command(command: &[String]) -> Self {
        if command.is_empty() {
            return Self::Unknown;
        }
        // Check full command for identification (handles "hirsel __acp-bridge" etc.)
        let full_cmd = command.join(" ").to_lowercase();
        if full_cmd.contains("claude") || full_cmd.contains("__acp-bridge") {
            Self::Claude
        } else if full_cmd.contains("gemini") {
            Self::Gemini
        } else if full_cmd.contains("codex") {
            Self::Codex
        } else if full_cmd.contains("goose") {
            Self::Goose
        } else {
            Self::Unknown
        }
    }

    /// Get the primary environment variable name for this agent type
    pub fn default_env_var(&self) -> Option<&'static str> {
        match self {
            Self::Claude => Some("ANTHROPIC_API_KEY"),
            Self::Gemini => Some("GOOGLE_API_KEY"),
            Self::Codex => Some("OPENAI_API_KEY"),
            Self::Goose => Some("ANTHROPIC_API_KEY"),
            Self::Unknown => None,
        }
    }

    /// Whether this agent type supports context tracking
    pub fn supports_context_tracking(&self) -> bool {
        matches!(self, Self::Claude)
    }
}

impl std::fmt::Display for AgentType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Claude => write!(f, "claude"),
            Self::Gemini => write!(f, "gemini"),
            Self::Codex => write!(f, "codex"),
            Self::Goose => write!(f, "goose"),
            Self::Unknown => write!(f, "unknown"),
        }
    }
}

/// Authentication method for agents
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AuthMethod {
    Env,
    ApiKey,
    #[default]
    OAuth,
}

impl std::str::FromStr for AuthMethod {
    type Err = ConfigError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "env" => Ok(Self::Env),
            "api_key" => Ok(Self::ApiKey),
            "oauth" => Ok(Self::OAuth),
            _ => Err(ConfigError::ValidationError(format!(
                "Invalid auth method: '{}'. Must be one of: env, api_key, oauth",
                s
            ))),
        }
    }
}

/// Authentication configuration for a specific agent type
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentAuth {
    #[serde(default)]
    pub method: AuthMethod,
    pub api_key: Option<String>,
    pub env_var: Option<String>,
}

impl AgentAuth {
    /// Get credentials as environment variables to forward
    pub fn get_credentials(&self, agent_type: AgentType) -> HashMap<String, String> {
        let mut result = HashMap::new();

        match self.method {
            AuthMethod::ApiKey => {
                if let Some(ref api_key) = self.api_key {
                    let env_var = self
                        .env_var
                        .as_deref()
                        .or_else(|| agent_type.default_env_var());
                    if let Some(var) = env_var {
                        result.insert(var.to_string(), api_key.clone());
                    }
                }
            }
            AuthMethod::Env => {
                let env_var = self
                    .env_var
                    .as_deref()
                    .or_else(|| agent_type.default_env_var());
                if let Some(var) = env_var {
                    if let Ok(value) = env::var(var) {
                        result.insert(var.to_string(), value);
                    }
                }
            }
            AuthMethod::OAuth => {
                if agent_type != AgentType::Claude {
                    return result;
                }
                if let Some(creds) = get_claude_oauth_credentials() {
                    result.extend(creds);
                }
            }
        }

        result
    }
}

/// Read Claude OAuth credentials from ~/.claude/.credentials.json
fn get_claude_oauth_credentials() -> Option<HashMap<String, String>> {
    let credentials_path = dirs::home_dir()?.join(".claude").join(".credentials.json");
    if !credentials_path.exists() {
        return None;
    }

    let content = fs::read_to_string(&credentials_path).ok()?;
    let data: serde_json::Value = serde_json::from_str(&content).ok()?;

    let access_token = data
        .get("claudeAiOauth")?
        .get("accessToken")?
        .as_str()?
        .to_string();

    let mut result = HashMap::new();
    result.insert("CLAUDE_ACCESS_TOKEN".to_string(), access_token);
    Some(result)
}

/// Authentication configuration for all agent types
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuthConfig {
    pub claude: Option<AgentAuth>,
    pub gemini: Option<AgentAuth>,
    pub codex: Option<AgentAuth>,
    pub goose: Option<AgentAuth>,
    #[serde(default)]
    pub default_method: AuthMethod,
}

impl AuthConfig {
    /// Get auth config for a specific agent type
    pub fn get_auth_for(&self, agent_type: AgentType) -> AgentAuth {
        match agent_type {
            AgentType::Claude => self.claude.clone().unwrap_or(AgentAuth {
                method: self.default_method,
                ..Default::default()
            }),
            AgentType::Gemini => self.gemini.clone().unwrap_or(AgentAuth {
                method: self.default_method,
                ..Default::default()
            }),
            AgentType::Codex => self.codex.clone().unwrap_or(AgentAuth {
                method: self.default_method,
                ..Default::default()
            }),
            AgentType::Goose => self.goose.clone().unwrap_or(AgentAuth {
                method: self.default_method,
                ..Default::default()
            }),
            AgentType::Unknown => AgentAuth {
                method: self.default_method,
                ..Default::default()
            },
        }
    }
}

/// Get credentials for an agent type as environment variables
pub fn get_agent_env_vars(
    agent_type: AgentType,
    auth_config: &AuthConfig,
) -> HashMap<String, String> {
    let agent_auth = auth_config.get_auth_for(agent_type);
    agent_auth.get_credentials(agent_type)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_type_from_command() {
        // Claude variants
        assert_eq!(
            AgentType::from_command(&["claude-code-acp".to_string()]),
            AgentType::Claude
        );
        assert_eq!(
            AgentType::from_command(&["hirsel".to_string(), "__acp-bridge".to_string()]),
            AgentType::Claude
        );
        // Other agents
        assert_eq!(
            AgentType::from_command(&["gemini".to_string()]),
            AgentType::Gemini
        );
        assert_eq!(
            AgentType::from_command(&["codex".to_string()]),
            AgentType::Codex
        );
        assert_eq!(
            AgentType::from_command(&["goose".to_string()]),
            AgentType::Goose
        );
        assert_eq!(AgentType::from_command(&[]), AgentType::Unknown);
    }
}
