use crate::core::config::{AgentState, BridgeConfig};
use std::collections::HashMap;

pub trait AgentBridge: Send + Sync {
    fn name(&self) -> &str;
    fn is_available(&self) -> bool;
    fn detect(&self) -> bool;
    fn send_command(&self, command: &str) -> Result<(), BridgeError>;
    fn send_keys(&self, keys: &str) -> Result<(), BridgeError>;
    fn get_status(&self) -> Result<BridgeStatus, BridgeError>;
    fn shutdown(&self) -> Result<(), BridgeError>;
    fn restart(&self) -> Result<(), BridgeError>;
    fn configure(&mut self, config: &BridgeConfig);
}

#[derive(Debug, Clone)]
pub struct BridgeStatus {
    pub agent_name: String,
    pub state: AgentState,
    pub pid: Option<u32>,
    pub uptime_secs: u64,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, thiserror::Error)]
pub enum BridgeError {
    #[error("Bridge not available: {0}")]
    NotAvailable(String),

    #[error("Bridge operation failed: {0}")]
    OperationFailed(String),

    #[error("Bridge timeout: {0}")]
    Timeout(String),

    #[error("Bridge connection error: {0}")]
    ConnectionError(String),
}

pub struct BridgeManager {
    bridges: HashMap<String, Box<dyn AgentBridge>>,
    active_bridge: Option<String>,
}

impl BridgeManager {
    pub fn new() -> Self {
        Self {
            bridges: HashMap::new(),
            active_bridge: None,
        }
    }

    pub fn register(&mut self, mut bridge: Box<dyn AgentBridge>, config: Option<&BridgeConfig>) {
        if let Some(cfg) = config {
            bridge.configure(cfg);
        }
        let name = bridge.name().to_string();
        self.bridges.insert(name, bridge);
    }

    pub fn detect_active(&mut self) -> Option<String> {
        for (name, bridge) in self.bridges.iter() {
            if bridge.detect() {
                self.active_bridge = Some(name.clone());
                log::info!("Detected active agent bridge: {}", name);
                return Some(name.clone());
            }
        }
        None
    }

    pub fn get_active(&self) -> Option<&dyn AgentBridge> {
        self.active_bridge
            .as_ref()
            .and_then(|name| self.bridges.get(name))
            .map(|b| b.as_ref())
    }

    pub fn get(&self, name: &str) -> Option<&dyn AgentBridge> {
        self.bridges.get(name).map(|b| b.as_ref())
    }

    pub fn list_bridges(&self) -> Vec<&str> {
        self.bridges.keys().map(|s| s.as_str()).collect()
    }

    pub fn send_to_active(&self, command: &str) -> Result<(), BridgeError> {
        if let Some(bridge) = self.get_active() {
            bridge.send_command(command)
        } else {
            Err(BridgeError::NotAvailable("No active bridge".into()))
        }
    }

    pub fn is_any_available(&self) -> bool {
        self.bridges.values().any(|b| b.is_available())
    }
}

impl Default for BridgeManager {
    fn default() -> Self {
        Self::new()
    }
}
