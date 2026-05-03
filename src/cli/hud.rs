use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub fn run_hud(session_id: &str) -> Result<(), Box<dyn std::error::Error>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;

    let shutdown = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&shutdown)).ok();

    let app = HudApp::new(session_id.to_string());
    let result = run_app(stdout, app, shutdown);

    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture)?;

    result
}

struct HudApp {
    session_id: String,
    state: String,
    state_emoji: String,
    pid: u32,
    uptime: Duration,
    cpu: f64,
    memory: String,
    turn_count: u64,
    interventions: Vec<InterventionEntry>,
    start_time: Instant,
}

#[derive(Clone)]
struct InterventionEntry {
    name: String,
    time: String,
}

impl HudApp {
    fn new(session_id: String) -> Self {
        Self {
            session_id,
            state: "RUNNING".to_string(),
            state_emoji: "🟢".to_string(),
            pid: 0,
            uptime: Duration::ZERO,
            cpu: 0.0,
            memory: "N/A".to_string(),
            turn_count: 0,
            interventions: Vec::new(),
            start_time: Instant::now(),
        }
    }

    fn tick(&mut self) {
        self.uptime = self.start_time.elapsed();
        self.cpu = (self.cpu + 0.1) % 100.0;
        self.turn_count = self.turn_count.saturating_add(1);

        if self.uptime.as_secs() > 30 && self.uptime.as_secs() < 120 && self.state == "RUNNING" {
            self.state = "STALLED".to_string();
            self.state_emoji = "🟡".to_string();
        } else if self.uptime.as_secs() >= 120 && self.state != "ZOMBIE" {
            self.state = "ZOMBIE".to_string();
            self.state_emoji = "🔴".to_string();
        }
    }

    fn add_intervention(&mut self, name: &str) {
        self.interventions.push(InterventionEntry {
            name: name.to_string(),
            time: format!("{}m", self.uptime.as_secs() / 60),
        });
    }
}

fn run_app(
    stdout: io::Stdout,
    mut app: HudApp,
    shutdown: Arc<AtomicBool>,
) -> Result<(), Box<dyn std::error::Error>> {
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = ratatui::Terminal::new(backend)?;
    let tick_rate = Duration::from_millis(100);
    let mut last_tick = Instant::now();

    while !shutdown.load(Ordering::SeqCst) {
        terminal.draw(|f| ui(f, &app))?;

        let timeout = tick_rate.saturating_sub(last_tick.elapsed());
        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') => {
                            shutdown.store(true, Ordering::SeqCst);
                        }
                        KeyCode::Char('w') => {
                            app.add_intervention("manual-inject");
                        }
                        KeyCode::Char('c') => {
                            app.add_intervention("ctrl-c");
                        }
                        KeyCode::Char('p') => {
                            app.add_intervention("preset");
                        }
                        KeyCode::Char('r') => {
                            app.add_intervention("restart");
                        }
                        _ => {}
                    }
                }
            }
        }

        if last_tick.elapsed() >= tick_rate {
            app.tick();
            last_tick = Instant::now();
        }
    }

    Ok(())
}

fn ui(f: &mut Frame, app: &HudApp) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(5),
            Constraint::Min(0),
            Constraint::Length(3),
        ])
        .split(f.area());

    render_header(f, chunks[0], app);
    render_status(f, chunks[1], app);
    render_interventions(f, chunks[2], app);
    render_footer(f, chunks[3]);
}

fn render_header(f: &mut Frame, area: Rect, app: &HudApp) {
    let header_text = format!(
        "AgentWhipper HUD | Session: {} | PID: {} | Uptime: {}",
        app.session_id,
        app.pid,
        crate::utils::format_duration(app.uptime.as_secs())
    );

    let header = Paragraph::new(header_text)
        .block(Block::new().borders(Borders::ALL))
        .style(Style::default().fg(Color::Cyan));
    f.render_widget(header, area);
}

fn render_status(f: &mut Frame, area: Rect, app: &HudApp) {
    let state_color = match app.state.as_str() {
        "RUNNING" => Color::Green,
        "STALLED" => Color::Yellow,
        "ZOMBIE" => Color::Red,
        "IDLE" => Color::Gray,
        _ => Color::White,
    };

    let mut status_spans = vec![
        Span::raw("Status: "),
        Span::styled(
            format!("{} {}", app.state_emoji, app.state),
            Style::default().fg(state_color),
        ),
    ];

    if app.state != "RUNNING" {
        status_spans.push(Span::raw(format!(
            " ({}s no output)",
            app.uptime.as_secs().saturating_sub(30)
        )));
    }

    let status_line = Line::from(status_spans);
    let stats_line = Line::from(vec![Span::raw(format!(
        "CPU: {:.1}% | Mem: {} | Turn #{}",
        app.cpu, app.memory, app.turn_count
    ))]);

    let status = Paragraph::new(vec![status_line, stats_line])
        .block(Block::new().borders(Borders::ALL).title("Status"));
    f.render_widget(status, area);
}

fn render_interventions(f: &mut Frame, area: Rect, app: &HudApp) {
    let mut lines: Vec<Line> = Vec::new();
    let summary = format!("Interventions: {}", app.interventions.len());
    lines.push(Line::from(Span::styled(
        summary,
        Style::default().fg(Color::Yellow),
    )));

    for intervention in &app.interventions {
        lines.push(Line::from(format!(
            "  - {} @{}",
            intervention.name, intervention.time
        )));
    }

    let interventions =
        Paragraph::new(lines).block(Block::new().borders(Borders::ALL).title("Interventions"));
    f.render_widget(interventions, area);
}

fn render_footer(f: &mut Frame, area: Rect) {
    let footer = Paragraph::new("[w]inject [c]trl+c [p]reset [q]uit [r]estart")
        .block(Block::new().borders(Borders::ALL))
        .style(Style::default().fg(Color::DarkGray));
    f.render_widget(footer, area);
}
