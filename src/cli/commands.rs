use crate::core::config::{AgentState, SupervisorConfig, WatchMode};
use crate::core::supervisor::Supervisor;
use crate::plugins::{build_bridge_manager, detect_running_runtimes, DetectedRuntime};
use crate::presets::manager::{Preset, PresetManager, PresetStep};
use crate::utils::stats::{WhipEntry, WhipStats};
use crate::utils::{
    inject_text_via_ui, resolve_ui_target_pid, send_ctrl_c_via_ui, send_ctrl_d_via_ui,
    send_enter_via_ui,
};
use chrono::Local;
use clap::{Parser, Subcommand};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

#[derive(Parser)]
#[command(name = "whip")]
#[command(about = "AgentWhipper - AI智能体监督与干预工具 🎯")]
#[command(version = "0.2.0")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    Start {
        agent: String,
        #[arg(long, default_value = "watch")]
        mode: String,
        #[arg(long, hide = true)]
        model: Option<String>,
    },
    #[command(hide = true)]
    Attach {
        session_id: String,
    },
    #[command(hide = true)]
    Watch {
        #[arg(long)]
        all: bool,
        #[arg(long)]
        session_id: Option<String>,
    },
    #[command(hide = true)]
    Inject {
        session_id: String,
        command: Vec<String>,
        #[arg(long)]
        preset: Option<String>,
    },
    Whip {
        #[arg(long, default_value = "speedup")]
        preset: String,
    },
    Preset {
        #[command(subcommand)]
        action: PresetAction,
    },
    Stats,
    #[command(hide = true)]
    Status {
        session_id: Option<String>,
    },
    History,
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
}

#[derive(Subcommand)]
pub enum PresetAction {
    List,
    #[command(hide = true)]
    Run {
        name: String,
        #[arg(long)]
        session_id: Option<String>,
    },
    Create {
        name: String,
        #[arg(long)]
        description: Option<String>,
        #[arg(long, default_value = "stalled")]
        trigger_on: String,
    },
    #[command(hide = true)]
    Edit {
        name: String,
    },
}

#[derive(Subcommand)]
pub enum ConfigAction {
    Show,
    #[command(hide = true)]
    Edit,
}

fn configured_preset_dir() -> Result<String, Box<dyn std::error::Error>> {
    Ok(SupervisorConfig::load()?
        .resolved_preset_dir()?
        .to_string_lossy()
        .into_owned())
}

pub fn handle_start(
    agent: &str,
    mode: &str,
    model: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut config = SupervisorConfig::load()?;
    if !SupervisorConfig::is_supported_agent(agent) {
        return Err(format!(
            "Unsupported agent '{}'. Supported agents: codex, claude, hermes.",
            agent
        )
        .into());
    }
    config.agent = agent.to_string();
    config.mode = WatchMode::parse(mode)?;

    if let Some(m) = model {
        return Err(format!(
            "--model {} is not wired to agent launch yet; omit it until model-specific spawning is implemented",
            m
        )
        .into());
    }

    if config.mode == WatchMode::Passive {
        return Err(
            "Passive mode is not supported yet because sessions cannot persist across CLI invocations"
                .into(),
        );
    }

    let mut supervisor = Supervisor::new(config)?;
    supervisor.start_agent()?;

    println!("AgentWhipper started");
    println!("  Agent:     {}", agent);
    println!("  Mode:      {}", mode);
    println!("  Session:   {}", supervisor.session_id);
    println!("  PID:       {:?}", supervisor.get_pid());

    println!("\nMonitoring active. Press Ctrl+C to stop.");

    let shutdown = Arc::new(std::sync::atomic::AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&shutdown)).ok();

    while supervisor.running.load(std::sync::atomic::Ordering::SeqCst)
        && !shutdown.load(std::sync::atomic::Ordering::SeqCst)
    {
        std::thread::sleep(std::time::Duration::from_secs(2));
        let state = supervisor.get_state();
        if state == AgentState::Stopped {
            break;
        }
        let stats = supervisor.get_stats();
        if state != AgentState::Running {
            println!(
                "  [{}] {} | CPU: {:.1}% | Mem: {}",
                state.emoji(),
                state.as_str(),
                stats.cpu_percent,
                stats.memory_formatted
            );
        }
    }

    let unexpected_exit = !shutdown.load(std::sync::atomic::Ordering::SeqCst)
        && supervisor.get_state() == AgentState::Stopped;
    supervisor.stop()?;

    if unexpected_exit {
        return Err("Agent process exited unexpectedly.".into());
    }

    println!("\nAgentWhipper stopped.");

    Ok(())
}

pub fn handle_attach(session_id: &str) -> Result<(), Box<dyn std::error::Error>> {
    Err(format!(
        "Attach is not supported for session {} until persistent session storage is implemented",
        session_id
    )
    .into())
}

pub fn handle_watch(all: bool, session_id: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
    if !all && session_id.is_none() {
        return Err("Either --all or --session-id is required".into());
    }

    if all {
        return handle_watch_all();
    } else if let Some(sid) = session_id {
        return Err(format!(
            "Live HUD for session {} is not implemented because sessions are not persisted yet",
            sid
        )
        .into());
    }

    Ok(())
}

fn handle_watch_all() -> Result<(), Box<dyn std::error::Error>> {
    let config = SupervisorConfig::load()?;
    let shutdown = Arc::new(std::sync::atomic::AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&shutdown)).ok();

    println!("AgentWhipper all-runtime watch started");
    println!("  Scope: all detectable agent runtimes");
    println!("  Launch dependency: none");
    println!();
    println!("Monitoring active. Press Ctrl+C to stop.");

    let mut last_snapshot = String::new();
    while !shutdown.load(std::sync::atomic::Ordering::SeqCst) {
        let runtimes = detect_running_runtimes(&config);
        let snapshot = format_runtime_snapshot(&runtimes);

        if snapshot != last_snapshot {
            println!();
            println!("[{}] {}", Local::now().format("%H:%M:%S"), snapshot);
            last_snapshot = snapshot;
        }

        std::thread::sleep(Duration::from_secs(3));
    }

    println!();
    println!("AgentWhipper all-runtime watch stopped.");
    Ok(())
}

fn format_runtime_snapshot(runtimes: &[DetectedRuntime]) -> String {
    if runtimes.is_empty() {
        return "No detectable agent runtimes are currently running.".to_string();
    }

    runtimes
        .iter()
        .map(|runtime| {
            let pids = if runtime.pids.is_empty() {
                "bridge".to_string()
            } else {
                runtime
                    .pids
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(",")
            };

            format!("{} [{}]", runtime.display_name, pids)
        })
        .collect::<Vec<_>>()
        .join("; ")
}

pub fn handle_inject(
    session_id: &str,
    command: &[String],
    preset_name: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    if preset_name.is_none() && command.is_empty() {
        return Err("Either a command or --preset is required".into());
    }

    Err(format!(
        "Injecting into session {} is not implemented until live session lookup is available",
        session_id
    )
    .into())
}

pub fn handle_whip(preset_name: &str) -> Result<(), Box<dyn std::error::Error>> {
    let config = SupervisorConfig::load()?;
    println!("{}", crate::utils::whip_crack_animation());
    println!();
    println!("💥 啪！ 挥鞭检查已触发。");
    println!("🎯 执行预设: {}", preset_name);

    let mut pm = PresetManager::new(configured_preset_dir()?);
    pm.load_builtins()?;
    pm.load_from_dir()?;

    let preset = pm.get_preset(preset_name)?.clone();
    println!("📝 描述: {}", preset.description);
    println!("📊 步骤数: {}", preset.steps.len());
    println!("🔁 最大重试: {}", preset.max_retries);

    let runtimes: Vec<DetectedRuntime> = detect_running_runtimes(&config)
        .into_iter()
        .filter(|runtime| runtime.can_accelerate)
        .collect();
    if runtimes.is_empty() {
        return Err("未检测到正在运行的可加速 agent 运行时。".into());
    }

    println!();
    println!("🔎 检测到 {} 类运行时，开始注入预设。", runtimes.len());

    let mut seen_targets = HashSet::new();
    let mut successes = Vec::new();
    let mut failures = Vec::new();

    for runtime in runtimes {
        let target_key = runtime_target_key(&runtime);
        if !seen_targets.insert(target_key) {
            println!(
                "↪️ {}：共享同一注入目标，跳过重复注入。",
                runtime.display_name
            );
            continue;
        }

        match execute_preset_via_runtime(&config, &preset, &runtime) {
            Ok(channel) => {
                println!("✅ {}：已通过 {} 注入。", runtime.display_name, channel);
                successes.push(runtime.display_name.clone());
            }
            Err(error) => {
                println!("⚠️ {}：{}", runtime.display_name, error);
                failures.push(format!("{}: {}", runtime.display_name, error));
            }
        }
    }

    if successes.is_empty() {
        return Err(format!("未能注入到任何运行时：{}", failures.join("; ")).into());
    }

    println!();
    println!("🎉 本次已加速: {}", successes.join("、"));
    record_manual_whip(preset_name, &successes)?;

    Ok(())
}

fn runtime_target_key(runtime: &DetectedRuntime) -> String {
    for pid in &runtime.pids {
        if let Ok(target_pid) = resolve_ui_target_pid(*pid) {
            return format!("window:{target_pid}");
        }
    }

    if let Some(pid) = runtime.pids.first() {
        return format!("pid:{pid}");
    }

    format!("bridge:{}", runtime.key)
}

fn record_manual_whip(
    preset_name: &str,
    accelerated_runtimes: &[String],
) -> Result<(), Box<dyn std::error::Error>> {
    let mut stats = WhipStats::load()?;
    stats.record_whip(WhipEntry {
        timestamp: Local::now(),
        preset_name: preset_name.to_string(),
        agent: accelerated_runtimes.join(", "),
        state_before: "manual-whip".to_string(),
        saved_estimated_secs: 120,
    });
    Ok(())
}

fn execute_preset_via_runtime(
    config: &SupervisorConfig,
    preset: &Preset,
    runtime: &DetectedRuntime,
) -> Result<String, String> {
    if !runtime.pids.is_empty() {
        let mut ui_errors = Vec::new();
        for pid in &runtime.pids {
            match execute_preset_via_ui(*pid, preset) {
                Ok(()) => return Ok(format!("UI(PID {})", pid)),
                Err(error) => ui_errors.push(format!("PID {}: {}", pid, error)),
            }
        }

        return Err(format!("ui: {}", ui_errors.join("; ")));
    }

    execute_preset_via_bridge(config, preset, runtime)?;
    Ok("bridge".to_string())
}

fn execute_preset_via_bridge(
    config: &SupervisorConfig,
    preset: &Preset,
    runtime: &DetectedRuntime,
) -> Result<(), String> {
    let manager = build_bridge_manager(config);
    let Some(bridge) = manager.get(&runtime.key) else {
        return Err("no direct bridge".to_string());
    };

    let mut skip_next_enter = false;
    for step in &preset.steps {
        match step {
            PresetStep::Text { .. } => {
                let text = step
                    .get_text_content()
                    .filter(|value| !value.trim().is_empty())
                    .ok_or_else(|| "preset text step is empty".to_string())?;
                bridge
                    .send_command(text)
                    .map_err(|error| error.to_string())?;
                skip_next_enter = true;
            }
            PresetStep::Enter => {
                if skip_next_enter {
                    skip_next_enter = false;
                } else {
                    bridge
                        .send_keys("Enter")
                        .map_err(|error| error.to_string())?;
                }
            }
            PresetStep::CtrlC => {
                bridge.send_keys("C-c").map_err(|error| error.to_string())?;
                skip_next_enter = false;
            }
            PresetStep::CtrlD => {
                bridge.send_keys("C-d").map_err(|error| error.to_string())?;
                skip_next_enter = false;
            }
            PresetStep::Wait { duration_secs } => {
                std::thread::sleep(Duration::from_secs_f64(*duration_secs));
            }
            PresetStep::Exec { content } => {
                return Err(format!("Exec preset steps remain disabled: {}", content));
            }
            PresetStep::Signal { signal_name } => {
                let keys = match signal_name.to_uppercase().as_str() {
                    "SIGINT" | "SIGTERM" | "SIGKILL" => "C-c",
                    other => return Err(format!("unsupported preset signal: {}", other)),
                };
                bridge.send_keys(keys).map_err(|error| error.to_string())?;
                skip_next_enter = false;
            }
        }
    }

    Ok(())
}

fn execute_preset_via_ui(pid: u32, preset: &Preset) -> Result<(), String> {
    for step in &preset.steps {
        match step {
            PresetStep::Text { .. } => {
                let text = step
                    .get_text_content()
                    .filter(|value| !value.trim().is_empty())
                    .ok_or_else(|| "preset text step is empty".to_string())?;
                inject_text_via_ui(pid, text)?;
            }
            PresetStep::Enter => send_enter_via_ui(pid)?,
            PresetStep::CtrlC => send_ctrl_c_via_ui(pid)?,
            PresetStep::CtrlD => send_ctrl_d_via_ui(pid)?,
            PresetStep::Wait { duration_secs } => {
                std::thread::sleep(Duration::from_secs_f64(*duration_secs));
            }
            PresetStep::Exec { content } => {
                return Err(format!("Exec preset steps remain disabled: {}", content));
            }
            PresetStep::Signal { signal_name } => match signal_name.to_uppercase().as_str() {
                "SIGINT" | "SIGTERM" | "SIGKILL" => send_ctrl_c_via_ui(pid)?,
                other => return Err(format!("unsupported preset signal: {}", other)),
            },
        }

        std::thread::sleep(Duration::from_millis(50));
    }

    Ok(())
}

pub fn handle_preset_list() -> Result<(), Box<dyn std::error::Error>> {
    let mut pm = PresetManager::new(configured_preset_dir()?);
    pm.load_builtins()?;
    pm.load_from_dir()?;

    println!("Available presets:");
    println!("==================");
    for preset in pm.list_presets() {
        let triggers: Vec<String> = preset
            .trigger_on
            .iter()
            .map(|s| s.as_str().to_string())
            .collect();
        println!(
            "  {} - {} (triggers: {}, retries: {})",
            preset.name,
            preset.description,
            triggers.join(", "),
            preset.max_retries
        );
    }
    println!("\nTotal: {} presets", pm.count());
    Ok(())
}

pub fn handle_preset_run(
    name: &str,
    session_id: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(sid) = session_id {
        return Err(format!(
            "Running preset '{}' against session {} is not available until persistent session lookup is implemented",
            name, sid
        )
        .into());
    }

    handle_whip(name)
}

pub fn handle_preset_create(
    name: &str,
    description: Option<&str>,
    trigger_on: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut pm = PresetManager::new(configured_preset_dir()?);
    pm.load_builtins().ok();

    let states = parse_trigger_states(trigger_on)?;
    let preset = pm.create_preset(
        name,
        description.unwrap_or("User-defined preset"),
        states,
        3,
        vec![crate::presets::manager::PresetStep::Text {
            content: vec![String::new()],
        }],
    );

    pm.save_preset_to_dir(&preset)?;
    let preset_path = pm.preset_path_for_name(name);
    println!("Preset '{}' created.", name);
    println!("Edit {} to customize steps.", preset_path.display());
    Ok(())
}

pub fn handle_preset_edit(name: &str) -> Result<(), Box<dyn std::error::Error>> {
    let pm = PresetManager::new(configured_preset_dir()?);
    let path = pm.preset_path_for_name(name);
    if path.exists() {
        Err(format!(
            "Preset edit is not implemented. Open this file manually: {}",
            path.display()
        )
        .into())
    } else {
        Err(format!(
            "Preset '{}' not found. Use 'whip preset create {}' first.",
            name, name
        )
        .into())
    }
}

pub fn handle_status(session_id: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
    match session_id {
        Some(sid) => Err(format!(
            "Status for session {} is not available until persistent session tracking is implemented",
            sid
        )
        .into()),
        None => Err("Listing active sessions is not implemented yet".into()),
    }
}

pub fn handle_stats() -> Result<(), Box<dyn std::error::Error>> {
    let stats = WhipStats::load()?;

    println!("╔══════════════════════════════════════════════╗");
    println!("║          📊  AgentWhipper 统计数据          ║");
    println!("╠══════════════════════════════════════════════╣");
    println!("║                                              ║");
    println!(
        "║  🔥 总鞭打次数:        {:>12}          ║",
        stats.total_whips
    );
    println!(
        "║  🧟 成功拯救假死:      {:>12}          ║",
        stats.successful_rescues
    );
    println!(
        "║  ⏱️ 预估节省时间:     {:>12}          ║",
        stats.format_time_saved()
    );
    println!("║                                              ║");
    println!(
        "║  📅 今日鞭打:          {:>12}          ║",
        stats.whips_today
    );
    println!(
        "║  📊 今日日期:          {:>12}          ║",
        stats.today_date
    );
    println!("║                                              ║");
    println!("╚══════════════════════════════════════════════╝");

    if !stats.history.is_empty() {
        println!("\n最近 5 次鞭打记录:");
        println!("─────────────────────────────────────────");
        let recent: Vec<&crate::utils::stats::WhipEntry> =
            stats.history.iter().rev().take(5).collect();
        for entry in recent {
            println!(
                "  {} | {:<16} | {:<14} | {} -> {:>8}s",
                entry.timestamp.format("%Y-%m-%d %H:%M"),
                entry.preset_name,
                entry.agent,
                entry.state_before,
                entry.saved_estimated_secs
            );
        }
    }

    Ok(())
}

pub fn handle_history() -> Result<(), Box<dyn std::error::Error>> {
    let stats = WhipStats::load()?;

    if stats.history.is_empty() {
        println!("Intervention history:");
        println!("=====================");
        println!("  (no history recorded yet)");
        println!(
            "\nStart monitoring with 'whip start codex --mode watch' to record interventions."
        );
        return Ok(());
    }

    println!("Intervention history:");
    println!("=====================");
    println!("  Total records: {}", stats.history.len());
    println!();

    for (i, entry) in stats.history.iter().rev().take(20).enumerate() {
        println!(
            "  {}. [{}] {} | preset: {} | state: {}",
            i + 1,
            entry.timestamp.format("%Y-%m-%d %H:%M:%S"),
            entry.agent,
            entry.preset_name,
            entry.state_before
        );
    }

    if stats.history.len() > 20 {
        println!("\n  ... and {} more entries", stats.history.len() - 20);
    }

    Ok(())
}

pub fn handle_config_show() -> Result<(), Box<dyn std::error::Error>> {
    let config = SupervisorConfig::load()?;
    println!("AgentWhipper Configuration");
    println!("==========================");
    println!("Agent:              {}", config.agent);
    println!("Mode:               {:?}", config.mode);
    println!("Poll interval:      {}ms", config.watch.poll_interval_ms);
    println!("Stalled timeout:    {}s", config.watch.stalled_timeout_secs);
    println!("Zombie timeout:     {}s", config.watch.zombie_timeout_secs);
    println!(
        "CPU threshold:      {}%",
        config.watch.cpu_threshold_percent
    );
    println!("Auto intervene:     {}", config.watch.auto_intervene);
    println!("Char delay:         {}ms", config.inject.char_delay_ms);
    println!("Max command len:    {}", config.inject.max_command_length);
    println!("Adaptive delay:     {}", config.inject.adaptive_delay);
    println!("Hot reload presets: {}", config.preset.hot_reload);

    println!("\nBridges:");
    for (name, bridge) in &config.bridges {
        println!("  {}: enabled={}", name, bridge.enabled);
    }

    Ok(())
}

pub fn handle_config_edit() -> Result<(), Box<dyn std::error::Error>> {
    let config_path = SupervisorConfig::resolved_config_path()?;

    Err(format!(
        "Config edit is not implemented. Open this file manually: {}",
        config_path.display()
    )
    .into())
}

pub fn parse_trigger_states(input: &str) -> Result<Vec<AgentState>, Box<dyn std::error::Error>> {
    let mut states = Vec::new();
    let mut invalid = Vec::new();

    for token in input
        .split(',')
        .map(str::trim)
        .filter(|token| !token.is_empty())
    {
        match token.to_uppercase().as_str() {
            "RUNNING" => states.push(AgentState::Running),
            "STALLED" => states.push(AgentState::Stalled),
            "ZOMBIE" => states.push(AgentState::Zombie),
            "IDLE" => states.push(AgentState::Idle),
            "STOPPED" => states.push(AgentState::Stopped),
            _ => invalid.push(token.to_string()),
        }
    }

    if !invalid.is_empty() {
        return Err(format!(
            "Invalid trigger states: {}. Use only RUNNING, STALLED, ZOMBIE, IDLE, STOPPED.",
            invalid.join(", ")
        )
        .into());
    }

    if states.is_empty() {
        return Err("No valid states specified. Use: RUNNING,STALLED,ZOMBIE,IDLE,STOPPED".into());
    }
    Ok(states)
}

#[cfg(test)]
mod tests {
    #[test]
    fn commands_source_has_no_disabled_execution_marker() {
        let source = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/cli/commands.rs"));
        let marker = concat!("dry", "-run");
        let legacy_label = concat!("预设", "预览");
        assert!(!source.to_ascii_lowercase().contains(marker));
        assert!(!source.contains(legacy_label));
    }

    #[test]
    fn readme_has_no_disabled_execution_marker() {
        let readme = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/README.md"));
        let marker = concat!("dry", "-run");
        assert!(!readme.to_ascii_lowercase().contains(marker));
    }
}
