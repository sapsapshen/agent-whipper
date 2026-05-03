use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Component;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SupervisorConfig {
    pub agent: String,
    pub mode: WatchMode,
    pub watch: WatchConfig,
    pub inject: InjectConfig,
    pub bridges: HashMap<String, BridgeConfig>,
    pub preset: PresetConfig,
    pub session_dir: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub enum WatchMode {
    #[serde(rename = "watch")]
    #[default]
    Watch,
    #[serde(rename = "passive")]
    Passive,
}

impl WatchMode {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.to_lowercase().as_str() {
            "watch" | "active" => Ok(WatchMode::Watch),
            "passive" => Ok(WatchMode::Passive),
            other => Err(format!(
                "Unsupported watch mode '{}'. Use 'watch' or 'passive'.",
                other
            )),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WatchConfig {
    pub poll_interval_ms: u64,
    pub stalled_timeout_secs: u64,
    pub zombie_timeout_secs: u64,
    pub cpu_threshold_percent: f64,
    pub heartbeat_timeout_secs: u64,
    pub track_memory: bool,
    pub auto_intervene: bool,
}

impl Default for WatchConfig {
    fn default() -> Self {
        Self {
            poll_interval_ms: 1000,
            stalled_timeout_secs: 30,
            zombie_timeout_secs: 120,
            cpu_threshold_percent: 1.0,
            heartbeat_timeout_secs: 10,
            track_memory: true,
            auto_intervene: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct InjectConfig {
    pub char_delay_ms: u64,
    pub max_command_length: usize,
    pub adaptive_delay: bool,
    pub confirm_receipt: bool,
}

impl Default for InjectConfig {
    fn default() -> Self {
        Self {
            char_delay_ms: 10,
            max_command_length: 4096,
            adaptive_delay: true,
            confirm_receipt: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BridgeConfig {
    pub enabled: bool,
    pub name: String,
    pub settings: HashMap<String, String>,
}

impl Default for BridgeConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            name: String::new(),
            settings: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PresetConfig {
    pub enabled: bool,
    pub auto_match: bool,
    pub max_retries_default: u32,
    pub preset_dir: String,
    pub hot_reload: bool,
}

impl Default for PresetConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            auto_match: true,
            max_retries_default: 3,
            preset_dir: String::from("presets"),
            hot_reload: true,
        }
    }
}

impl Default for SupervisorConfig {
    fn default() -> Self {
        let mut bridges = HashMap::new();
        bridges.insert(
            "omx".to_string(),
            BridgeConfig {
                enabled: true,
                name: "oh-my-codex".to_string(),
                settings: HashMap::new(),
            },
        );
        bridges.insert(
            "codex".to_string(),
            BridgeConfig {
                enabled: true,
                name: "Codex CLI".to_string(),
                settings: HashMap::new(),
            },
        );
        bridges.insert(
            "claude".to_string(),
            BridgeConfig {
                enabled: false,
                name: "Claude Code".to_string(),
                settings: HashMap::new(),
            },
        );
        bridges.insert(
            "hermes".to_string(),
            BridgeConfig {
                enabled: false,
                name: "Hermes Agent".to_string(),
                settings: HashMap::new(),
            },
        );

        Self {
            agent: String::from("codex"),
            mode: WatchMode::Watch,
            watch: WatchConfig::default(),
            inject: InjectConfig::default(),
            bridges,
            preset: PresetConfig::default(),
            session_dir: String::from("sessions"),
        }
    }
}

impl SupervisorConfig {
    fn discover_bundled_preset_dir() -> Option<PathBuf> {
        let exe = std::env::current_exe().ok()?;
        for dir in exe.ancestors().skip(1).take(4) {
            let candidate = dir.join("presets");
            if candidate.exists() {
                return Some(candidate);
            }
        }
        None
    }

    fn prefers_bundled_preset_dir(path: &std::path::Path) -> bool {
        let components = path.components().collect::<Vec<_>>();
        matches!(components.as_slice(), [Component::Normal(name)] if *name == "presets")
            || matches!(
                components.as_slice(),
                [Component::CurDir, Component::Normal(name)] if *name == "presets"
            )
    }

    fn app_config_dir() -> Result<PathBuf, Box<dyn std::error::Error>> {
        Ok(dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("agentwhipper"))
    }

    pub fn load() -> Result<Self, Box<dyn std::error::Error>> {
        let config_path = Self::config_path()?;
        if config_path.exists() {
            let content = std::fs::read_to_string(&config_path)?;
            let mut config: Self = serde_yaml::from_str(&content)?;
            for (name, bridge) in Self::default().bridges {
                config.bridges.entry(name).or_insert(bridge);
            }
            Ok(config)
        } else {
            let config = Self::default();
            config.save()?;
            Ok(config)
        }
    }

    pub fn save(&self) -> Result<(), Box<dyn std::error::Error>> {
        let config_path = Self::config_path()?;
        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = serde_yaml::to_string(self)?;
        std::fs::write(config_path, content)?;
        Ok(())
    }

    fn config_path() -> Result<PathBuf, Box<dyn std::error::Error>> {
        Ok(Self::app_config_dir()?.join("config.yaml"))
    }

    pub fn resolved_config_path() -> Result<PathBuf, Box<dyn std::error::Error>> {
        Self::config_path()
    }

    pub fn agent_command(&self) -> Vec<String> {
        match self.agent.as_str() {
            "codex" => vec!["codex".to_string()],
            "claude" => vec!["claude".to_string()],
            "hermes" => vec!["hermes".to_string(), "run".to_string()],
            other => vec![other.to_string()],
        }
    }

    pub fn resolved_preset_dir(&self) -> Result<PathBuf, Box<dyn std::error::Error>> {
        let preset_dir = PathBuf::from(&self.preset.preset_dir);
        if preset_dir.is_absolute() {
            return Ok(preset_dir);
        }

        if Self::prefers_bundled_preset_dir(&preset_dir) {
            if let Some(bundled_dir) = Self::discover_bundled_preset_dir() {
                return Ok(bundled_dir);
            }
        }

        Ok(Self::app_config_dir()?.join(preset_dir))
    }

    pub fn is_supported_agent(agent: &str) -> bool {
        matches!(agent, "codex" | "claude" | "hermes")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AgentState {
    #[serde(rename = "RUNNING", alias = "Running")]
    Running,
    #[serde(rename = "STALLED", alias = "Stalled")]
    Stalled,
    #[serde(rename = "ZOMBIE", alias = "Zombie")]
    Zombie,
    #[serde(rename = "IDLE", alias = "Idle")]
    Idle,
    #[serde(rename = "STOPPED", alias = "Stopped")]
    Stopped,
}

impl AgentState {
    pub fn as_str(&self) -> &'static str {
        match self {
            AgentState::Running => "RUNNING",
            AgentState::Stalled => "STALLED",
            AgentState::Zombie => "ZOMBIE",
            AgentState::Idle => "IDLE",
            AgentState::Stopped => "STOPPED",
        }
    }

    pub fn emoji(&self) -> &'static str {
        match self {
            AgentState::Running => "🟢",
            AgentState::Stalled => "🟡",
            AgentState::Zombie => "🔴",
            AgentState::Idle => "⚪",
            AgentState::Stopped => "⚫",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = SupervisorConfig::default();
        assert_eq!(config.agent, "codex");
        assert_eq!(config.watch.poll_interval_ms, 1000);
        assert_eq!(config.watch.stalled_timeout_secs, 30);
        assert_eq!(config.watch.zombie_timeout_secs, 120);
        assert_eq!(config.bridges.len(), 4);
    }

    #[test]
    fn test_watch_mode_from_str() {
        assert_eq!(WatchMode::parse("watch").unwrap(), WatchMode::Watch);
        assert_eq!(WatchMode::parse("passive").unwrap(), WatchMode::Passive);
        assert_eq!(WatchMode::parse("active").unwrap(), WatchMode::Watch);
        assert!(WatchMode::parse("nope").is_err());
    }

    #[test]
    fn test_agent_state_display() {
        assert_eq!(AgentState::Running.as_str(), "RUNNING");
        assert_eq!(AgentState::Stalled.emoji(), "🟡");
    }

    #[test]
    fn test_config_serialization() {
        let config = SupervisorConfig::default();
        let yaml = serde_yaml::to_string(&config).unwrap();
        let parsed: SupervisorConfig = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(parsed.agent, config.agent);
    }
}
