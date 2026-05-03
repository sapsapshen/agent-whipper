use crate::core::config::AgentState;
use notify::Watcher;
use rand::prelude::SliceRandom;
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Preset {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub trigger_on: Vec<AgentState>,
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
    pub steps: Vec<PresetStep>,
}

fn default_max_retries() -> u32 {
    3
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum PresetStep {
    #[serde(rename = "ctrl_c")]
    CtrlC,
    #[serde(rename = "ctrl_d")]
    CtrlD,
    #[serde(rename = "enter")]
    Enter,
    #[serde(rename = "text")]
    Text {
        #[serde(deserialize_with = "deserialize_string_or_array", default)]
        content: Vec<String>,
    },
    #[serde(rename = "wait")]
    Wait {
        #[serde(alias = "duration")]
        duration_secs: f64,
    },
    #[serde(rename = "exec")]
    Exec { content: String },
    #[serde(rename = "signal")]
    Signal { signal_name: String },
}

fn deserialize_string_or_array<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de::{self, Visitor};

    struct StringOrArrayVisitor;

    impl<'de> Visitor<'de> for StringOrArrayVisitor {
        type Value = Vec<String>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a string or an array of strings")
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(vec![value.to_string()])
        }

        fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(vec![value])
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: de::SeqAccess<'de>,
        {
            let mut vec = Vec::new();
            while let Some(value) = seq.next_element::<String>()? {
                vec.push(value);
            }
            Ok(vec)
        }
    }

    deserializer.deserialize_any(StringOrArrayVisitor)
}

impl PresetStep {
    pub fn get_text_content(&self) -> Option<&str> {
        match self {
            PresetStep::Text { content } => {
                content.choose(&mut rand::thread_rng()).map(|s| s.as_str())
            }
            _ => None,
        }
    }
}

impl Preset {
    pub fn is_safe_for_auto(&self) -> bool {
        let has_actionable_step = self.steps.iter().any(|step| match step {
            PresetStep::Text { content } => content.iter().any(|text| !text.trim().is_empty()),
            PresetStep::CtrlC
            | PresetStep::CtrlD
            | PresetStep::Enter
            | PresetStep::Signal { .. } => true,
            PresetStep::Wait { .. } | PresetStep::Exec { .. } => false,
        });

        has_actionable_step
            && self
                .steps
                .iter()
                .all(|step| !matches!(step, PresetStep::Exec { .. }))
    }
}

pub struct PresetManager {
    presets: HashMap<String, Preset>,
    preset_dir: PathBuf,
    hot_reload: bool,
    reload_rx: Option<mpsc::Receiver<()>>,
    reload_running: Option<Arc<AtomicBool>>,
    watcher_handle: Option<std::thread::JoinHandle<()>>,
}

impl PresetManager {
    pub fn new(preset_dir: String) -> Self {
        Self {
            presets: HashMap::new(),
            preset_dir: PathBuf::from(preset_dir),
            hot_reload: false,
            reload_rx: None,
            reload_running: None,
            watcher_handle: None,
        }
    }

    pub fn with_hot_reload(mut self) -> Self {
        let (tx, rx) = mpsc::channel();
        self.reload_rx = Some(rx);
        self.hot_reload = true;
        let _ = std::fs::create_dir_all(&self.preset_dir);

        let preset_dir = self.preset_dir.clone();
        let reload_running = Arc::new(AtomicBool::new(true));
        let thread_running = Arc::clone(&reload_running);
        let handle = std::thread::spawn(move || {
            let (watcher_tx, watcher_rx) = mpsc::channel();
            let mut last_snapshot = Self::directory_snapshot(&preset_dir);
            let mut watcher = notify::recommended_watcher(move |res| {
                if let Ok(_event) = res {
                    watcher_tx.send(()).ok();
                }
            })
            .ok();

            if let Some(ref mut w) = watcher {
                if w.watch(&preset_dir, notify::RecursiveMode::NonRecursive)
                    .is_ok()
                {
                    log::info!("Hot-reload watcher started for {:?}", preset_dir);
                } else {
                    log::warn!(
                        "Hot-reload watch registration failed for {:?}, polling instead",
                        preset_dir
                    );
                    watcher = None;
                }
            }

            while thread_running.load(Ordering::SeqCst) {
                if watcher.is_none() {
                    std::thread::sleep(std::time::Duration::from_secs(5));
                    let current_snapshot = Self::directory_snapshot(&preset_dir);
                    if thread_running.load(Ordering::SeqCst) && current_snapshot != last_snapshot {
                        tx.send(()).ok();
                        last_snapshot = current_snapshot;
                    }
                    continue;
                }

                match watcher_rx.recv_timeout(std::time::Duration::from_secs(1)) {
                    Ok(()) => {
                        tx.send(()).ok();
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
        });

        self.reload_running = Some(reload_running);
        self.watcher_handle = Some(handle);
        self
    }

    pub fn load_builtins(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let builtins = super::builtins::get_builtin_presets();
        for preset in builtins {
            log::info!("Loaded builtin preset: {}", preset.name);
            self.presets.insert(preset.name.clone(), preset);
        }
        Ok(())
    }

    pub fn load_from_dir(&mut self) -> Result<usize, Box<dyn std::error::Error>> {
        if !self.preset_dir.exists() {
            std::fs::create_dir_all(&self.preset_dir)?;
            log::info!("Created preset directory: {:?}", self.preset_dir);
            return Ok(0);
        }

        let mut count = 0;
        for entry in std::fs::read_dir(&self.preset_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path
                .extension()
                .is_some_and(|ext| ext == "yaml" || ext == "yml")
            {
                match Self::load_preset_file(&path) {
                    Ok(preset) => {
                        log::info!("Loaded preset from {:?}: {}", path, preset.name);
                        self.presets.insert(preset.name.clone(), preset);
                        count += 1;
                    }
                    Err(e) => {
                        log::warn!("Failed to load preset {:?}: {}", path, e);
                    }
                }
            }
        }
        Ok(count)
    }

    fn load_preset_file(path: &Path) -> Result<Preset, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        let preset: Preset = serde_yaml::from_str(&content)?;
        Ok(preset)
    }

    pub fn reload(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.presets.clear();
        self.load_builtins()?;
        self.load_from_dir()?;
        log::info!("Presets reloaded: {} total", self.presets.len());
        Ok(())
    }

    pub fn check_reload(&mut self) -> bool {
        if let Some(ref rx) = self.reload_rx {
            if rx.try_recv().is_ok() {
                log::info!("Hot-reload triggered");
                if let Err(e) = self.reload() {
                    log::error!("Hot-reload failed: {}", e);
                }
                return true;
            }
        }
        false
    }

    pub fn get_preset(&self, name: &str) -> Result<&Preset, Box<dyn std::error::Error>> {
        self.presets
            .get(name)
            .ok_or_else(|| format!("Preset not found: {}", name).into())
    }

    pub fn match_preset_for_state(&self, state: AgentState) -> Option<&Preset> {
        let mut matches: Vec<&Preset> = self
            .presets
            .values()
            .filter(|p| p.trigger_on.contains(&state))
            .collect();
        matches.sort_by(|a, b| a.name.cmp(&b.name));
        matches.into_iter().next()
    }

    pub fn match_auto_preset_for_state(&self, state: AgentState) -> Option<&Preset> {
        let mut matches: Vec<&Preset> = self
            .presets
            .values()
            .filter(|p| p.trigger_on.contains(&state) && p.is_safe_for_auto())
            .collect();
        matches.sort_by(|a, b| a.name.cmp(&b.name));
        matches.into_iter().next()
    }

    pub fn list_presets(&self) -> Vec<&Preset> {
        let mut presets: Vec<&Preset> = self.presets.values().collect();
        presets.sort_by(|a, b| a.name.cmp(&b.name));
        presets
    }

    pub fn add_preset(&mut self, preset: Preset) {
        self.presets.insert(preset.name.clone(), preset);
    }

    pub fn remove_preset(&mut self, name: &str) -> Option<Preset> {
        self.presets.remove(name)
    }

    pub fn preset_file_name(name: &str) -> String {
        format!("{}.yaml", crate::utils::sanitize_filename(name))
    }

    pub fn preset_path_for_name(&self, name: &str) -> PathBuf {
        self.preset_dir.join(Self::preset_file_name(name))
    }

    pub fn save_preset_to_dir(&self, preset: &Preset) -> Result<(), Box<dyn std::error::Error>> {
        std::fs::create_dir_all(&self.preset_dir)?;
        let path = self.preset_path_for_name(&preset.name);
        let content = serde_yaml::to_string(preset)?;
        std::fs::write(path, content)?;
        Ok(())
    }

    pub fn create_preset(
        &mut self,
        name: &str,
        description: &str,
        trigger_on: Vec<AgentState>,
        max_retries: u32,
        steps: Vec<PresetStep>,
    ) -> Preset {
        let preset = Preset {
            name: name.to_string(),
            description: description.to_string(),
            trigger_on,
            max_retries,
            steps,
        };
        self.add_preset(preset.clone());
        preset
    }

    pub fn count(&self) -> usize {
        self.presets.len()
    }

    fn directory_snapshot(path: &Path) -> Vec<(String, Option<std::time::SystemTime>)> {
        let mut snapshot = Vec::new();

        if let Ok(entries) = fs::read_dir(path) {
            for entry in entries.flatten() {
                let file_path = entry.path();
                if file_path
                    .extension()
                    .is_some_and(|ext| ext == "yaml" || ext == "yml")
                {
                    let modified = entry.metadata().ok().and_then(|meta| meta.modified().ok());
                    snapshot.push((file_path.to_string_lossy().into_owned(), modified));
                }
            }
        }

        snapshot.sort_by(|a, b| a.0.cmp(&b.0));
        snapshot
    }
}

impl Drop for PresetManager {
    fn drop(&mut self) {
        if let Some(flag) = &self.reload_running {
            flag.store(false, Ordering::SeqCst);
        }
        if let Some(handle) = self.watcher_handle.take() {
            let _ = handle.join();
        }
    }
}

impl PresetStep {
    pub fn from_yaml_value(value: &serde_yaml::Value) -> Result<Self, Box<dyn std::error::Error>> {
        let step_type = value["type"].as_str().ok_or("Missing step type")?;

        match step_type {
            "ctrl_c" => Ok(PresetStep::CtrlC),
            "ctrl_d" => Ok(PresetStep::CtrlD),
            "enter" => Ok(PresetStep::Enter),
            "text" => {
                let content = Self::extract_text_content(&value["content"])?;
                Ok(PresetStep::Text { content })
            }
            "wait" => {
                let duration = value["duration_secs"]
                    .as_f64()
                    .or_else(|| value["duration"].as_f64())
                    .ok_or("Missing duration/duration_secs for wait step")?;
                Ok(PresetStep::Wait {
                    duration_secs: duration,
                })
            }
            "exec" => {
                let content = value["content"]
                    .as_str()
                    .ok_or("Missing content for exec step")?
                    .to_string();
                Ok(PresetStep::Exec { content })
            }
            "signal" => {
                let signal_name = value["signal_name"]
                    .as_str()
                    .ok_or("Missing signal_name for signal step")?
                    .to_string();
                Ok(PresetStep::Signal { signal_name })
            }
            _ => Err(format!("Unknown step type: {}", step_type).into()),
        }
    }

    fn extract_text_content(
        value: &serde_yaml::Value,
    ) -> Result<Vec<String>, Box<dyn std::error::Error>> {
        if let Some(s) = value.as_str() {
            return Ok(vec![s.to_string()]);
        }
        if let Some(arr) = value.as_sequence() {
            let vec: Vec<String> = arr
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect();
            if vec.is_empty() {
                return Err("Empty content array".into());
            }
            return Ok(vec);
        }
        Err("Missing content for text step".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_preset_manager_creation() {
        let pm = PresetManager::new("test_presets".to_string());
        assert_eq!(pm.count(), 0);
    }

    #[test]
    fn test_builtins_loading() {
        let mut pm = PresetManager::new("test_presets".to_string());
        pm.load_builtins().unwrap();
        assert_eq!(pm.count(), 4);
    }

    #[test]
    fn test_preset_matching() {
        let mut pm = PresetManager::new("test_presets".to_string());
        pm.load_builtins().unwrap();

        let stalled_match = pm.match_preset_for_state(AgentState::Stalled);
        assert!(stalled_match.is_some());
        assert_eq!(stalled_match.unwrap().name, "dev-review-patch");

        let zombie_match = pm.match_preset_for_state(AgentState::Zombie);
        assert!(zombie_match.is_some());
        assert_eq!(zombie_match.unwrap().name, "unblock");
    }

    #[test]
    fn test_auto_matching_skips_exec_presets() {
        let mut pm = PresetManager::new("test_presets".to_string());
        pm.load_builtins().unwrap();

        let zombie_match = pm.match_auto_preset_for_state(AgentState::Zombie);
        assert!(zombie_match.is_some());
        assert_eq!(zombie_match.unwrap().name, "unblock");
    }

    #[test]
    fn test_add_and_remove_preset() {
        let mut pm = PresetManager::new("test_presets".to_string());
        pm.load_builtins().unwrap();
        let count_before = pm.count();

        let preset = Preset {
            name: "custom-test".to_string(),
            description: "A custom test preset".to_string(),
            trigger_on: vec![AgentState::Idle],
            max_retries: 1,
            steps: vec![PresetStep::CtrlC],
        };
        pm.add_preset(preset);
        assert_eq!(pm.count(), count_before + 1);

        let removed = pm.remove_preset("custom-test");
        assert!(removed.is_some());
        assert_eq!(pm.count(), count_before);
    }

    #[test]
    fn test_preset_step_serialization() {
        let preset = Preset {
            name: "test".to_string(),
            description: "test".to_string(),
            trigger_on: vec![AgentState::Stalled],
            max_retries: 3,
            steps: vec![
                PresetStep::CtrlC,
                PresetStep::Wait { duration_secs: 1.0 },
                PresetStep::Text {
                    content: vec!["hello".to_string()],
                },
                PresetStep::Enter,
            ],
        };

        let yaml = serde_yaml::to_string(&preset).unwrap();
        let parsed: Preset = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(parsed.name, "test");
        assert_eq!(parsed.steps.len(), 4);
    }

    #[test]
    fn test_text_array_randomization() {
        let step = PresetStep::Text {
            content: vec!["msg1".to_string(), "msg2".to_string(), "msg3".to_string()],
        };
        assert!(step.get_text_content().is_some());
    }

    #[test]
    fn test_deserialize_string_or_array() {
        let yaml_str = r#"
type: text
content: "single message"
"#;
        let value: serde_yaml::Value = serde_yaml::from_str(yaml_str).unwrap();
        let step = PresetStep::from_yaml_value(&value).unwrap();
        match step {
            PresetStep::Text { content } => assert_eq!(content, vec!["single message".to_string()]),
            _ => panic!("Expected Text step"),
        }

        let yaml_arr = r#"
type: text
content:
  - "msg1"
  - "msg2"
"#;
        let value: serde_yaml::Value = serde_yaml::from_str(yaml_arr).unwrap();
        let step = PresetStep::from_yaml_value(&value).unwrap();
        match step {
            PresetStep::Text { content } => {
                assert_eq!(content.len(), 2);
                assert!(content.contains(&"msg1".to_string()));
                assert!(content.contains(&"msg2".to_string()));
            }
            _ => panic!("Expected Text step"),
        }
    }
}
