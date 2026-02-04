//! ACP Client for the Conflict Resolver Agent
//!
//! This client allows the agent to:
//! - Read files in the work directory
//! - Write files in the work directory
//! - Execute terminal commands (git add, git status)
//!
//! The agent needs these capabilities to resolve conflicts and stage the resolved files.

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use agent_client_protocol::{
    Client, CreateTerminalRequest, CreateTerminalResponse, KillTerminalCommandRequest,
    KillTerminalCommandResponse, PermissionOptionKind, ReadTextFileRequest, ReadTextFileResponse,
    ReleaseTerminalRequest, ReleaseTerminalResponse, RequestPermissionOutcome,
    RequestPermissionRequest, RequestPermissionResponse, SelectedPermissionOutcome,
    SessionNotification, TerminalExitStatus, TerminalId, TerminalOutputRequest,
    TerminalOutputResponse, WaitForTerminalExitRequest, WaitForTerminalExitResponse,
    WriteTextFileRequest, WriteTextFileResponse,
};

/// Result type for ACP operations
type AcpResult<T> = std::result::Result<T, agent_client_protocol::Error>;

/// ACP client for conflict resolver agent
///
/// Allows file operations and git commands within the work directory.
pub struct ConflictResolverClient {
    work_dir: PathBuf,
    next_terminal_id: AtomicU64,
    terminals: Mutex<std::collections::HashMap<TerminalId, TerminalState>>,
}

struct TerminalState {
    child: std::process::Child,
}

impl ConflictResolverClient {
    pub fn new(work_dir: PathBuf) -> Self {
        Self {
            work_dir,
            next_terminal_id: AtomicU64::new(1),
            terminals: Mutex::new(std::collections::HashMap::new()),
        }
    }

    /// Check if a path is within the work directory (security check)
    fn is_path_allowed(&self, path: &std::path::Path) -> bool {
        // Allow absolute paths within work_dir
        if path.is_absolute() {
            return path.starts_with(&self.work_dir);
        }

        // Allow relative paths (they're relative to work_dir)
        true
    }

    /// Resolve a path to an absolute path within work_dir
    fn resolve_path(&self, path: &std::path::Path) -> PathBuf {
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.work_dir.join(path)
        }
    }
}

#[async_trait::async_trait(?Send)]
impl Client for ConflictResolverClient {
    async fn request_permission(
        &self,
        args: RequestPermissionRequest,
    ) -> AcpResult<RequestPermissionResponse> {
        // Auto-approve all permission requests
        let option_id = args
            .options
            .iter()
            .find(|o| o.kind == PermissionOptionKind::AllowAlways)
            .or_else(|| {
                args.options
                    .iter()
                    .find(|o| o.kind == PermissionOptionKind::AllowOnce)
            })
            .map(|o| o.option_id.clone())
            .unwrap_or_else(|| "allow".into());

        Ok(RequestPermissionResponse::new(
            RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(option_id)),
        ))
    }

    async fn session_notification(&self, _args: SessionNotification) -> AcpResult<()> {
        // Accept all notifications silently
        Ok(())
    }

    async fn read_text_file(&self, args: ReadTextFileRequest) -> AcpResult<ReadTextFileResponse> {
        if !self.is_path_allowed(&args.path) {
            return Err(agent_client_protocol::Error::internal_error());
        }

        let path = self.resolve_path(&args.path);
        match std::fs::read_to_string(&path) {
            Ok(mut content) => {
                if args.line.is_some() || args.limit.is_some() {
                    let lines: Vec<&str> = content.lines().collect();
                    let start = args.line.map(|l| l as usize).unwrap_or(0);
                    let end = args
                        .limit
                        .map(|l| start + l as usize)
                        .unwrap_or(lines.len());
                    content = lines[start.min(lines.len())..end.min(lines.len())].join("\n");
                }
                Ok(ReadTextFileResponse::new(content))
            }
            Err(_) => Err(agent_client_protocol::Error::internal_error()),
        }
    }

    async fn write_text_file(
        &self,
        args: WriteTextFileRequest,
    ) -> AcpResult<WriteTextFileResponse> {
        if !self.is_path_allowed(&args.path) {
            return Err(agent_client_protocol::Error::internal_error());
        }

        let path = self.resolve_path(&args.path);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match std::fs::write(&path, &args.content) {
            Ok(()) => Ok(WriteTextFileResponse::new()),
            Err(_) => Err(agent_client_protocol::Error::internal_error()),
        }
    }

    async fn create_terminal(
        &self,
        args: CreateTerminalRequest,
    ) -> AcpResult<CreateTerminalResponse> {
        // Only allow certain commands for security
        let command = &args.command;
        let allowed = command.starts_with("git ")
            || command.starts_with("ls ")
            || command.starts_with("cat ")
            || command.starts_with("head ")
            || command.starts_with("tail ")
            || command.starts_with("grep ");

        if !allowed {
            tracing::warn!("Conflict resolver denied command: {}", command);
            return Err(agent_client_protocol::Error::internal_error());
        }

        let terminal_id = TerminalId::new(format!(
            "terminal-{}",
            self.next_terminal_id.fetch_add(1, Ordering::SeqCst)
        ));

        // Spawn the command
        let child = std::process::Command::new("sh")
            .args(["-c", command])
            .current_dir(&self.work_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|_| agent_client_protocol::Error::internal_error())?;

        // Store terminal state
        let mut terminals = self.terminals.lock().unwrap();
        terminals.insert(terminal_id.clone(), TerminalState { child });

        Ok(CreateTerminalResponse::new(terminal_id))
    }

    async fn terminal_output(
        &self,
        args: TerminalOutputRequest,
    ) -> AcpResult<TerminalOutputResponse> {
        let mut terminals = self.terminals.lock().unwrap();
        let terminal = terminals
            .get_mut(&args.terminal_id)
            .ok_or_else(agent_client_protocol::Error::internal_error)?;

        // Try to get output if process has finished
        let output = match terminal.child.try_wait() {
            Ok(Some(_status)) => {
                // Process finished - read remaining output
                
                terminal
                    .child
                    .stdout
                    .take()
                    .and_then(|mut s| {
                        use std::io::Read;
                        let mut buf = String::new();
                        s.read_to_string(&mut buf).ok().map(|_| buf)
                    })
                    .unwrap_or_default()
            }
            Ok(None) => {
                // Still running - no output yet
                String::new()
            }
            Err(_) => String::new(),
        };

        Ok(TerminalOutputResponse::new(output, false))
    }

    async fn release_terminal(
        &self,
        args: ReleaseTerminalRequest,
    ) -> AcpResult<ReleaseTerminalResponse> {
        let mut terminals = self.terminals.lock().unwrap();
        if let Some(mut terminal) = terminals.remove(&args.terminal_id) {
            let _ = terminal.child.kill();
        }
        Ok(ReleaseTerminalResponse::new())
    }

    async fn wait_for_terminal_exit(
        &self,
        args: WaitForTerminalExitRequest,
    ) -> AcpResult<WaitForTerminalExitResponse> {
        let mut terminals = self.terminals.lock().unwrap();
        let terminal = terminals
            .get_mut(&args.terminal_id)
            .ok_or_else(agent_client_protocol::Error::internal_error)?;

        let exit_status = match terminal.child.wait() {
            Ok(status) => TerminalExitStatus::new().exit_code(status.code().map(|c| c as u32)),
            Err(_) => TerminalExitStatus::new(),
        };

        Ok(WaitForTerminalExitResponse::new(exit_status))
    }

    async fn kill_terminal_command(
        &self,
        args: KillTerminalCommandRequest,
    ) -> AcpResult<KillTerminalCommandResponse> {
        let mut terminals = self.terminals.lock().unwrap();
        if let Some(terminal) = terminals.get_mut(&args.terminal_id) {
            let _ = terminal.child.kill();
        }
        Ok(KillTerminalCommandResponse::new())
    }
}
