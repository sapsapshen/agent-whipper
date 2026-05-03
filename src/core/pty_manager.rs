use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const OUTPUT_BUFFER_MAX_BYTES: usize = 64 * 1024;

pub struct PtySession {
    master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
    child: Arc<Mutex<Box<dyn Child + Send + Sync>>>,
    pub pid: u32,
    pub session_id: String,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    output_buffer: Arc<Mutex<Vec<u8>>>,
    output_bytes_received: Arc<AtomicU64>,
    output_thread: Arc<Mutex<Option<std::thread::JoinHandle<()>>>>,
}

impl PtySession {
    pub fn spawn(
        command: &[String],
        session_id: &str,
        rows: u16,
        cols: u16,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let command = crate::utils::prepare_command_for_pty(command)
            .map_err(|e| format!("Failed to prepare PTY command: {}", e))?;
        let pty_system = native_pty_system();
        let pty_size = PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        };

        let pair = pty_system.openpty(pty_size)?;

        let mut cmd = CommandBuilder::new(command[0].as_str());
        for arg in &command[1..] {
            cmd.arg(arg);
        }
        cmd.cwd(std::env::current_dir()?);
        cmd.env("TERM", "xterm-256color");

        let child = pair.slave.spawn_command(cmd)?;
        let pid = child.process_id().unwrap_or(0);
        let master = pair.master;
        let reader = master.try_clone_reader()?;
        let writer = master.take_writer()?;
        let output_buffer = Arc::new(Mutex::new(Vec::new()));
        let output_bytes_received = Arc::new(AtomicU64::new(0));
        let output_thread = Arc::new(Mutex::new(Some(Self::spawn_output_pump(
            reader,
            Arc::clone(&output_buffer),
            Arc::clone(&output_bytes_received),
            session_id.to_string(),
        ))));

        Ok(Self {
            master: Arc::new(Mutex::new(master)),
            child: Arc::new(Mutex::new(child)),
            pid,
            session_id: session_id.to_string(),
            writer: Arc::new(Mutex::new(writer)),
            output_buffer,
            output_bytes_received,
            output_thread,
        })
    }

    pub fn attach(pty_path: &str, session_id: &str) -> Result<Self, Box<dyn std::error::Error>> {
        Err(format!(
            "Attaching to an existing PTY ({pty_path}) is not implemented for session {session_id}"
        )
        .into())
    }

    pub fn read_output(&self) -> Result<String, Box<dyn std::error::Error>> {
        let bytes = {
            let mut buffer = self.output_buffer.lock().unwrap();
            if buffer.is_empty() {
                return Ok(String::new());
            }
            buffer.drain(..).collect::<Vec<u8>>()
        };
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    fn spawn_output_pump(
        mut reader: Box<dyn Read + Send>,
        output_buffer: Arc<Mutex<Vec<u8>>>,
        output_bytes_received: Arc<AtomicU64>,
        session_id: String,
    ) -> std::thread::JoinHandle<()> {
        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];

            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        output_bytes_received.fetch_add(n as u64, Ordering::SeqCst);
                        let mut buffer = output_buffer.lock().unwrap();
                        buffer.extend_from_slice(&buf[..n]);
                        if buffer.len() > OUTPUT_BUFFER_MAX_BYTES {
                            let overflow = buffer.len() - OUTPUT_BUFFER_MAX_BYTES;
                            buffer.drain(..overflow);
                        }
                    }
                    Err(e) => {
                        log::debug!("PTY output pump stopped for session {}: {}", session_id, e);
                        break;
                    }
                }
            }
        })
    }

    pub fn is_alive(&self) -> Result<bool, Box<dyn std::error::Error>> {
        let mut child = self.child.lock().unwrap();
        Ok(child.try_wait()?.is_none())
    }

    pub fn output_bytes_received(&self) -> u64 {
        self.output_bytes_received.load(Ordering::SeqCst)
    }

    pub fn wait_for_exit(&self) -> Result<(), Box<dyn std::error::Error>> {
        let mut child = self.child.lock().unwrap();
        child.wait()?;
        Ok(())
    }

    fn join_output_thread(&self) {
        if let Ok(mut handle) = self.output_thread.lock() {
            if let Some(handle) = handle.take() {
                let _ = handle.join();
            }
        }
    }

    pub fn write_input(&self, data: &str) -> Result<(), Box<dyn std::error::Error>> {
        let mut writer = self.writer.lock().unwrap();
        writer.write_all(data.as_bytes())?;
        writer.flush()?;
        Ok(())
    }

    pub fn write_char_by_char(
        &self,
        data: &str,
        delay_ms: u64,
    ) -> Result<(), Box<dyn std::error::Error>> {
        for ch in data.chars() {
            let mut buf = [0u8; 4];
            let encoded = ch.encode_utf8(&mut buf);
            {
                let mut writer = self.writer.lock().unwrap();
                writer.write_all(encoded.as_bytes())?;
                writer.flush()?;
            }
            std::thread::sleep(Duration::from_millis(delay_ms));
        }
        Ok(())
    }

    pub fn send_signal(&self, signal: PtySignal) -> Result<(), Box<dyn std::error::Error>> {
        match signal {
            PtySignal::CtrlC => {
                self.write_input("\x03")?;
            }
            PtySignal::CtrlD => {
                self.write_input("\x04")?;
            }
            PtySignal::Enter => {
                self.write_input("\r\n")?;
            }
            PtySignal::Sigterm => {
                #[cfg(unix)]
                unsafe {
                    if self.pid > 0 {
                        if libc::kill(self.pid as i32, libc::SIGTERM) != 0 {
                            return Err(std::io::Error::last_os_error().into());
                        }
                    } else {
                        return Err("Cannot signal PTY process without a PID".into());
                    }
                }
                #[cfg(windows)]
                {
                    self.kill()?;
                }
            }
            PtySignal::Sigkill => {
                #[cfg(unix)]
                unsafe {
                    if self.pid > 0 {
                        if libc::kill(self.pid as i32, libc::SIGKILL) != 0 {
                            return Err(std::io::Error::last_os_error().into());
                        }
                    } else {
                        return Err("Cannot signal PTY process without a PID".into());
                    }
                }
                #[cfg(windows)]
                {
                    self.kill()?;
                }
            }
        }
        Ok(())
    }

    pub fn kill(&self) -> Result<(), Box<dyn std::error::Error>> {
        let already_exited = {
            let mut child = self.child.lock().unwrap();
            if child.try_wait()?.is_some() {
                true
            } else {
                #[cfg(unix)]
                {
                    let master = self.master.lock().unwrap();
                    if let Some(group_leader) = master.process_group_leader() {
                        if libc::kill(-group_leader, libc::SIGKILL) == 0 {
                            false
                        } else {
                            child.kill()?;
                            false
                        }
                    } else {
                        child.kill()?;
                        false
                    }
                }
                #[cfg(windows)]
                {
                    let taskkill = std::env::var("SystemRoot")
                        .map(std::path::PathBuf::from)
                        .unwrap_or_else(|_| std::path::PathBuf::from(r"C:\Windows"))
                        .join("System32")
                        .join("taskkill.exe");
                    let status = std::process::Command::new(taskkill)
                        .args(["/PID", &self.pid.to_string(), "/T", "/F"])
                        .status()?;
                    if !status.success() {
                        child.kill()?;
                    }
                    false
                }
                #[cfg(not(any(unix, windows)))]
                child.kill()?;
                #[cfg(not(any(unix, windows)))]
                false
            }
        };

        if !already_exited {
            let _ = self.wait_for_exit();
        }

        self.join_output_thread();
        Ok(())
    }

    pub fn get_output_buffer(&self) -> Vec<u8> {
        self.output_buffer.lock().unwrap().clone()
    }

    pub fn clear_output_buffer(&self) {
        self.output_buffer.lock().unwrap().clear();
    }

    pub fn resize(&self, rows: u16, cols: u16) -> Result<(), Box<dyn std::error::Error>> {
        let size = PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        };
        let master = self.master.lock().unwrap();
        master.resize(size)?;
        Ok(())
    }
}

impl Drop for PtySession {
    fn drop(&mut self) {
        let _ = self.kill();
    }
}

pub enum PtySignal {
    CtrlC,
    CtrlD,
    Enter,
    Sigterm,
    Sigkill,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_command() -> Vec<String> {
        if cfg!(windows) {
            vec![
                "cmd.exe".to_string(),
                "/c".to_string(),
                "echo hello".to_string(),
            ]
        } else {
            vec!["echo".to_string(), "hello".to_string()]
        }
    }

    #[test]
    #[ignore]
    fn test_pty_spawn() {
        let session = PtySession::spawn(&test_command(), "test-1", 24, 80);
        assert!(session.is_ok());
    }

    #[test]
    #[ignore]
    fn test_pty_write_read() {
        if cfg!(windows) {
            return;
        }
        let session = PtySession::spawn(&["cat".to_string()], "test-2", 24, 80);
        if let Ok(session) = session {
            session.write_input("hello\n").ok();
            std::thread::sleep(Duration::from_millis(200));
            let output = session.read_output().unwrap_or_default();
            assert!(output.contains("hello"));
        }
    }
}
