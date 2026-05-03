use crate::core::config::SupervisorConfig;
use crate::plugins::base::BridgeManager;
use crate::plugins::claude_bridge::ClaudeBridge;
use crate::plugins::codex_bridge::CodexBridge;
use crate::plugins::hermes_bridge::HermesBridge;
use crate::plugins::omx_bridge::OmxBridge;
use std::collections::{HashMap, HashSet};
use std::path::Path;

pub mod base;
pub mod claude_bridge;
pub mod codex_bridge;
pub mod hermes_bridge;
pub mod omx_bridge;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectedRuntime {
    pub key: String,
    pub display_name: String,
    pub pids: Vec<u32>,
    pub can_accelerate: bool,
}

pub(crate) fn process_matches_cli(process: &sysinfo::Process, command: &str) -> bool {
    let matches_token = |token: &str| {
        let cleaned = token.trim_matches('"').to_lowercase();
        let stem = Path::new(&cleaned)
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or(cleaned.as_str());
        stem == command
    };

    matches_token(process.name().to_string_lossy().as_ref())
        || process
            .cmd()
            .iter()
            .filter_map(|os| os.to_str())
            .any(matches_token)
}

pub fn detect_running_runtimes(config: &SupervisorConfig) -> Vec<DetectedRuntime> {
    let manager = build_bridge_manager(config);
    let process_matches = detect_runtime_processes();
    let mut detected: HashMap<String, DetectedRuntime> = HashMap::new();

    for (key, pids) in process_matches {
        if pids.is_empty() {
            continue;
        }

        let display_name = catalog_display_name(&key)
            .unwrap_or_else(|| default_display_name(&key))
            .to_string();
        detected.insert(
            key.clone(),
            DetectedRuntime {
                key,
                display_name,
                pids,
                can_accelerate: cfg!(windows),
            },
        );
    }

    for key in supported_bridge_keys() {
        let Some(bridge) = manager.get(key) else {
            continue;
        };

        let enabled = config
            .bridges
            .get(*key)
            .map(|bridge_config| bridge_config.enabled)
            .unwrap_or(true);
        if !enabled || !bridge.is_available() || !bridge.detect() {
            continue;
        }

        let display_name = bridge
            .get_status()
            .map(|status| status.agent_name)
            .unwrap_or_else(|_| display_name_for_agent(config, key));

        detected
            .entry(key.to_string())
            .and_modify(|runtime| {
                runtime.display_name = display_name.clone();
                runtime.can_accelerate = true;
            })
            .or_insert_with(|| DetectedRuntime {
                key: key.to_string(),
                display_name,
                pids: Vec::new(),
                can_accelerate: true,
            });
    }

    let mut runtimes: Vec<DetectedRuntime> = detected.into_values().collect();
    runtimes.sort_by(|a, b| a.display_name.cmp(&b.display_name));
    runtimes
}

pub fn detect_runtime_for_agent(
    config: &SupervisorConfig,
    agent_key: &str,
) -> Option<DetectedRuntime> {
    detect_running_runtimes(config)
        .into_iter()
        .find(|runtime| runtime.key == agent_key)
}

pub fn display_name_for_agent(config: &SupervisorConfig, agent_key: &str) -> String {
    if let Some(name) = config
        .bridges
        .get(agent_key)
        .map(|bridge| bridge.name.trim())
        .filter(|name| !name.is_empty())
    {
        return name.to_string();
    }

    default_display_name(agent_key).to_string()
}

pub fn build_bridge_manager(config: &SupervisorConfig) -> BridgeManager {
    let mut manager = BridgeManager::new();
    manager.register(Box::new(CodexBridge::new()), config.bridges.get("codex"));
    manager.register(Box::new(ClaudeBridge::new()), config.bridges.get("claude"));
    manager.register(Box::new(HermesBridge::new()), config.bridges.get("hermes"));
    manager.register(Box::new(OmxBridge::new()), config.bridges.get("omx"));
    manager
}

fn detect_runtime_processes() -> HashMap<String, Vec<u32>> {
    let mut system = sysinfo::System::new_all();
    system.refresh_all();

    let mut matches: HashMap<String, Vec<u32>> = HashMap::new();
    for (pid, process) in system.processes() {
        let name = process.name().to_string_lossy().to_lowercase();
        let cmd = process
            .cmd()
            .iter()
            .filter_map(|arg| arg.to_str())
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase();
        let haystack = format!("{name} {cmd}");

        for entry in runtime_catalog() {
            if entry.matches(&haystack) {
                matches
                    .entry(entry.key.to_string())
                    .or_default()
                    .push(pid.as_u32());
            }
        }
    }

    for pids in matches.values_mut() {
        let mut seen = HashSet::new();
        pids.retain(|pid| seen.insert(*pid));
        pids.sort_unstable();
    }

    matches
}

fn supported_bridge_keys() -> &'static [&'static str] {
    &["codex", "claude", "hermes", "omx"]
}

fn default_display_name(agent_key: &str) -> &'static str {
    match agent_key {
        "codex" => "Codex CLI",
        "claude" => "Claude Code",
        "hermes" => "Hermes Agent",
        "omx" => "oh-my-codex",
        "openclaw" => "OpenClaw",
        "opencode" => "OpenCode",
        "vscode" => "Visual Studio Code",
        "github-copilot" => "GitHub Copilot",
        "vscode-insiders" => "Visual Studio Code Insiders",
        "cursor" => "Cursor",
        "trae" => "Trae",
        "trae-solo" => "Trae Solo",
        "zed" => "Zed",
        "claude-desktop" => "Claude Desktop",
        "codex-desktop" => "Codex Desktop",
        "windsurf" => "Windsurf",
        "continue" => "Continue",
        "cline" => "Cline",
        "roo-code" => "Roo Code",
        "kilo-code" => "Kilo Code",
        "aider" => "Aider",
        "gemini-cli" => "Gemini CLI",
        "qwen-code" => "Qwen Code",
        "sourcegraph-cody" => "Sourcegraph Cody",
        "tabby" => "Tabby",
        "kiro" => "Kiro",
        "void" => "Void",
        "pearai" => "PearAI",
        "replit-agent" => "Replit Agent",
        "warp" => "Warp",
        _ => "Unknown Agent",
    }
}

fn catalog_display_name(key: &str) -> Option<&'static str> {
    runtime_catalog()
        .iter()
        .find(|entry| entry.key == key)
        .map(|entry| entry.display_name)
}

struct RuntimeCatalogEntry {
    key: &'static str,
    display_name: &'static str,
    any_patterns: &'static [&'static str],
    all_patterns: &'static [&'static str],
}

impl RuntimeCatalogEntry {
    fn matches(&self, haystack: &str) -> bool {
        self.all_patterns
            .iter()
            .all(|pattern| pattern_matches(haystack, pattern))
            && self
                .any_patterns
                .iter()
                .any(|pattern| pattern_matches(haystack, pattern))
    }
}

fn pattern_matches(haystack: &str, pattern: &str) -> bool {
    if pattern.contains('.') {
        return haystack.contains(pattern);
    }

    let normalized_haystack = normalize_runtime_text(haystack);
    let normalized_pattern = normalize_runtime_text(pattern);
    if normalized_pattern.contains(' ') {
        return normalized_haystack.contains(&normalized_pattern);
    }

    normalized_haystack
        .split_whitespace()
        .any(|token| token == normalized_pattern)
}

fn normalize_runtime_text(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn runtime_catalog() -> &'static [RuntimeCatalogEntry] {
    &[
        RuntimeCatalogEntry {
            key: "openclaw",
            display_name: "OpenClaw",
            any_patterns: &["openclaw", "open-claw"],
            all_patterns: &[],
        },
        RuntimeCatalogEntry {
            key: "hermes",
            display_name: "Hermes Agent",
            any_patterns: &["hermes"],
            all_patterns: &[],
        },
        RuntimeCatalogEntry {
            key: "opencode",
            display_name: "OpenCode",
            any_patterns: &["opencode", "open-code"],
            all_patterns: &[],
        },
        RuntimeCatalogEntry {
            key: "github-copilot",
            display_name: "GitHub Copilot",
            any_patterns: &[
                "github.copilot",
                "github copilot",
                "copilot-chat",
                "copilot",
            ],
            all_patterns: &[],
        },
        RuntimeCatalogEntry {
            key: "vscode-insiders",
            display_name: "Visual Studio Code Insiders",
            any_patterns: &["code - insiders", "code-insiders", "vscode-insiders"],
            all_patterns: &[],
        },
        RuntimeCatalogEntry {
            key: "vscode",
            display_name: "Visual Studio Code",
            any_patterns: &["code.exe", "visual studio code", "microsoft vs code"],
            all_patterns: &[],
        },
        RuntimeCatalogEntry {
            key: "cursor",
            display_name: "Cursor",
            any_patterns: &["cursor"],
            all_patterns: &[],
        },
        RuntimeCatalogEntry {
            key: "trae-solo",
            display_name: "Trae Solo",
            any_patterns: &["trae solo", "trae-solo"],
            all_patterns: &[],
        },
        RuntimeCatalogEntry {
            key: "trae",
            display_name: "Trae",
            any_patterns: &["trae"],
            all_patterns: &[],
        },
        RuntimeCatalogEntry {
            key: "zed",
            display_name: "Zed",
            any_patterns: &["zed"],
            all_patterns: &[],
        },
        RuntimeCatalogEntry {
            key: "claude-desktop",
            display_name: "Claude Desktop",
            any_patterns: &["claude desktop", "claude.exe"],
            all_patterns: &[],
        },
        RuntimeCatalogEntry {
            key: "claude",
            display_name: "Claude Code",
            any_patterns: &["claude-code", "claude code", "claude"],
            all_patterns: &[],
        },
        RuntimeCatalogEntry {
            key: "codex-desktop",
            display_name: "Codex Desktop",
            any_patterns: &["codex desktop", "codex-desktop"],
            all_patterns: &[],
        },
        RuntimeCatalogEntry {
            key: "codex",
            display_name: "Codex CLI",
            any_patterns: &["codex"],
            all_patterns: &[],
        },
        RuntimeCatalogEntry {
            key: "omx",
            display_name: "oh-my-codex",
            any_patterns: &["oh-my-codex", "omx"],
            all_patterns: &[],
        },
        RuntimeCatalogEntry {
            key: "windsurf",
            display_name: "Windsurf",
            any_patterns: &["windsurf", "codeium"],
            all_patterns: &[],
        },
        RuntimeCatalogEntry {
            key: "continue",
            display_name: "Continue",
            any_patterns: &["continue.continue", "continue"],
            all_patterns: &[],
        },
        RuntimeCatalogEntry {
            key: "cline",
            display_name: "Cline",
            any_patterns: &["saoudrizwan.claude-dev", "cline"],
            all_patterns: &[],
        },
        RuntimeCatalogEntry {
            key: "roo-code",
            display_name: "Roo Code",
            any_patterns: &["rooveterinaryinc.roo-cline", "roo-code", "roo code"],
            all_patterns: &[],
        },
        RuntimeCatalogEntry {
            key: "kilo-code",
            display_name: "Kilo Code",
            any_patterns: &["kilocode", "kilo-code", "kilo code"],
            all_patterns: &[],
        },
        RuntimeCatalogEntry {
            key: "aider",
            display_name: "Aider",
            any_patterns: &["aider"],
            all_patterns: &[],
        },
        RuntimeCatalogEntry {
            key: "gemini-cli",
            display_name: "Gemini CLI",
            any_patterns: &["gemini-cli", "gemini cli"],
            all_patterns: &[],
        },
        RuntimeCatalogEntry {
            key: "qwen-code",
            display_name: "Qwen Code",
            any_patterns: &["qwen-code", "qwen code"],
            all_patterns: &[],
        },
        RuntimeCatalogEntry {
            key: "sourcegraph-cody",
            display_name: "Sourcegraph Cody",
            any_patterns: &["sourcegraph.cody", "sourcegraph cody", "cody"],
            all_patterns: &[],
        },
        RuntimeCatalogEntry {
            key: "tabby",
            display_name: "Tabby",
            any_patterns: &["tabby"],
            all_patterns: &[],
        },
        RuntimeCatalogEntry {
            key: "kiro",
            display_name: "Kiro",
            any_patterns: &["kiro"],
            all_patterns: &[],
        },
        RuntimeCatalogEntry {
            key: "void",
            display_name: "Void",
            any_patterns: &["void"],
            all_patterns: &[],
        },
        RuntimeCatalogEntry {
            key: "pearai",
            display_name: "PearAI",
            any_patterns: &["pearai", "pear ai"],
            all_patterns: &[],
        },
        RuntimeCatalogEntry {
            key: "replit-agent",
            display_name: "Replit Agent",
            any_patterns: &["replit agent", "replit-agent"],
            all_patterns: &[],
        },
        RuntimeCatalogEntry {
            key: "warp",
            display_name: "Warp",
            any_patterns: &["warp"],
            all_patterns: &[],
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_display_name_for_known_agent_falls_back_to_default() {
        let config = SupervisorConfig::default();
        assert_eq!(display_name_for_agent(&config, "codex"), "Codex CLI");
    }

    #[test]
    fn test_display_name_uses_configured_bridge_name() {
        let mut config = SupervisorConfig::default();
        config.bridges.get_mut("codex").unwrap().name = "My Codex".to_string();

        assert_eq!(display_name_for_agent(&config, "codex"), "My Codex");
    }

    #[test]
    fn test_display_name_for_unknown_agent_uses_placeholder() {
        let config = SupervisorConfig::default();
        assert_eq!(display_name_for_agent(&config, "unknown"), "Unknown Agent");
    }

    #[test]
    fn test_catalog_detects_requested_runtimes() {
        let keys: HashSet<&str> = runtime_catalog().iter().map(|entry| entry.key).collect();
        for key in [
            "openclaw",
            "hermes",
            "opencode",
            "vscode",
            "github-copilot",
            "cursor",
            "vscode-insiders",
            "trae",
            "trae-solo",
            "zed",
            "claude",
            "claude-desktop",
            "codex",
            "codex-desktop",
        ] {
            assert!(keys.contains(key), "missing runtime catalog entry: {key}");
        }
    }

    #[test]
    fn test_catalog_pattern_matching() {
        let cursor = runtime_catalog()
            .iter()
            .find(|entry| entry.key == "cursor")
            .unwrap();
        assert!(cursor.matches(r"cursor.exe c:\users\demo\appdata\local\programs\cursor"));
    }

    #[test]
    fn test_short_catalog_patterns_do_not_match_inside_words() {
        let zed = runtime_catalog()
            .iter()
            .find(|entry| entry.key == "zed")
            .unwrap();
        assert!(!zed.matches("lghub_system_tray.exe --minimized"));
    }
}
