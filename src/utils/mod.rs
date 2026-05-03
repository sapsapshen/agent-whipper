pub mod stats;
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn which_command(cmd: &str) -> bool {
    resolve_executable(cmd).is_ok()
}

#[cfg(windows)]
pub fn inject_text_via_ui(pid: u32, text: &str) -> Result<(), String> {
    if pid == 0 {
        return Err("Cannot inject into PID 0".to_string());
    }
    if text.trim().is_empty() {
        return Err("Cannot inject an empty prompt".to_string());
    }

    run_ui_injector(pid, "paste", Some(text), None, false)
}

#[cfg(windows)]
pub fn resolve_ui_target_pid(pid: u32) -> Result<u32, String> {
    if pid == 0 {
        return Err("Cannot resolve PID 0".to_string());
    }

    let script = r#"
$ErrorActionPreference = 'Stop'

function Get-InjectableProcess {
    param([int]$StartPid)

    $visited = New-Object 'System.Collections.Generic.HashSet[int]'
    $currentPid = $StartPid

    while ($currentPid -gt 0 -and -not $visited.Contains($currentPid)) {
        [void]$visited.Add($currentPid)

        $procInfo = Get-CimInstance Win32_Process -Filter "ProcessId = $currentPid"
        try {
            $process = [System.Diagnostics.Process]::GetProcessById($currentPid)
            $process.Refresh()
        } catch {
            $process = $null
        }

        if ($process -and $process.MainWindowHandle -ne [IntPtr]::Zero) {
            return $process
        }

        if (-not $procInfo -or -not $procInfo.ParentProcessId) {
            break
        }
        $currentPid = [int]$procInfo.ParentProcessId
    }

    throw "PID $StartPid and its parent chain have no main window handle"
}

$targetPid = [int][Environment]::GetEnvironmentVariable('AGENTWHIPPER_TARGET_PID')
$process = Get-InjectableProcess -StartPid $targetPid
Write-Output $process.Id
"#;

    let output = Command::new("powershell.exe")
        .args(["-NoLogo", "-NoProfile", "-Command", script])
        .env("AGENTWHIPPER_TARGET_PID", pid.to_string())
        .output()
        .map_err(|e| format!("Failed to start PowerShell PID resolver: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let detail = if !stderr.is_empty() {
            stderr
        } else if !stdout.is_empty() {
            stdout
        } else {
            format!("PowerShell PID resolver exited with {}", output.status)
        };
        return Err(detail);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout.trim().parse::<u32>().map_err(|error| {
        format!(
            "Failed to parse resolved PID '{}': {}",
            stdout.trim(),
            error
        )
    })
}

#[cfg(windows)]
pub fn send_enter_via_ui(pid: u32) -> Result<(), String> {
    run_ui_injector(pid, "key", None, Some(0x0D), false)
}

#[cfg(windows)]
pub fn send_ctrl_c_via_ui(pid: u32) -> Result<(), String> {
    run_ui_injector(pid, "key", None, Some(0x43), true)
}

#[cfg(windows)]
pub fn send_ctrl_d_via_ui(pid: u32) -> Result<(), String> {
    run_ui_injector(pid, "key", None, Some(0x44), true)
}

#[cfg(windows)]
fn run_ui_injector(
    pid: u32,
    action: &str,
    text: Option<&str>,
    key_code: Option<u32>,
    control_modifier: bool,
) -> Result<(), String> {
    let script = r#"
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms
Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class AgentWhipperNativeWindow {
    [DllImport("user32.dll")]
    public static extern bool PostMessage(IntPtr hWnd, uint Msg, IntPtr wParam, IntPtr lParam);
    [DllImport("user32.dll")]
    public static extern IntPtr SendMessage(IntPtr hWnd, uint Msg, IntPtr wParam, IntPtr lParam);
}
"@

function Get-InjectableProcess {
    param([int]$StartPid)

    $visited = New-Object 'System.Collections.Generic.HashSet[int]'
    $currentPid = $StartPid

    while ($currentPid -gt 0 -and -not $visited.Contains($currentPid)) {
        [void]$visited.Add($currentPid)

        $procInfo = Get-CimInstance Win32_Process -Filter "ProcessId = $currentPid"
        try {
            $process = [System.Diagnostics.Process]::GetProcessById($currentPid)
            $process.Refresh()
        } catch {
            $process = $null
        }

        if ($process -and $process.MainWindowHandle -ne [IntPtr]::Zero) {
            return $process
        }

        if (-not $procInfo -or -not $procInfo.ParentProcessId) {
            break
        }
        $currentPid = [int]$procInfo.ParentProcessId
    }

    throw "PID $StartPid and its parent chain have no main window handle"
}

$targetPid = [int][Environment]::GetEnvironmentVariable('AGENTWHIPPER_TARGET_PID')
$prompt = [Environment]::GetEnvironmentVariable('AGENTWHIPPER_PROMPT')
$process = Get-InjectableProcess -StartPid $targetPid
$handle = $process.MainWindowHandle

$action = [Environment]::GetEnvironmentVariable('AGENTWHIPPER_UI_ACTION')
$prompt = [Environment]::GetEnvironmentVariable('AGENTWHIPPER_PROMPT')
$virtualKeyText = [Environment]::GetEnvironmentVariable('AGENTWHIPPER_VIRTUAL_KEY')
$controlModifier = [Environment]::GetEnvironmentVariable('AGENTWHIPPER_CONTROL_MODIFIER') -eq '1'

$WM_KEYDOWN = 0x0100
$WM_KEYUP = 0x0101
$WM_PASTE = 0x0302
$VK_CONTROL = 0x11

if ($action -eq 'paste') {
    [System.Windows.Forms.Clipboard]::SetText($prompt)
    [AgentWhipperNativeWindow]::SendMessage($handle, $WM_PASTE, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null
    exit 0
}

if ($action -eq 'key') {
    if (-not $virtualKeyText) {
        throw 'Missing virtual key'
    }

    $virtualKey = [int]$virtualKeyText
    if ($controlModifier) {
        [AgentWhipperNativeWindow]::PostMessage($handle, $WM_KEYDOWN, [IntPtr]$VK_CONTROL, [IntPtr]::Zero) | Out-Null
    }
    [AgentWhipperNativeWindow]::PostMessage($handle, $WM_KEYDOWN, [IntPtr]$virtualKey, [IntPtr]::Zero) | Out-Null
    [AgentWhipperNativeWindow]::PostMessage($handle, $WM_KEYUP, [IntPtr]$virtualKey, [IntPtr]::Zero) | Out-Null
    if ($controlModifier) {
        [AgentWhipperNativeWindow]::PostMessage($handle, $WM_KEYUP, [IntPtr]$VK_CONTROL, [IntPtr]::Zero) | Out-Null
    }
    exit 0
}

throw "Unsupported UI action: $action"
"#;

    let output = Command::new("powershell.exe")
        .args(["-NoLogo", "-NoProfile", "-STA", "-Command", script])
        .env("AGENTWHIPPER_TARGET_PID", pid.to_string())
        .env("AGENTWHIPPER_UI_ACTION", action)
        .env("AGENTWHIPPER_PROMPT", text.unwrap_or_default())
        .env(
            "AGENTWHIPPER_VIRTUAL_KEY",
            key_code.map(|value| value.to_string()).unwrap_or_default(),
        )
        .env(
            "AGENTWHIPPER_CONTROL_MODIFIER",
            if control_modifier { "1" } else { "0" },
        )
        .output()
        .map_err(|e| format!("Failed to start PowerShell UI injector: {}", e))?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let detail = if !stderr.is_empty() {
        stderr
    } else if !stdout.is_empty() {
        stdout
    } else {
        format!("PowerShell UI injector exited with {}", output.status)
    };
    Err(detail)
}

#[cfg(not(windows))]
pub fn inject_text_via_ui(_pid: u32, _text: &str) -> Result<(), String> {
    Err("UI injection is currently implemented for Windows only".to_string())
}

#[cfg(not(windows))]
pub fn resolve_ui_target_pid(_pid: u32) -> Result<u32, String> {
    Err("UI injection is currently implemented for Windows only".to_string())
}

#[cfg(not(windows))]
pub fn send_enter_via_ui(_pid: u32) -> Result<(), String> {
    Err("UI injection is currently implemented for Windows only".to_string())
}

#[cfg(not(windows))]
pub fn send_ctrl_c_via_ui(_pid: u32) -> Result<(), String> {
    Err("UI injection is currently implemented for Windows only".to_string())
}

#[cfg(not(windows))]
pub fn send_ctrl_d_via_ui(_pid: u32) -> Result<(), String> {
    Err("UI injection is currently implemented for Windows only".to_string())
}

pub fn prepare_command_for_pty(command: &[String]) -> Result<Vec<String>, String> {
    if command.is_empty() {
        return Err("Cannot spawn an empty command".to_string());
    }

    let executable = resolve_executable(&command[0])?;
    let mut prepared = Vec::new();

    #[cfg(windows)]
    {
        if is_cmd_script(&executable) {
            prepared.push("cmd.exe".to_string());
            prepared.push("/d".to_string());
            prepared.push("/c".to_string());
            prepared.push(executable.to_string_lossy().into_owned());
            prepared.extend(command.iter().skip(1).cloned());
            return Ok(prepared);
        }

        if is_powershell_script(&executable) {
            prepared.push("powershell.exe".to_string());
            prepared.push("-NoLogo".to_string());
            prepared.push("-NoProfile".to_string());
            prepared.push("-File".to_string());
            prepared.push(executable.to_string_lossy().into_owned());
            prepared.extend(command.iter().skip(1).cloned());
            return Ok(prepared);
        }
    }

    prepared.push(executable.to_string_lossy().into_owned());
    prepared.extend(command.iter().skip(1).cloned());
    Ok(prepared)
}

fn resolve_executable(executable: &str) -> Result<PathBuf, String> {
    let candidate = PathBuf::from(executable);
    if looks_like_path(&candidate) {
        return resolve_explicit_executable(&candidate);
    }

    let locator = if cfg!(windows) { "where.exe" } else { "which" };
    let output = Command::new(locator)
        .arg(executable)
        .output()
        .map_err(|e| format!("Failed to resolve command '{}': {}", executable, e))?;

    if !output.status.success() {
        return Err(format!(
            "Command '{}' was not found in PATH. Install it first or pass an explicit executable path.",
            executable
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let candidates: Vec<PathBuf> = stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(PathBuf::from)
        .collect();

    pick_resolved_executable(executable, candidates)
}

fn resolve_explicit_executable(candidate: &Path) -> Result<PathBuf, String> {
    #[cfg(windows)]
    {
        let mut candidates = Vec::new();

        if candidate.extension().is_none() {
            for extension in ["exe", "com", "cmd", "bat", "ps1"] {
                let with_extension = candidate.with_extension(extension);
                if with_extension.is_file() {
                    candidates.push(with_extension);
                }
            }
        }

        if candidate.is_file() {
            candidates.push(candidate.to_path_buf());
        }

        if candidates.is_empty() {
            return Err(format!("Executable not found: {}", candidate.display()));
        }

        pick_resolved_executable(&candidate.display().to_string(), candidates)
    }

    #[cfg(not(windows))]
    {
        if candidate.is_file() {
            Ok(candidate.to_path_buf())
        } else {
            Err(format!("Executable not found: {}", candidate.display()))
        }
    }
}

fn pick_resolved_executable(executable: &str, candidates: Vec<PathBuf>) -> Result<PathBuf, String> {
    #[cfg(windows)]
    {
        if let Some(resolved) = choose_windows_executable(&candidates) {
            return Ok(resolved);
        }

        if let Some(first) = candidates.first() {
            return Err(format!(
                "Command '{}' resolved to '{}', but that file is not directly executable on Windows. Use the matching .exe, .cmd, .bat, or .ps1 launcher instead.",
                executable,
                first.display()
            ));
        }
    }

    candidates
        .into_iter()
        .next()
        .ok_or_else(|| format!("Command '{}' was not found in PATH", executable))
}

fn looks_like_path(path: &Path) -> bool {
    path.is_absolute() || path.components().count() > 1
}

#[cfg(windows)]
fn is_cmd_script(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("cmd") || ext.eq_ignore_ascii_case("bat"))
}

#[cfg(windows)]
fn is_powershell_script(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("ps1"))
}

#[cfg(windows)]
fn choose_windows_executable(candidates: &[PathBuf]) -> Option<PathBuf> {
    let cwd = std::env::current_dir().ok();

    for extension in ["exe", "com", "cmd", "bat", "ps1"] {
        if let Some(candidate) = candidates.iter().find(|candidate| {
            let matches_extension = candidate
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case(extension));
            let outside_cwd = cwd.as_ref().is_none_or(|cwd| !candidate.starts_with(cwd));

            matches_extension && outside_cwd
        }) {
            return Some(candidate.clone());
        }
    }

    None
}

pub fn format_duration(secs: u64) -> String {
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
    }
}

pub fn truncate_str(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len - 3])
    }
}

pub fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

pub fn terminate_pid(pid: u32) -> Result<(), String> {
    if pid == 0 {
        return Err("Cannot terminate PID 0".to_string());
    }

    #[cfg(unix)]
    unsafe {
        if libc::kill(pid as i32, libc::SIGTERM) == 0 {
            return Ok(());
        }

        return Err(std::io::Error::last_os_error().to_string());
    }

    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::{
            OpenProcess, TerminateProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE,
            PROCESS_TERMINATE,
        };

        let handle = OpenProcess(PROCESS_TERMINATE | PROCESS_SYNCHRONIZE, 0, pid);
        if handle == 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }

        let terminated = TerminateProcess(handle, 1);
        let result = if terminated == 0 {
            Err(std::io::Error::last_os_error().to_string())
        } else {
            let _ = WaitForSingleObject(handle, 5_000);
            Ok(())
        };

        CloseHandle(handle);
        return result;
    }

    #[allow(unreachable_code)]
    Err("Process termination is not supported on this platform".to_string())
}

pub fn whip_crack_animation() -> &'static str {
    r#"
    💥💥💥  *CRACK!*  💥💥💥
   ╱  ╱  ╱              ╲  ╲  ╲
  ╱  ╱  ╱    啪！        ╲  ╲  ╲
 ╱  ╱  ╱                  ╲  ╲  ╲
╱  ╱  ╱    鞭子已抽下！    ╲  ╲  ╲
╲  ╲  ╲                    ╱  ╱  ╱
 ╲  ╲  ╲   别卡了，继续！  ╱  ╱  ╱
  ╲  ╲  ╲                ╱  ╱  ╱
   ╲  ╲  ╲              ╱  ╱  ╱
"#
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_executable_rejects_missing_command() {
        assert!(resolve_executable("agentwhipper-command-that-does-not-exist").is_err());
    }

    #[cfg(windows)]
    #[test]
    fn test_pick_resolved_executable_prefers_windows_launchers() {
        let resolved = pick_resolved_executable(
            "codex",
            vec![
                PathBuf::from(r"C:\Users\demo\AppData\Roaming\npm\codex"),
                PathBuf::from(r"C:\Users\demo\AppData\Roaming\npm\codex.ps1"),
                PathBuf::from(r"C:\Users\demo\AppData\Roaming\npm\codex.cmd"),
            ],
        )
        .unwrap();

        assert_eq!(
            resolved.extension().and_then(|ext| ext.to_str()),
            Some("cmd")
        );
    }

    #[cfg(windows)]
    #[test]
    fn test_prepare_command_for_pty_wraps_batch_launchers() {
        let tempdir = tempfile::tempdir().unwrap();
        let launcher = tempdir.path().join("codex.cmd");
        std::fs::write(&launcher, "@echo off\r\n").unwrap();

        let prepared =
            prepare_command_for_pty(&[launcher.to_string_lossy().into_owned(), "exec".to_string()])
                .unwrap();

        assert_eq!(prepared[0], "cmd.exe");
        assert_eq!(prepared[1], "/d");
        assert_eq!(prepared[2], "/c");
        assert_eq!(prepared[3], launcher.to_string_lossy());
        assert_eq!(prepared[4], "exec");
    }
}
