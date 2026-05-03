use crate::core::config::{AgentState, BridgeConfig};
use crate::plugins::base::{AgentBridge, BridgeError, BridgeStatus};
use std::collections::HashMap;
use std::process::Command;

pub struct OmxBridge {
    config: BridgeConfig,
    available: bool,
    tmux_session: Option<String>,
}

impl OmxBridge {
    pub fn new() -> Self {
        Self {
            config: BridgeConfig {
                enabled: true,
                name: "oh-my-codex".to_string(),
                settings: HashMap::new(),
            },
            available: Self::check_available(),
            tmux_session: None,
        }
    }

    fn check_available() -> bool {
        crate::utils::which_command("tmux")
    }

    fn resolve_tmux_session_target(session_name: &str) -> Result<String, BridgeError> {
        let output = Command::new("tmux")
            .args(["list-sessions", "-F", "#{session_name}\t#{session_id}"])
            .output()
            .map_err(|e| {
                BridgeError::OperationFailed(format!("tmux list-sessions failed: {}", e))
            })?;

        if !output.status.success() {
            return Err(BridgeError::OperationFailed(
                "tmux list-sessions returned non-zero".into(),
            ));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        stdout
            .lines()
            .filter_map(|line| line.split_once('\t'))
            .find_map(|(name, id)| (name == session_name).then(|| id.to_string()))
            .ok_or_else(|| {
                BridgeError::OperationFailed(format!(
                    "Configured OMX tmux session '{}' does not exist",
                    session_name
                ))
            })
    }

    pub fn send_tmux_keys(&self, session: &str, keys: &str) -> Result<(), BridgeError> {
        let status = Command::new("tmux")
            .args(["send-keys", "-t", session, keys])
            .status()
            .map_err(|e| BridgeError::OperationFailed(format!("tmux send-keys failed: {}", e)))?;

        if !status.success() {
            return Err(BridgeError::OperationFailed(
                "tmux send-keys returned non-zero".into(),
            ));
        }
        Ok(())
    }

    pub fn send_tmux_text(&self, session: &str, text: &str) -> Result<(), BridgeError> {
        let status = Command::new("tmux")
            .args(["send-keys", "-t", session, "-l", "--", text])
            .status()
            .map_err(|e| {
                BridgeError::OperationFailed(format!("tmux send-keys literal failed: {}", e))
            })?;

        if !status.success() {
            return Err(BridgeError::OperationFailed(
                "tmux send-keys literal returned non-zero".into(),
            ));
        }
        Ok(())
    }

    pub fn send_tmux_enter(&self, session: &str) -> Result<(), BridgeError> {
        let status = Command::new("tmux")
            .args(["send-keys", "-t", session, "Enter"])
            .status()
            .map_err(|e| {
                BridgeError::OperationFailed(format!("tmux send-keys Enter failed: {}", e))
            })?;

        if !status.success() {
            return Err(BridgeError::OperationFailed(
                "tmux send-keys Enter returned non-zero".into(),
            ));
        }
        Ok(())
    }

    fn target_session(&self) -> Result<String, BridgeError> {
        let session = self.tmux_session.as_deref().ok_or_else(|| {
            BridgeError::OperationFailed(
                "OMX command injection requires an explicit 'tmux_session' bridge setting".into(),
            )
        })?;

        Self::resolve_tmux_session_target(session)
    }

    pub fn shutdown_team(&self, team_name: &str) -> Result<(), BridgeError> {
        Err(BridgeError::OperationFailed(format!(
            "OMX team shutdown is disabled until '{}' can be bound to an exact tracked session",
            team_name
        )))
    }

    pub fn resume_team(&self) -> Result<(), BridgeError> {
        Err(BridgeError::OperationFailed(
            "OMX team resume is disabled until it can target an exact tracked session".into(),
        ))
    }

    pub fn run_hook(&self, hook_name: &str) -> Result<(), BridgeError> {
        Err(BridgeError::OperationFailed(format!(
            "OMX hook '{}' is disabled until hooks are bound to an exact tracked session",
            hook_name
        )))
    }
}

impl AgentBridge for OmxBridge {
    fn name(&self) -> &str {
        "omx"
    }

    fn is_available(&self) -> bool {
        self.available && self.config.enabled
    }

    fn detect(&self) -> bool {
        if !self.available {
            return false;
        }
        self.target_session().is_ok()
    }

    fn send_command(&self, command: &str) -> Result<(), BridgeError> {
        let session = self.target_session()?;
        self.send_tmux_text(&session, command)?;
        self.send_tmux_enter(&session)?;
        Ok(())
    }

    fn send_keys(&self, keys: &str) -> Result<(), BridgeError> {
        let session = self.target_session()?;
        self.send_tmux_keys(&session, keys)
    }

    fn get_status(&self) -> Result<BridgeStatus, BridgeError> {
        Ok(BridgeStatus {
            agent_name: self.config.name.clone(),
            state: if self.detect() {
                AgentState::Running
            } else {
                AgentState::Stopped
            },
            pid: None,
            uptime_secs: 0,
            metadata: HashMap::new(),
        })
    }

    fn shutdown(&self) -> Result<(), BridgeError> {
        Err(BridgeError::OperationFailed(
            "OMX shutdown is disabled until the bridge can target an exact team/session".into(),
        ))
    }

    fn restart(&self) -> Result<(), BridgeError> {
        self.shutdown()?;
        std::thread::sleep(std::time::Duration::from_secs(1));
        self.resume_team()
    }

    fn configure(&mut self, config: &BridgeConfig) {
        self.config = config.clone();
        self.tmux_session = self
            .config
            .settings
            .get("tmux_session")
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        self.available = Self::check_available() && self.config.enabled;
    }
}

impl Default for OmxBridge {
    fn default() -> Self {
        Self::new()
    }
}
