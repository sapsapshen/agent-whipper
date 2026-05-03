use crate::core::config::{AgentState, WatchConfig};
use crate::core::pty_manager::PtySession;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub struct Watcher {
    pub state: Arc<Mutex<AgentState>>,
    pub stats: Arc<Mutex<SessionStats>>,
    config: WatchConfig,
    system: Arc<Mutex<sysinfo::System>>,
    last_cpu_refresh: Arc<Mutex<Option<Instant>>>,
    running: Arc<AtomicBool>,
    last_output: Arc<Mutex<Instant>>,
    last_heartbeat: Arc<Mutex<Instant>>,
    start_time: Instant,
    output_changed: Arc<AtomicBool>,
    previous_output_len: Arc<Mutex<u64>>,
    interventions: Arc<Mutex<Vec<InterventionRecord>>>,
}

#[derive(Debug, Clone, Default)]
pub struct SessionStats {
    pub cpu_percent: f64,
    pub memory_bytes: u64,
    pub memory_formatted: String,
    pub turn_count: u64,
    pub uptime_secs: u64,
    pub stalls: u32,
    pub interventions: u32,
}

#[derive(Debug, Clone)]
pub struct InterventionRecord {
    pub preset_name: String,
    pub timestamp: chrono::DateTime<chrono::Local>,
    pub from_state: AgentState,
}

impl Watcher {
    pub fn new(config: WatchConfig) -> Self {
        let now = Instant::now();
        Self {
            state: Arc::new(Mutex::new(AgentState::Running)),
            stats: Arc::new(Mutex::new(SessionStats::default())),
            config,
            system: Arc::new(Mutex::new(sysinfo::System::new_all())),
            last_cpu_refresh: Arc::new(Mutex::new(None)),
            running: Arc::new(AtomicBool::new(false)),
            last_output: Arc::new(Mutex::new(now)),
            last_heartbeat: Arc::new(Mutex::new(now)),
            start_time: now,
            output_changed: Arc::new(AtomicBool::new(false)),
            previous_output_len: Arc::new(Mutex::new(0)),
            interventions: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn start(
        watcher: Arc<Watcher>,
        session: Arc<Mutex<Option<Arc<PtySession>>>>,
        pid: u32,
    ) -> std::thread::JoinHandle<()> {
        watcher.running.store(true, Ordering::SeqCst);

        let watcher_clone = Arc::clone(&watcher);
        let session_clone = Arc::clone(&session);
        std::thread::spawn(move || {
            let poll_interval = Duration::from_millis(watcher_clone.config.poll_interval_ms);

            while watcher_clone.running.load(Ordering::SeqCst) {
                std::thread::sleep(poll_interval);

                if !watcher_clone.refresh_session_activity(&session_clone) {
                    watcher_clone.stop();
                    break;
                }

                let mut state = watcher_clone.state.lock().unwrap();
                if *state == AgentState::Stopped {
                    break;
                }

                let new_state = watcher_clone.determine_state(pid);
                if *state != new_state {
                    log::info!("State transition: {:?} -> {:?}", *state, new_state);
                    *state = new_state;
                }

                watcher_clone.update_stats(pid);
            }
        })
    }

    fn refresh_session_activity(&self, session: &Arc<Mutex<Option<Arc<PtySession>>>>) -> bool {
        let guard = session.lock().unwrap();
        let Some(session) = guard.as_ref() else {
            return true;
        };

        match session.is_alive() {
            Ok(true) => {}
            Ok(false) => return false,
            Err(e) => {
                log::warn!("Failed to check PTY liveness: {}", e);
                return true;
            }
        }

        let current_output_len = session.output_bytes_received();
        let mut previous_output_len = self.previous_output_len.lock().unwrap();
        if current_output_len > *previous_output_len {
            *previous_output_len = current_output_len;
            self.notify_output();
        }

        true
    }

    pub fn determine_state(&self, _pid: u32) -> AgentState {
        let elapsed_since_output = self.last_output.lock().unwrap().elapsed();
        let elapsed_since_heartbeat = self.last_heartbeat.lock().unwrap().elapsed();
        let stalled_timeout = Duration::from_secs(self.config.stalled_timeout_secs);
        let zombie_timeout = Duration::from_secs(self.config.zombie_timeout_secs);
        let heartbeat_timeout = Duration::from_secs(self.config.heartbeat_timeout_secs);
        let cpu = self.get_process_cpu(_pid);
        let is_low_cpu = cpu < self.config.cpu_threshold_percent;

        if elapsed_since_output > zombie_timeout && is_low_cpu {
            return AgentState::Zombie;
        }

        if elapsed_since_output > stalled_timeout && is_low_cpu {
            return AgentState::Stalled;
        }

        if elapsed_since_heartbeat > heartbeat_timeout && is_low_cpu {
            return AgentState::Idle;
        }

        AgentState::Running
    }

    pub fn update_stats(&self, pid: u32) {
        let mut stats = self.stats.lock().unwrap();
        stats.cpu_percent = self.get_process_cpu(pid);
        stats.uptime_secs = self.start_time.elapsed().as_secs();

        if let Some(mem) = self.get_process_memory(pid) {
            stats.memory_bytes = mem;
            stats.memory_formatted = Self::format_memory(mem);
        }
    }

    pub fn notify_output(&self) {
        *self.last_output.lock().unwrap() = Instant::now();
        *self.last_heartbeat.lock().unwrap() = Instant::now();
        self.output_changed.store(true, Ordering::SeqCst);
    }

    pub fn notify_heartbeat(&self) {
        *self.last_heartbeat.lock().unwrap() = Instant::now();
    }

    pub fn check_output_changed(&self) -> bool {
        self.output_changed.swap(false, Ordering::SeqCst)
    }

    pub fn record_intervention(&self, preset_name: &str, from_state: AgentState) {
        let mut interventions = self.interventions.lock().unwrap();
        let mut stats = self.stats.lock().unwrap();
        interventions.push(InterventionRecord {
            preset_name: preset_name.to_string(),
            timestamp: chrono::Local::now(),
            from_state,
        });
        stats.interventions += 1;
    }

    pub fn get_interventions(&self) -> Vec<InterventionRecord> {
        self.interventions.lock().unwrap().clone()
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
        if let Ok(mut state) = self.state.lock() {
            *state = AgentState::Stopped;
        }
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    fn get_process_cpu(&self, pid: u32) -> f64 {
        if pid == 0 {
            return 0.0;
        }
        let mut system = self.system.lock().unwrap();
        let mut last_cpu_refresh = self.last_cpu_refresh.lock().unwrap();
        if last_cpu_refresh
            .is_none_or(|last| last.elapsed() >= sysinfo::MINIMUM_CPU_UPDATE_INTERVAL)
        {
            system.refresh_cpu_usage();
            system.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[sysinfo::Pid::from_u32(
                pid,
            )]));
            *last_cpu_refresh = Some(Instant::now());
        }
        if let Some(process) = system.process(sysinfo::Pid::from_u32(pid)) {
            process.cpu_usage() as f64
        } else {
            0.0
        }
    }

    fn get_process_memory(&self, pid: u32) -> Option<u64> {
        if pid == 0 {
            return None;
        }
        let mut system = self.system.lock().unwrap();
        system.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[sysinfo::Pid::from_u32(
            pid,
        )]));
        system
            .process(sysinfo::Pid::from_u32(pid))
            .map(|p| p.memory())
    }

    fn format_memory(bytes: u64) -> String {
        if bytes >= 1024 * 1024 * 1024 {
            format!("{:.1}GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
        } else if bytes >= 1024 * 1024 {
            format!("{:.1}MB", bytes as f64 / (1024.0 * 1024.0))
        } else if bytes >= 1024 {
            format!("{:.1}KB", bytes as f64 / 1024.0)
        } else {
            format!("{}B", bytes)
        }
    }
}

impl Default for Watcher {
    fn default() -> Self {
        Self::new(WatchConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_watcher_creation() {
        let watcher = Watcher::default();
        assert!(!watcher.is_running());
    }

    #[test]
    fn test_state_transitions() {
        let config = WatchConfig {
            stalled_timeout_secs: 1,
            zombie_timeout_secs: 3,
            ..WatchConfig::default()
        };

        let watcher = Watcher::new(config);
        watcher.notify_output();

        let state = watcher.determine_state(0);
        assert_eq!(state, AgentState::Running);

        std::thread::sleep(Duration::from_secs(2));
        let state = watcher.determine_state(0);
        assert_eq!(state, AgentState::Stalled);

        std::thread::sleep(Duration::from_secs(2));
        let state = watcher.determine_state(0);
        assert_eq!(state, AgentState::Zombie);
    }

    #[test]
    fn test_format_memory() {
        assert!(Watcher::format_memory(500).contains("B"));
        assert!(Watcher::format_memory(2 * 1024 * 1024).contains("MB"));
        assert!(Watcher::format_memory(2u64 * 1024 * 1024 * 1024).contains("GB"));
    }

    #[test]
    fn test_heartbeat_idle() {
        let config = WatchConfig {
            heartbeat_timeout_secs: 0,
            ..WatchConfig::default()
        };
        let watcher = Watcher::new(config);

        std::thread::sleep(Duration::from_secs(1));
        let state = watcher.determine_state(0);
        assert_eq!(state, AgentState::Idle);
    }
}
