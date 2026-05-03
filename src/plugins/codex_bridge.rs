use crate::core::config::{AgentState, BridgeConfig};
use crate::plugins::base::{AgentBridge, BridgeError, BridgeStatus};
use std::collections::HashMap;

pub struct CodexBridge {
    config: BridgeConfig,
    available: bool,
}

impl CodexBridge {
    pub fn new() -> Self {
        Self {
            config: BridgeConfig {
                enabled: true,
                name: "Codex CLI".to_string(),
                settings: HashMap::new(),
            },
            available: Self::check_available(),
        }
    }

    fn check_available() -> bool {
        crate::utils::which_command("codex")
    }

    fn codex_process_pids(&self) -> Vec<u32> {
        let mut system = sysinfo::System::new_all();
        system.refresh_all();
        let mut pids = Vec::new();

        for (pid, process) in system.processes() {
            if crate::plugins::process_matches_cli(process, "codex") {
                pids.push(pid.as_u32());
            }
        }
        pids
    }

    fn find_codex_process(&self) -> Option<u32> {
        self.codex_process_pids().into_iter().next()
    }

    pub fn get_codex_pids(&self) -> Vec<u32> {
        self.codex_process_pids()
    }
}

impl AgentBridge for CodexBridge {
    fn name(&self) -> &str {
        "codex"
    }

    fn is_available(&self) -> bool {
        self.available && self.config.enabled
    }

    fn detect(&self) -> bool {
        self.find_codex_process().is_some()
    }

    fn send_command(&self, _command: &str) -> Result<(), BridgeError> {
        if !self.detect() {
            return Err(BridgeError::NotAvailable(
                "No Codex CLI process detected".into(),
            ));
        }
        Err(BridgeError::OperationFailed(
            "Codex CLI does not support direct command injection. Use PTY instead.".into(),
        ))
    }

    fn send_keys(&self, _keys: &str) -> Result<(), BridgeError> {
        Err(BridgeError::OperationFailed(
            "Codex CLI does not support key injection. Use PTY instead.".into(),
        ))
    }

    fn get_status(&self) -> Result<BridgeStatus, BridgeError> {
        let pids = self.get_codex_pids();
        let mut metadata = HashMap::new();
        metadata.insert("processes".to_string(), format!("{}", pids.len()));

        Ok(BridgeStatus {
            agent_name: self.config.name.clone(),
            state: if !pids.is_empty() {
                AgentState::Running
            } else {
                AgentState::Stopped
            },
            pid: pids.first().copied(),
            uptime_secs: 0,
            metadata,
        })
    }

    fn shutdown(&self) -> Result<(), BridgeError> {
        Err(BridgeError::OperationFailed(
            "Codex CLI shutdown is disabled until the bridge can target an exact tracked PID"
                .into(),
        ))
    }

    fn restart(&self) -> Result<(), BridgeError> {
        self.shutdown()?;
        std::thread::sleep(std::time::Duration::from_secs(2));
        std::process::Command::new("codex")
            .arg("exec")
            .spawn()
            .map_err(|e| BridgeError::OperationFailed(format!("Failed to restart codex: {}", e)))?;
        Ok(())
    }

    fn configure(&mut self, config: &BridgeConfig) {
        self.config = config.clone();
        self.available = Self::check_available() && self.config.enabled;
    }
}

impl Default for CodexBridge {
    fn default() -> Self {
        Self::new()
    }
}
