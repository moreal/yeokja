// TUI integration is pending: this module is not yet wired into the translate command.
#![allow(dead_code, clippy::collapsible_if)]

use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::{
    prelude::*,
    widgets::*,
};
use std::io::stdout;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct TranslationProgress {
    pub files: Vec<FileProgress>,
    pub current_segment: Option<String>,
    pub errors: Vec<String>,
    pub is_complete: bool,
}

#[derive(Debug, Clone)]
pub struct FileProgress {
    pub path: String,
    pub total: usize,
    pub translated: usize,
    pub pending: usize,
    pub status: FileStatus,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FileStatus {
    Waiting,
    Translating,
    Done,
    Error(String),
}

impl TranslationProgress {
    pub fn new() -> Self {
        Self {
            files: Vec::new(),
            current_segment: None,
            errors: Vec::new(),
            is_complete: false,
        }
    }

    pub fn total_segments(&self) -> usize {
        self.files.iter().map(|f| f.total).sum()
    }

    pub fn translated_segments(&self) -> usize {
        self.files.iter().map(|f| f.translated).sum()
    }
}

pub type SharedProgress = Arc<Mutex<TranslationProgress>>;

pub fn create_shared_progress() -> SharedProgress {
    Arc::new(Mutex::new(TranslationProgress::new()))
}

/// Run the TUI event loop. This blocks until translation is complete or user presses 'q'.
pub fn run_tui(progress: SharedProgress) -> anyhow::Result<()> {
    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;

    loop {
        terminal.draw(|frame| {
            let progress = progress.lock().unwrap();
            render_ui(frame, &progress);
        })?;

        // Poll for events with timeout
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press && key.code == KeyCode::Char('q') {
                    break;
                }
            }
        }

        // Check if translation is complete
        let is_complete = progress.lock().unwrap().is_complete;
        if is_complete {
            // Show final state for a moment, then wait for 'q'
            terminal.draw(|frame| {
                let progress = progress.lock().unwrap();
                render_ui(frame, &progress);
            })?;

            // Wait for user to press 'q' to exit
            loop {
                if let Event::Key(key) = event::read()? {
                    if key.kind == KeyEventKind::Press && key.code == KeyCode::Char('q') {
                        break;
                    }
                }
            }
            break;
        }
    }

    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;
    Ok(())
}

fn render_ui(frame: &mut Frame, progress: &TranslationProgress) {
    let area = frame.area();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(3),  // Title
            Constraint::Length(3),  // Overall progress
            Constraint::Min(8),    // File list
            Constraint::Length(3),  // Current segment
            Constraint::Length(3),  // Status bar
        ])
        .split(area);

    // Title
    let title = Paragraph::new("Yeokja — Translation Progress")
        .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        .block(Block::default().borders(Borders::BOTTOM));
    frame.render_widget(title, chunks[0]);

    // Overall progress bar
    let total = progress.total_segments();
    let translated = progress.translated_segments();
    let pct = if total > 0 { (translated as f64 / total as f64) * 100.0 } else { 0.0 };
    let gauge = Gauge::default()
        .block(Block::default().title(format!(" Progress: {translated}/{total} segments ")).borders(Borders::ALL))
        .gauge_style(Style::default().fg(Color::Green))
        .percent(pct as u16)
        .label(format!("{pct:.1}%"));
    frame.render_widget(gauge, chunks[1]);

    // File list
    let widths = [
        Constraint::Percentage(40),
        Constraint::Length(12),
        Constraint::Length(8),
        Constraint::Percentage(30),
    ];

    let rows: Vec<Row> = progress
        .files
        .iter()
        .map(|f| {
            let status_str = match &f.status {
                FileStatus::Waiting => "Waiting".to_string(),
                FileStatus::Translating => "Translating".to_string(),
                FileStatus::Done => "Done".to_string(),
                FileStatus::Error(e) => format!("Error: {e}"),
            };
            let file_pct = if f.total > 0 {
                format!("{:.0}%", (f.translated as f64 / f.total as f64) * 100.0)
            } else {
                "-".to_string()
            };
            Row::new(vec![
                Cell::from(f.path.clone()),
                Cell::from(format!("{}/{}", f.translated, f.total)),
                Cell::from(file_pct),
                Cell::from(status_str),
            ])
        })
        .collect();

    let table = Table::new(rows, widths)
        .header(
            Row::new(vec!["File", "Progress", "%", "Status"])
                .style(Style::default().add_modifier(Modifier::BOLD))
                .bottom_margin(1),
        )
        .block(Block::default().title(" Files ").borders(Borders::ALL));
    frame.render_widget(table, chunks[2]);

    // Current segment
    let current = progress
        .current_segment
        .as_deref()
        .unwrap_or("Idle");
    let current_text = if current.len() > 80 {
        format!("{}...", &current[..77])
    } else {
        current.to_string()
    };
    let current_widget = Paragraph::new(current_text)
        .block(Block::default().title(" Current Segment ").borders(Borders::ALL))
        .style(Style::default().fg(Color::Yellow));
    frame.render_widget(current_widget, chunks[3]);

    // Status bar
    let status_text = if progress.is_complete {
        format!("Translation complete. {} error(s). Press 'q' to exit.", progress.errors.len())
    } else {
        format!("Translating... {} error(s) so far. Press 'q' to cancel.", progress.errors.len())
    };
    let status = Paragraph::new(status_text)
        .style(Style::default().fg(if progress.errors.is_empty() { Color::Green } else { Color::Red }));
    frame.render_widget(status, chunks[4]);
}
