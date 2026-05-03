use crate::core::config::{AgentState, BridgeConfig};
use crate::plugins::base::{AgentBridge, BridgeError, BridgeStatus};
use std::collections::HashMap;

pub struct HermesBridge {
    config: BridgeConfig,
    available: bool,
}

impl HermesBridge {
    pub fn new() -> Self {
        Self {
            config: BridgeConfig {
                enabled: false,
                name: "Hermes Agent".to_string(),
                settings: HashMap::new(),
            },
            available: Self::check_available(),
        }
    }

    fn check_available() -> bool {
        crate::utils::which_command("hermes")
    }

    fn hermes_process_pids(&self) -> Vec<u32> {
        let mut system = sysinfo::System::new_all();
        system.refresh_all();
        let mut pids = Vec::new();

        for (pid, process) in system.processes() {
            if crate::plugins::process_matches_cli(process, "hermes") {
                pids.push(pid.as_u32());
            }
        }
        pids
    }

    fn find_hermes_process(&self) -> Option<u32> {
        self.hermes_process_pids().into_iter().next()
    }

    pub fn get_hermes_pids(&self) -> Vec<u32> {
        self.hermes_process_pids()
    }
}

impl AgentBridge for HermesBridge {
    fn name(&self) -> &str {
        "hermes"
    }

    fn is_available(&self) -> bool {
        self.available && self.config.enabled
    }

    fn detect(&self) -> bool {
        self.find_hermes_process().is_some()
    }

    fn send_command(&self, _command: &str) -> Result<(), BridgeError> {
        if !self.detect() {
            return Err(BridgeError::NotAvailable(
                "No Hermes Agent process detected".into(),
            ));
        }
        Err(BridgeError::OperationFailed(
            "Hermes Agent does not support direct command injection. Use PTY instead.".into(),
        ))
    }

    fn send_keys(&self, _keys: &str) -> Result<(), BridgeError> {
        Err(BridgeError::OperationFailed(
            "Hermes Agent does not support key injection. Use PTY instead.".into(),
        ))
    }

    fn get_status(&self) -> Result<BridgeStatus, BridgeError> {
        let pids = self.get_hermes_pids();
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
            "Hermes shutdown is disabled until the bridge can target an exact tracked PID".into(),
        ))
    }

    fn restart(&self) -> Result<(), BridgeError> {
        self.shutdown()?;
        std::thread::sleep(std::time::Duration::from_secs(2));
        std::process::Command::new("hermes")
            .arg("run")
            .spawn()
            .map_err(|e| {
                BridgeError::OperationFailed(format!("Failed to restart hermes: {}", e))
            })?;
        Ok(())
    }

    fn configure(&mut self, config: &BridgeConfig) {
        self.config = config.clone();
        self.available = Self::check_available() && self.config.enabled;
    }
}

impl Default for HermesBridge {
    fn default() -> Self {
        Self::new()
    }
}
