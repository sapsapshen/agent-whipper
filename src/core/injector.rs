use crate::core::config::{AgentState, InjectConfig};
use crate::core::pty_manager::{PtySession, PtySignal};
use crate::presets::manager::{PresetManager, PresetStep};
use rand::prelude::SliceRandom;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

pub struct Injector {
    config: InjectConfig,
    preset_manager: PresetManager,
    auto_retry_counts: HashMap<AgentState, u32>,
}

impl Injector {
    pub fn new(config: InjectConfig, preset_manager: PresetManager) -> Self {
        Self {
            config,
            preset_manager,
            auto_retry_counts: HashMap::new(),
        }
    }

    pub fn inject_text(
        &self,
        session: &PtySession,
        text: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if text.len() > self.config.max_command_length {
            return Err(format!(
                "Command too long: {} > {} chars",
                text.len(),
                self.config.max_command_length
            )
            .into());
        }

        let delay = if self.config.adaptive_delay {
            self.calculate_delay(text)
        } else {
            self.config.char_delay_ms
        };

        session.write_char_by_char(text, delay)?;
        Ok(())
    }

    pub fn inject_ctrl_c(&self, session: &PtySession) -> Result<(), Box<dyn std::error::Error>> {
        session.send_signal(PtySignal::CtrlC)?;
        Ok(())
    }

    pub fn inject_ctrl_d(&self, session: &PtySession) -> Result<(), Box<dyn std::error::Error>> {
        session.send_signal(PtySignal::CtrlD)?;
        Ok(())
    }

    pub fn inject_enter(&self, session: &PtySession) -> Result<(), Box<dyn std::error::Error>> {
        session.send_signal(PtySignal::Enter)?;
        Ok(())
    }

    pub fn inject_signal(
        &self,
        session: &PtySession,
        signal: PtySignal,
    ) -> Result<(), Box<dyn std::error::Error>> {
        session.send_signal(signal)?;
        Ok(())
    }

    pub fn execute_preset(
        &mut self,
        session: &PtySession,
        preset_name: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.preset_manager.check_reload();
        let preset = self.preset_manager.get_preset(preset_name)?;

        log::info!(
            "Executing preset: {} ({} steps)",
            preset.name,
            preset.steps.len()
        );

        for step in &preset.steps {
            self.execute_step(session, step)?;
            std::thread::sleep(Duration::from_millis(50));
        }

        Ok(())
    }

    pub fn execute_preset_for_state(
        &mut self,
        session: &PtySession,
        state: AgentState,
    ) -> Result<Option<String>, Box<dyn std::error::Error>> {
        self.execute_preset_for_state_interruptible(session, state, None)
    }

    pub fn execute_preset_for_state_interruptible(
        &mut self,
        session: &PtySession,
        state: AgentState,
        running: Option<&AtomicBool>,
    ) -> Result<Option<String>, Box<dyn std::error::Error>> {
        self.preset_manager.check_reload();
        let matched = self.preset_manager.match_auto_preset_for_state(state);

        if let Some(preset) = matched {
            let attempts = self.auto_retry_counts.entry(state).or_insert(0);
            if *attempts >= preset.max_retries {
                log::warn!(
                    "Auto preset '{}' exhausted retries for {:?} ({} attempts)",
                    preset.name,
                    state,
                    preset.max_retries
                );
                return Ok(None);
            }
            *attempts += 1;

            log::info!(
                "Auto-matched preset '{}' for state {:?}",
                preset.name,
                state
            );

            let name = preset.name.clone();
            for step in &preset.steps {
                self.execute_step_interruptible(session, step, running)?;
                Self::interruptible_sleep(Duration::from_millis(50), running)?;
            }
            Ok(Some(name))
        } else {
            Ok(None)
        }
    }

    pub fn reset_auto_retries(&mut self) {
        self.auto_retry_counts.clear();
    }

    fn execute_step(
        &self,
        session: &PtySession,
        step: &PresetStep,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.execute_step_interruptible(session, step, None)
    }

    fn execute_step_interruptible(
        &self,
        session: &PtySession,
        step: &PresetStep,
        running: Option<&AtomicBool>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        match step {
            PresetStep::Text { content } => {
                let text = content
                    .choose(&mut rand::thread_rng())
                    .map(|s| s.as_str())
                    .unwrap_or_default();

                if text.is_empty() {
                    log::warn!("Empty text content for text step");
                    return Ok(());
                }

                log::debug!("Injecting text: {}", &text[..text.len().min(50)]);
                self.inject_text(session, text)?;
            }
            PresetStep::CtrlC => {
                log::debug!("Sending Ctrl+C");
                self.inject_ctrl_c(session)?;
            }
            PresetStep::CtrlD => {
                log::debug!("Sending Ctrl+D");
                self.inject_ctrl_d(session)?;
            }
            PresetStep::Enter => {
                log::debug!("Sending Enter");
                self.inject_enter(session)?;
            }
            PresetStep::Wait { duration_secs } => {
                log::debug!("Waiting {}s", duration_secs);
                Self::interruptible_sleep(Duration::from_secs_f64(*duration_secs), running)?;
            }
            PresetStep::Exec { content } => {
                return Err(
                    format!("Exec preset steps are disabled for safety: {}", content).into(),
                );
            }
            PresetStep::Signal { signal_name } => {
                log::debug!("Sending signal: {}", signal_name);
                let signal = match signal_name.to_uppercase().as_str() {
                    "SIGTERM" => PtySignal::Sigterm,
                    "SIGKILL" => PtySignal::Sigkill,
                    "SIGINT" => PtySignal::CtrlC,
                    other => {
                        return Err(format!("Unsupported preset signal: {}", other).into());
                    }
                };
                self.inject_signal(session, signal)?;
            }
        }
        Ok(())
    }

    fn interruptible_sleep(
        duration: Duration,
        running: Option<&AtomicBool>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let deadline = std::time::Instant::now() + duration;
        loop {
            if running.is_some_and(|flag| !flag.load(Ordering::SeqCst)) {
                return Err("Interrupted by shutdown".into());
            }

            let now = std::time::Instant::now();
            if now >= deadline {
                return Ok(());
            }

            std::thread::sleep((deadline - now).min(Duration::from_millis(100)));
        }
    }

    fn calculate_delay(&self, text: &str) -> u64 {
        let base = self.config.char_delay_ms;
        if text.len() < 50 {
            base
        } else if text.len() < 200 {
            base.max(5)
        } else {
            base.min(5)
        }
    }

    pub fn get_preset_manager(&self) -> &PresetManager {
        &self.preset_manager
    }

    pub fn get_preset_manager_mut(&mut self) -> &mut PresetManager {
        &mut self.preset_manager
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::presets::manager::PresetStep;

    fn make_test_preset_manager() -> PresetManager {
        let mut pm = PresetManager::new("nonexistent".to_string());
        let preset = crate::presets::manager::Preset {
            name: "test-preset".to_string(),
            description: "Test preset".to_string(),
            trigger_on: vec![AgentState::Stalled],
            max_retries: 1,
            steps: vec![
                PresetStep::Text {
                    content: vec!["hello".to_string()],
                },
                PresetStep::Enter,
            ],
        };
        pm.add_preset(preset);
        pm
    }

    #[test]
    fn test_injector_creation() {
        let config = InjectConfig::default();
        let pm = make_test_preset_manager();
        let injector = Injector::new(config, pm);
        assert_eq!(injector.config.char_delay_ms, 10);
    }

    #[test]
    fn test_calculate_delay() {
        let config = InjectConfig::default();
        let pm = make_test_preset_manager();
        let injector = Injector::new(config, pm);

        let delay_short = injector.calculate_delay("hi");
        assert_eq!(delay_short, 10);

        let delay_medium = injector.calculate_delay(&"x".repeat(80));
        assert_eq!(delay_medium, 10);

        let delay_long = injector.calculate_delay(&"x".repeat(300));
        assert!(delay_long <= 5);
    }

    #[test]
    fn test_text_too_long() {
        let config = InjectConfig {
            max_command_length: 10,
            ..InjectConfig::default()
        };
        let pm = make_test_preset_manager();
        let injector = Injector::new(config, pm);

        assert!(injector.inject_text_internal("this is too long").is_err());
    }

    impl Injector {
        fn inject_text_internal(&self, text: &str) -> Result<(), Box<dyn std::error::Error>> {
            if text.len() > self.config.max_command_length {
                return Err(format!(
                    "Command too long: {} > {} chars",
                    text.len(),
                    self.config.max_command_length
                )
                .into());
            }
            Ok(())
        }
    }
}
