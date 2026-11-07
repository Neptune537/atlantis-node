use crate::TelemetryPayload;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Gauge, List, ListItem, Paragraph, Row, Table},
    Frame, Terminal,
};
use std::io;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct TuiState {
    pub telemetry: TelemetryPayload,
    pub log_messages: Vec<String>,
    pub peer_count: usize,
    pub dtn_queue_len: usize,
    pub dividend_rate: f64,
    pub audit_balanced: bool,
    pub audit_reconciled: f64,
    pub active_store_nodes: usize,
    pub annual_stake_locked_usd: f64,
    pub monthly_pool_drip_usd: f64,
}

impl Default for TuiState {
    fn default() -> Self {
        Self {
            telemetry: TelemetryPayload {
                cpu_usage_percent: 0.0,
                cpu_temp_celsius: None,
                free_ram_mb: 0,
                total_ram_mb: 0,
                p99_latency_ms: 0.0,
                poh_score: 100.0,
                node_id: "atlantis-node".to_string(),
                raft_term: 1,
                committed_log_index: 0,
            },
            log_messages: Vec::new(),
            peer_count: 0,
            dtn_queue_len: 0,
            dividend_rate: 0.02,
            audit_balanced: true,
            audit_reconciled: 0.0,
            active_store_nodes: 0,
            annual_stake_locked_usd: 0.0,
            monthly_pool_drip_usd: 0.0,
        }
    }
}

pub fn create_shared_tui_state() -> Arc<Mutex<TuiState>> {
    Arc::new(Mutex::new(TuiState::default()))
}

pub fn celsius_to_fahrenheit(celsius: f32) -> f32 {
    (celsius * 9.0 / 5.0) + 32.0
}

pub fn run_tui(shared_state: Arc<Mutex<TuiState>>) -> io::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let tick_rate = Duration::from_millis(250);
    let mut should_quit = false;

    while !should_quit {
        terminal.draw(|f| {
            let state = shared_state.lock().unwrap().clone();
            render_dashboard(f, &state);
        })?;

        if event::poll(tick_rate)? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Esc => {
                        should_quit = true;
                    }
                    _ => {}
                }
            }
        }
    }

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;
    Ok(())
}

fn render_dashboard(f: &mut Frame, state: &TuiState) {
    let size = f.area();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(3),  // Header banner
            Constraint::Length(9),  // American Metrics & Telemetry
            Constraint::Length(7),  // Tide Ledger & Consensus
            Constraint::Min(6),     // Live Log Ring Buffer
            Constraint::Length(2),  // Status & Keybind Help
        ])
        .split(size);

    // 1. Header Banner
    let header = Paragraph::new(Line::from(vec![
        Span::styled("⚡ ATLANTIS PI HOME NODE ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::styled("│ TIER 3 SOVEREIGN EDGE DAEMON │ ", Style::default().fg(Color::DarkGray)),
        Span::styled(format!("Node ID: {} ", state.telemetry.node_id), Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Span::styled("│ KUROSHIO GYRE", Style::default().fg(Color::Blue)),
    ]))
    .alignment(Alignment::Center)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Cyan)),
    );
    f.render_widget(header, chunks[0]);

    // 2. Metrics & Telemetry (American Units: Fahrenheit, MB, %, PoH)
    let metrics_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(50),
            Constraint::Percentage(50),
        ])
        .split(chunks[1]);

    let fahrenheit_display = match state.telemetry.cpu_temp_celsius {
        Some(c) => format!("{:.1} °F ({:.1} °C)", celsius_to_fahrenheit(c), c),
        None => "N/A (Virtual)".to_string(),
    };

    let temp_color = match state.telemetry.cpu_temp_celsius {
        Some(c) if celsius_to_fahrenheit(c) > 165.0 => Color::Red,
        Some(c) if celsius_to_fahrenheit(c) > 140.0 => Color::Yellow,
        _ => Color::Green,
    };

    let ram_used = state.telemetry.total_ram_mb.saturating_sub(state.telemetry.free_ram_mb);
    let ram_ratio = if state.telemetry.total_ram_mb > 0 {
        (ram_used as f64) / (state.telemetry.total_ram_mb as f64)
    } else {
        0.0
    };

    let hardware_rows = vec![
        Row::new(vec![
            Span::raw("CPU Temperature:"),
            Span::styled(fahrenheit_display, Style::default().fg(temp_color).add_modifier(Modifier::BOLD)),
        ]),
        Row::new(vec![
            Span::raw("CPU Load:"),
            Span::styled(format!("{:.1} %", state.telemetry.cpu_usage_percent), Style::default().fg(Color::White)),
        ]),
        Row::new(vec![
            Span::raw("RAM Memory:"),
            Span::styled(
                format!("{} MB / {} MB ({:.1}%)", ram_used, state.telemetry.total_ram_mb, ram_ratio * 100.0),
                Style::default().fg(Color::White),
            ),
        ]),
        Row::new(vec![
            Span::raw("P99 Mesh Latency:"),
            Span::styled(format!("{:.2} ms", state.telemetry.p99_latency_ms), Style::default().fg(Color::Green)),
        ]),
    ];

    let hardware_table = Table::new(hardware_rows, [Constraint::Length(20), Constraint::Min(25)])
        .block(
            Block::default()
                .title(" 🌡️ System Hardware & American Metrics ")
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::Yellow)),
        );
    f.render_widget(hardware_table, metrics_chunks[0]);

    // Gauge right side: PoH Score & RAM Gauge
    let gauge_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(metrics_chunks[1]);

    let poh_gauge = Gauge::default()
        .block(Block::default().title(" 🛡️ Proof-of-Health (PoH) Canary Score ").borders(Borders::ALL).border_type(BorderType::Rounded))
        .gauge_style(Style::default().fg(Color::Green).bg(Color::DarkGray))
        .percent(state.telemetry.poh_score.clamp(0.0, 100.0) as u16)
        .label(format!("{:.1}% PoH", state.telemetry.poh_score));
    f.render_widget(poh_gauge, gauge_layout[0]);

    let ram_gauge = Gauge::default()
        .block(Block::default().title(" 💾 Memory Allocation ").borders(Borders::ALL).border_type(BorderType::Rounded))
        .gauge_style(Style::default().fg(Color::Magenta).bg(Color::DarkGray))
        .percent((ram_ratio * 100.0).clamp(0.0, 100.0) as u16)
        .label(format!("{} MB Used", ram_used));
    f.render_widget(ram_gauge, gauge_layout[1]);

    // 3. Tide Ledger & Consensus Rail
    let ledger_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(50),
            Constraint::Percentage(50),
        ])
        .split(chunks[2]);

    let consensus_rows = vec![
        Row::new(vec![
            Span::raw("OpenRaft Term:"),
            Span::styled(format!("{}", state.telemetry.raft_term), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        ]),
        Row::new(vec![
            Span::raw("Committed Log Index:"),
            Span::styled(format!("{}", state.telemetry.committed_log_index), Style::default().fg(Color::Cyan)),
        ]),
        Row::new(vec![
            Span::raw("Connected Mesh Peers:"),
            Span::styled(format!("{}", state.peer_count), Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
        ]),
    ];

    let consensus_table = Table::new(consensus_rows, [Constraint::Length(24), Constraint::Min(20)])
        .block(
            Block::default()
                .title(" 🤝 Consensus & Rail (Port 9001) ")
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::Cyan)),
        );
    f.render_widget(consensus_table, ledger_chunks[0]);

    let audit_style = if state.audit_balanced {
        Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
    };

    let ledger_rows = vec![
        Row::new(vec![
            Span::raw("Double-Entry Audit:"),
            Span::styled(
                if state.audit_balanced {
                    format!("BALANCED (${:.2})", state.audit_reconciled)
                } else {
                    format!("DESYNC ALERT (${:.2})", state.audit_reconciled)
                },
                audit_style,
            ),
        ]),
        Row::new(vec![
            Span::raw("Club Dividend Rate:"),
            Span::styled(format!("{:.1}%", state.dividend_rate * 100.0), Style::default().fg(Color::Yellow)),
        ]),
        Row::new(vec![
            Span::raw("DTN Delayed Buffer:"),
            Span::styled(format!("{} spooled records", state.dtn_queue_len), Style::default().fg(Color::LightBlue)),
        ]),
        Row::new(vec![
            Span::raw("Store Stake Pool:"),
            Span::styled(
                format!("{} Nodes | ${:.2} Locked | ${:.2}/mo Drip", state.active_store_nodes, state.annual_stake_locked_usd, state.monthly_pool_drip_usd),
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            ),
        ]),
    ];

    let ledger_table = Table::new(ledger_rows, [Constraint::Length(24), Constraint::Min(20)])
        .block(
            Block::default()
                .title(" 💰 Tide Ledger Escrow (Port 8081) ")
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::Green)),
        );
    f.render_widget(ledger_table, ledger_chunks[1]);

    // 4. Live Log Ring Buffer
    let log_items: Vec<ListItem> = state
        .log_messages
        .iter()
        .rev()
        .take(15)
        .map(|msg| {
            let color = if msg.contains("ERROR") {
                Color::Red
            } else if msg.contains("WARN") {
                Color::Yellow
            } else if msg.contains("Heartbeat") {
                Color::DarkGray
            } else {
                Color::White
            };
            ListItem::new(Span::styled(msg.clone(), Style::default().fg(color)))
        })
        .collect();

    let logs_list = List::new(log_items).block(
        Block::default()
            .title(" 📜 Live Event Stream & Heartbeat Logs ")
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::White)),
    );
    f.render_widget(logs_list, chunks[3]);

    // 5. Help Footer
    let footer = Paragraph::new(Line::from(vec![
        Span::styled(" [Q / ESC] ", Style::default().fg(Color::Black).bg(Color::White).add_modifier(Modifier::BOLD)),
        Span::raw(" Exit TUI (Daemon stays running)  "),
        Span::styled(" [R] ", Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::raw(" Force Raft Tick  "),
        Span::styled(" [A] ", Style::default().fg(Color::Black).bg(Color::Green).add_modifier(Modifier::BOLD)),
        Span::raw(" Audit Invariants  "),
        Span::styled(" Units: °Fahrenheit / MB / % ", Style::default().fg(Color::DarkGray)),
    ]))
    .alignment(Alignment::Center);
    f.render_widget(footer, chunks[4]);
}
