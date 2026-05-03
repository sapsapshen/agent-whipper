use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use std::fs;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhipStats {
    pub total_whips: u64,
    pub successful_rescues: u64,
    pub estimated_time_saved_secs: u64,
    pub whips_today: u64,
    pub today_date: String,
    pub history: Vec<WhipEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhipEntry {
    pub timestamp: DateTime<Local>,
    pub preset_name: String,
    pub agent: String,
    pub state_before: String,
    pub saved_estimated_secs: u64,
}

const AVERAGE_WHIP_SAVE_SECS: u64 = 120;

impl WhipStats {
    pub fn load() -> Result<Self, Box<dyn std::error::Error>> {
        let _lock = StatsFileLock::acquire()?;
        Self::load_unlocked()
    }

    fn load_unlocked() -> Result<Self, Box<dyn std::error::Error>> {
        let path = Self::stats_path()?;
        if path.exists() {
            let content = fs::read_to_string(&path)?;
            let mut stats: WhipStats = serde_json::from_str(&content)?;
            stats.reset_if_new_day();
            Ok(stats)
        } else {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            let stats = Self::default();
            stats.save_unlocked()?;
            Ok(stats)
        }
    }

    pub fn save(&self) -> Result<(), Box<dyn std::error::Error>> {
        let _lock = StatsFileLock::acquire()?;
        self.save_unlocked()
    }

    fn save_unlocked(&self) -> Result<(), Box<dyn std::error::Error>> {
        let path = Self::stats_path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(self)?;
        let temp_path = path.with_extension(format!("tmp-{}", std::process::id()));
        fs::write(&temp_path, content)?;
        replace_file(&temp_path, &path)?;
        Ok(())
    }

    pub fn record_whip(&mut self, entry: WhipEntry) {
        self.total_whips += 1;
        self.whips_today += 1;
        self.estimated_time_saved_secs += entry.saved_estimated_secs;
        self.history.push(entry);

        if self.history.len() > 1000 {
            self.history = self.history[self.history.len() - 500..].to_vec();
        }

        if let Err(e) = self.save() {
            log::warn!("Failed to persist whip stats: {}", e);
        }
    }

    pub fn record_rescue(&mut self, preset_name: &str, agent: &str, state_before: &str) {
        self.successful_rescues += 1;
        let entry = WhipEntry {
            timestamp: Local::now(),
            preset_name: preset_name.to_string(),
            agent: agent.to_string(),
            state_before: state_before.to_string(),
            saved_estimated_secs: AVERAGE_WHIP_SAVE_SECS,
        };
        self.record_whip(entry);
    }

    pub fn record_rescue_global(
        preset_name: &str,
        agent: &str,
        state_before: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let _lock = StatsFileLock::acquire()?;
        let mut stats = Self::load_unlocked()?;
        stats.successful_rescues += 1;
        let entry = WhipEntry {
            timestamp: Local::now(),
            preset_name: preset_name.to_string(),
            agent: agent.to_string(),
            state_before: state_before.to_string(),
            saved_estimated_secs: AVERAGE_WHIP_SAVE_SECS,
        };
        stats.total_whips += 1;
        stats.whips_today += 1;
        stats.estimated_time_saved_secs += entry.saved_estimated_secs;
        stats.history.push(entry);

        if stats.history.len() > 1000 {
            stats.history = stats.history[stats.history.len() - 500..].to_vec();
        }

        stats.save_unlocked()?;
        Ok(())
    }

    fn reset_if_new_day(&mut self) {
        let today = Local::now().format("%Y-%m-%d").to_string();
        if self.today_date != today {
            self.whips_today = 0;
            self.today_date = today;
        }
    }

    fn stats_path() -> Result<PathBuf, Box<dyn std::error::Error>> {
        let data_dir = dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("agentwhipper");
        Ok(data_dir.join("whip_stats.json"))
    }

    pub fn format_time_saved(&self) -> String {
        let secs = self.estimated_time_saved_secs;
        if secs >= 3600 {
            format!("{:.1} 小时", secs as f64 / 3600.0)
        } else if secs >= 60 {
            format!("{} 分钟", secs / 60)
        } else {
            format!("{} 秒", secs)
        }
    }
}

fn replace_file(
    source: &std::path::Path,
    destination: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(windows)]
    unsafe {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::{
            MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
        };

        let source_wide = source
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<u16>>();
        let destination_wide = destination
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<u16>>();

        if MoveFileExW(
            source_wide.as_ptr(),
            destination_wide.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        ) == 0
        {
            return Err(std::io::Error::last_os_error().into());
        }

        Ok(())
    }

    #[cfg(not(windows))]
    {
        fs::rename(source, destination)?;
        Ok(())
    }
}

struct StatsFileLock {
    path: PathBuf,
    token: String,
}

impl StatsFileLock {
    fn acquire() -> Result<Self, Box<dyn std::error::Error>> {
        let path = WhipStats::stats_path()?.with_extension("lock");
        let started = Instant::now();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        loop {
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(mut file) => {
                    let process_start_time =
                        Self::process_start_time(std::process::id()).unwrap_or_default();
                    let token = format!(
                        "{}:{}:{}",
                        std::process::id(),
                        process_start_time,
                        SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs()
                    );
                    writeln!(file, "{}", token)?;
                    return Ok(Self { path, token });
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    if Self::clear_stale_lock(&path)? {
                        continue;
                    }
                    if started.elapsed() >= Duration::from_secs(5) {
                        return Err("Timed out waiting for whip stats lock".into());
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(e) => return Err(Box::new(e)),
            }
        }
    }

    fn clear_stale_lock(path: &PathBuf) -> Result<bool, Box<dyn std::error::Error>> {
        const MALFORMED_LOCK_GRACE: Duration = Duration::from_secs(1);

        let content = match fs::read_to_string(path) {
            Ok(content) => content,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(true),
            Err(_) if Self::lock_is_older_than(path, MALFORMED_LOCK_GRACE) => {
                fs::remove_file(path)?;
                return Ok(true);
            }
            Err(_) => return Ok(false),
        };

        let mut parts = content.trim().split(':');
        let pid = parts.next().and_then(|value| value.parse::<u32>().ok());
        let process_start_time = parts.next().and_then(|value| value.parse::<u64>().ok());

        if pid
            .zip(process_start_time)
            .is_some_and(|(pid, start_time)| !Self::pid_matches_start_time(pid, start_time))
        {
            fs::remove_file(path)?;
            return Ok(true);
        }

        if (pid.is_none() || process_start_time.is_none())
            && Self::lock_is_older_than(path, MALFORMED_LOCK_GRACE)
        {
            fs::remove_file(path)?;
            return Ok(true);
        }

        Ok(false)
    }

    fn lock_is_older_than(path: &PathBuf, grace: Duration) -> bool {
        fs::metadata(path)
            .ok()
            .and_then(|metadata| metadata.modified().ok())
            .and_then(|modified| SystemTime::now().duration_since(modified).ok())
            .is_some_and(|age| age >= grace)
    }

    fn process_start_time(pid: u32) -> Option<u64> {
        if pid == 0 {
            return None;
        }

        let mut system = sysinfo::System::new();
        system.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[sysinfo::Pid::from_u32(
            pid,
        )]));
        system
            .process(sysinfo::Pid::from_u32(pid))
            .map(|process| process.start_time())
    }

    fn pid_matches_start_time(pid: u32, start_time: u64) -> bool {
        Self::process_start_time(pid).is_some_and(|current| current == start_time)
    }
}

impl Drop for StatsFileLock {
    fn drop(&mut self) {
        if fs::read_to_string(&self.path)
            .ok()
            .is_some_and(|content| content.trim() == self.token)
        {
            let _ = fs::remove_file(&self.path);
        }
    }
}

impl Default for WhipStats {
    fn default() -> Self {
        Self {
            total_whips: 0,
            successful_rescues: 0,
            estimated_time_saved_secs: 0,
            whips_today: 0,
            today_date: Local::now().format("%Y-%m-%d").to_string(),
            history: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stats_creation() {
        let stats = WhipStats::default();
        assert_eq!(stats.total_whips, 0);
        assert_eq!(stats.whips_today, 0);
    }

    #[test]
    fn test_record_whip() {
        let mut stats = WhipStats::default();
        stats.record_rescue("speedup", "codex", "STALLED");
        assert_eq!(stats.total_whips, 1);
        assert_eq!(stats.successful_rescues, 1);
        assert_eq!(stats.whips_today, 1);
        assert!(stats.history.len() == 1);
    }

    #[test]
    fn test_format_time() {
        let mut stats = WhipStats::default();
        assert_eq!(stats.format_time_saved(), "0 秒");

        stats.estimated_time_saved_secs = 120;
        assert_eq!(stats.format_time_saved(), "2 分钟");

        stats.estimated_time_saved_secs = 7200;
        assert_eq!(stats.format_time_saved(), "2.0 小时");
    }
}
