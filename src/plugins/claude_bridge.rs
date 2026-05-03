use crate::core::config::{AgentState, BridgeConfig};
use crate::plugins::base::{AgentBridge, BridgeError, BridgeStatus};
use std::collections::HashMap;

pub struct ClaudeBridge {
    config: BridgeConfig,
    available: bool,
}

impl ClaudeBridge {
    pub fn new() -> Self {
        Self {
            config: BridgeConfig {
                enabled: false,
                name: "Claude Code".to_string(),
                settings: HashMap::new(),
            },
            available: Self::check_available(),
        }
    }

    fn check_available() -> bool {
        crate::utils::which_command("claude")
    }

    fn claude_process_pids(&self) -> Vec<u32> {
        let mut system = sysinfo::System::new_all();
        system.refresh_all();
        let mut pids = Vec::new();

        for (pid, process) in system.processes() {
            if crate::plugins::process_matches_cli(process, "claude") {
                pids.push(pid.as_u32());
            }
        }
        pids
    }

    fn find_claude_process(&self) -> Option<u32> {
        self.claude_process_pids().into_iter().next()
    }

    pub fn send_acp_command(&self, _command: &str) -> Result<(), BridgeError> {
        Err(BridgeError::OperationFailed(
            "ACP protocol not yet implemented".into(),
        ))
    }

    pub fn acp_request(&self, _request: &str) -> Result<String, BridgeError> {
        Err(BridgeError::NotAvailable(
            "ACP protocol not yet available".into(),
        ))
    }
}

impl AgentBridge for ClaudeBridge {
    fn name(&self) -> &str {
        "claude"
    }

    fn is_available(&self) -> bool {
        self.available && self.config.enabled
    }

    fn detect(&self) -> bool {
        self.find_claude_process().is_some()
    }

    fn send_command(&self, command: &str) -> Result<(), BridgeError> {
        if !self.detect() {
            return Err(BridgeError::NotAvailable(
                "No Claude Code process detected".into(),
            ));
        }
        self.send_acp_command(command)
    }

    fn send_keys(&self, keys: &str) -> Result<(), BridgeError> {
        self.send_command(keys)
    }

    fn get_status(&self) -> Result<BridgeStatus, BridgeError> {
        Ok(BridgeStatus {
            agent_name: self.config.name.clone(),
            state: if self.detect() {
                AgentState::Running
            } else {
                AgentState::Stopped
            },
            pid: self.find_claude_process(),
            uptime_secs: 0,
            metadata: HashMap::new(),
        })
    }

    fn shutdown(&self) -> Result<(), BridgeError> {
        Err(BridgeError::OperationFailed(
            "Claude shutdown is disabled until the bridge can target an exact tracked PID".into(),
        ))
    }

    fn restart(&self) -> Result<(), BridgeError> {
        self.shutdown()?;
        std::thread::sleep(std::time::Duration::from_secs(2));
        std::process::Command::new("claude").spawn().map_err(|e| {
            BridgeError::OperationFailed(format!("Failed to restart claude: {}", e))
        })?;
        Ok(())
    }

    fn configure(&mut self, config: &BridgeConfig) {
        self.config = config.clone();
        self.available = Self::check_available() && self.config.enabled;
    }
}

impl Default for ClaudeBridge {
    fn default() -> Self {
        Self::new()
    }
}
