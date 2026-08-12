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
use tokio::sync::mpsc::UnboundedReceiver;
use yeokja_translate::orchestrator::ProgressEvent;

#[derive(Debug, Clone, Default)]
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
    pub fn total_segments(&self) -> usize {
        self.files.iter().map(|f| f.total).sum()
    }

    pub fn translated_segments(&self) -> usize {
        self.files.iter().map(|f| f.translated).sum()
    }
}

pub type SharedProgress = Arc<Mutex<TranslationProgress>>;

pub fn create_shared_progress() -> SharedProgress {
    Arc::new(Mutex::new(TranslationProgress::default()))
}

/// Apply orchestrator progress events to the shared TUI state.
/// Runs until the sender side (the translation run) closes the channel.
pub async fn consume_progress_events(
    mut rx: UnboundedReceiver<ProgressEvent>,
    progress: SharedProgress,
) {
    while let Some(event) = rx.recv().await {
        let mut p = progress.lock().unwrap();
        match event {
            ProgressEvent::FilesDiscovered { files } => {
                p.files = files
                    .into_iter()
                    .map(|(path, pending)| FileProgress {
                        path: path.display().to_string(),
                        total: pending,
                        translated: 0,
                        status: if pending == 0 {
                            FileStatus::Done
                        } else {
                            FileStatus::Waiting
                        },
                    })
                    .collect();
            }
            ProgressEvent::FileStarted { file } => {
                let path = file.display().to_string();
                if let Some(f) = p.files.iter_mut().find(|f| f.path == path) {
                    f.status = FileStatus::Translating;
                }
            }
            ProgressEvent::BlockTranslated {
                file,
                segments,
                current,
            } => {
                let path = file.display().to_string();
                if let Some(f) = p.files.iter_mut().find(|f| f.path == path) {
                    f.translated = (f.translated + segments).min(f.total);
                }
                if current.is_some() {
                    p.current_segment = current;
                }
            }
            ProgressEvent::FileCompleted { file } => {
                let path = file.display().to_string();
                if let Some(f) = p.files.iter_mut().find(|f| f.path == path)
                    && f.status != FileStatus::Done {
                        f.status = FileStatus::Done;
                    }
            }
            ProgressEvent::FileFailed { file, error } => {
                let path = file.display().to_string();
                p.errors.push(format!("{path}: {error}"));
                if let Some(f) = p.files.iter_mut().find(|f| f.path == path) {
                    f.status = FileStatus::Error(error);
                }
            }
            ProgressEvent::Finished { .. } => {
                p.is_complete = true;
                p.current_segment = None;
            }
        }
    }
    progress.lock().unwrap().is_complete = true;
}

/// Run the TUI event loop. Blocks until the user presses 'q'.
/// Returns `true` if the user quit before the translation completed.
pub fn run_tui(progress: SharedProgress) -> anyhow::Result<bool> {
    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;

    let cancelled = loop {
        terminal.draw(|frame| {
            let progress = progress.lock().unwrap();
            render_ui(frame, &progress);
        })?;

        // Poll for events with timeout so the view refreshes while translating.
        if event::poll(Duration::from_millis(100))?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
            && key.code == KeyCode::Char('q')
        {
            let is_complete = progress.lock().unwrap().is_complete;
            break !is_complete;
        }
    };

    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;
    Ok(cancelled)
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
    let current_text: String = if current.chars().count() > 80 {
        let truncated: String = current.chars().take(77).collect();
        format!("{truncated}...")
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
