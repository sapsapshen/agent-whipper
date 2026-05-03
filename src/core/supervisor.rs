use crate::core::config::{AgentState, SupervisorConfig};
use crate::core::injector::Injector;
use crate::core::pty_manager::PtySession;
use crate::core::watcher::{SessionStats, Watcher};
use crate::plugins::display_name_for_agent;
use crate::presets::manager::PresetManager;
use crate::utils::stats::WhipStats;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

pub struct Supervisor {
    pub config: SupervisorConfig,
    pub session: Arc<Mutex<Option<Arc<PtySession>>>>,
    pub watcher: Arc<Watcher>,
    pub injector: Arc<Mutex<Injector>>,
    pub session_id: String,
    pub running: Arc<AtomicBool>,
    watch_handle: Arc<Mutex<Option<std::thread::JoinHandle<()>>>>,
    auto_intervene_handle: Arc<Mutex<Option<std::thread::JoinHandle<()>>>>,
}

impl Supervisor {
    pub fn new(config: SupervisorConfig) -> Result<Self, Box<dyn std::error::Error>> {
        let preset_dir = config.resolved_preset_dir()?.to_string_lossy().into_owned();
        let inject_config = config.inject.clone();
        let watch_config = config.watch.clone();
        let session_id = Uuid::new_v4().to_string();

        let mut preset_manager = PresetManager::new(preset_dir);
        if let Err(e) = preset_manager.load_builtins() {
            log::warn!("Failed to load builtin presets: {}", e);
        }
        if config.preset.hot_reload {
            preset_manager = preset_manager.with_hot_reload();
        }
        if let Err(e) = preset_manager.load_from_dir() {
            log::warn!("Failed to load user presets: {}", e);
        }

        Ok(Self {
            config,
            session: Arc::new(Mutex::new(None)),
            watcher: Arc::new(Watcher::new(watch_config)),
            injector: Arc::new(Mutex::new(Injector::new(inject_config, preset_manager))),
            session_id,
            running: Arc::new(AtomicBool::new(false)),
            watch_handle: Arc::new(Mutex::new(None)),
            auto_intervene_handle: Arc::new(Mutex::new(None)),
        })
    }

    pub fn start_agent(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let command = self.config.agent_command();
        let session = PtySession::spawn(&command, &self.session_id, 24, 120)?;

        std::thread::sleep(std::time::Duration::from_millis(250));
        if !session.is_alive()? {
            let output = session.read_output().unwrap_or_default();
            let detail = output.trim();
            if detail.is_empty() {
                return Err("Agent process exited immediately after startup".into());
            }
            return Err(format!(
                "Agent process exited immediately after startup:\n{}",
                detail
            )
            .into());
        }

        log::info!(
            "Agent started: pid={}, session={}",
            session.pid,
            self.session_id
        );

        *self.session.lock().unwrap() = Some(Arc::new(session));
        self.running.store(true, Ordering::SeqCst);

        if self.config.mode == crate::core::config::WatchMode::Watch {
            self.start_monitoring();
        }

        Ok(())
    }

    fn start_monitoring(&mut self) {
        let pid = self.get_pid().unwrap_or(0);
        let watcher = Arc::clone(&self.watcher);
        let session = Arc::clone(&self.session);

        let handle = Watcher::start(watcher, session, pid);
        *self.watch_handle.lock().unwrap() = Some(handle);

        if self.config.watch.auto_intervene {
            self.start_auto_intervene();
        }
    }

    fn start_auto_intervene(&mut self) {
        let watcher = Arc::clone(&self.watcher);
        let injector = Arc::clone(&self.injector);
        let session = Arc::clone(&self.session);
        let running = Arc::clone(&self.running);
        let config = self.config.clone();

        let handle = std::thread::spawn(move || {
            let mut retries_armed = false;

            while running.load(Ordering::SeqCst) {
                std::thread::sleep(std::time::Duration::from_secs(1));

                let current_state = *watcher.state.lock().unwrap();

                match current_state {
                    AgentState::Stalled | AgentState::Zombie => {
                        retries_armed = true;
                        if let Ok(mut inj) = injector.lock() {
                            let session_handle = session
                                .lock()
                                .ok()
                                .and_then(|guard| guard.as_ref().cloned());

                            if let Some(s) = session_handle {
                                match s.is_alive() {
                                    Ok(true) => {}
                                    Ok(false) => {
                                        log::warn!(
                                            "Skipping intervention for {:?} because the PTY session is no longer running",
                                            current_state
                                        );
                                        continue;
                                    }
                                    Err(e) => {
                                        log::warn!(
                                            "Skipping intervention for {:?} because session liveness could not be determined: {}",
                                            current_state,
                                            e
                                        );
                                        continue;
                                    }
                                }

                                log::warn!("Auto-intervening for state: {:?}", current_state);
                                match inj.execute_preset_for_state_interruptible(
                                    &s,
                                    current_state,
                                    Some(&running),
                                ) {
                                    Ok(Some(name)) => {
                                        watcher.notify_heartbeat();
                                        let recovered = (0..10).any(|_| {
                                            std::thread::sleep(std::time::Duration::from_millis(
                                                500,
                                            ));
                                            matches!(
                                                *watcher.state.lock().unwrap(),
                                                AgentState::Running | AgentState::Idle
                                            )
                                        });

                                        if recovered {
                                            watcher.record_intervention(&name, current_state);
                                            let display_name =
                                                display_name_for_agent(&config, &config.agent);

                                            if let Err(e) = WhipStats::record_rescue_global(
                                                &name,
                                                &display_name,
                                                current_state.as_str(),
                                            ) {
                                                log::warn!("Failed to persist rescue stats: {}", e);
                                            }
                                            log::info!(
                                                "Intervention '{}' recovered {} from {:?}",
                                                name,
                                                display_name,
                                                current_state
                                            );
                                        } else {
                                            log::warn!(
                                                "Intervention '{}' executed for {:?}, but recovery was not confirmed",
                                                name, current_state
                                            );
                                        }
                                    }
                                    Ok(None) => {
                                        log::debug!("No preset matched for {:?}", current_state);
                                    }
                                    Err(_e) if !running.load(Ordering::SeqCst) => break,
                                    Err(e) => {
                                        log::error!(
                                            "Intervention failed for {:?}: {}",
                                            current_state,
                                            e
                                        );
                                    }
                                }
                            }
                        }
                        for _ in 0..50 {
                            if !running.load(Ordering::SeqCst) {
                                break;
                            }
                            std::thread::sleep(std::time::Duration::from_millis(100));
                        }
                    }
                    AgentState::Stopped => break,
                    _ => {
                        if retries_armed {
                            if let Ok(mut inj) = injector.lock() {
                                inj.reset_auto_retries();
                            }
                            retries_armed = false;
                        }
                    }
                }
            }
        });

        *self.auto_intervene_handle.lock().unwrap() = Some(handle);
    }

    pub fn inject_command(&self, text: &str) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(ref session) = *self.session.lock().unwrap() {
            let injector = self.injector.lock().unwrap();
            injector.inject_text(session, text)?;
            injector.inject_enter(session)?;
            self.watcher.notify_output();
            Ok(())
        } else {
            Err("No active session".into())
        }
    }

    pub fn inject_preset(&self, preset_name: &str) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(ref session) = *self.session.lock().unwrap() {
            let mut injector = self.injector.lock().unwrap();
            injector.execute_preset(session, preset_name)?;
            self.watcher.notify_output();
            Ok(())
        } else {
            Err("No active session".into())
        }
    }

    pub fn send_signal(
        &self,
        signal: crate::core::pty_manager::PtySignal,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(ref session) = *self.session.lock().unwrap() {
            session.send_signal(signal)?;
            self.watcher.notify_output();
            Ok(())
        } else {
            Err("No active session".into())
        }
    }

    pub fn get_state(&self) -> AgentState {
        *self.watcher.state.lock().unwrap()
    }

    pub fn get_stats(&self) -> SessionStats {
        self.watcher.stats.lock().unwrap().clone()
    }

    pub fn get_pid(&self) -> Option<u32> {
        self.session.lock().unwrap().as_ref().map(|s| s.pid)
    }

    pub fn read_output(&self) -> Result<String, Box<dyn std::error::Error>> {
        if let Some(ref session) = *self.session.lock().unwrap() {
            session.read_output()
        } else {
            Ok(String::new())
        }
    }

    pub fn stop(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.running.store(false, Ordering::SeqCst);
        self.watcher.stop();

        let session = self.session.lock().unwrap().take();
        let kill_result = if let Some(session) = session {
            session.kill()
        } else {
            Ok(())
        };

        if let Some(handle) = self.watch_handle.lock().unwrap().take() {
            handle.join().ok();
        }
        if let Some(handle) = self.auto_intervene_handle.lock().unwrap().take() {
            handle.join().ok();
        }
        log::info!("Supervisor stopped for session {}", self.session_id);
        kill_result?;
        Ok(())
    }

    pub fn get_interventions(&self) -> Vec<crate::core::watcher::InterventionRecord> {
        self.watcher.get_interventions()
    }
}

impl Drop for Supervisor {
    fn drop(&mut self) {
        self.stop().ok();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_supervisor_creation() {
        let config = SupervisorConfig::default();
        let supervisor = Supervisor::new(config);
        assert!(supervisor.is_ok());
    }

    #[test]
    fn test_session_id_unique() {
        let config = SupervisorConfig::default();
        let s1 = Supervisor::new(config.clone()).unwrap();
        let s2 = Supervisor::new(config).unwrap();
        assert_ne!(s1.session_id, s2.session_id);
    }

    #[test]
    fn test_inject_without_session() {
        let config = SupervisorConfig::default();
        let supervisor = Supervisor::new(config).unwrap();
        assert!(supervisor.inject_command("test").is_err());
    }

    #[test]
    fn test_initial_state() {
        let config = SupervisorConfig::default();
        let supervisor = Supervisor::new(config).unwrap();
        assert_eq!(supervisor.get_state(), AgentState::Running);
    }
}
