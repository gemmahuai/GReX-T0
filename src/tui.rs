//! Terminal interface to monitor the behavior of T0, WIP (not working yet)

use crossterm::event::{self, Event, KeyCode};
use std::io::stdout;
use tui::{
    backend::{Backend, CrosstermBackend},
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Style},
    widgets::{Block, BorderType, Borders},
    Frame, Terminal,
};
use tui_logger::{TuiLoggerLevelOutput, TuiLoggerWidget};

fn run_app<B: Backend>(terminal: &mut Terminal<B>) -> anyhow::Result<()> {
    loop {
        terminal.draw(ui)?;

        if let Event::Key(key) = event::read()? {
            if let KeyCode::Char('q') = key.code {
                return Ok(());
            }
        }
    }
}

fn ui<B: Backend>(f: &mut Frame<B>) {
    let size = f.size();
    // Surrounding block
    let block = Block::default()
        .borders(Borders::ALL)
        .title("GReX T0")
        .title_alignment(Alignment::Center)
        .border_type(BorderType::Rounded);
    f.render_widget(block, size);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(4)
        .constraints([Constraint::Percentage(80), Constraint::Percentage(20)].as_ref())
        .split(f.size());

    let tui_w: TuiLoggerWidget = TuiLoggerWidget::default()
        .block(
            Block::default()
                .title("Independent Tui Logger View")
                .border_style(Style::default().fg(Color::White).bg(Color::Black))
                .borders(Borders::ALL),
        )
        .output_separator('|')
        .output_timestamp(Some("%F %H:%M:%S%.3f".to_string()))
        .output_level(Some(TuiLoggerLevelOutput::Long))
        .output_target(false)
        .output_file(false)
        .output_line(false)
        .style(Style::default().fg(Color::White).bg(Color::Black));
    f.render_widget(tui_w, chunks[1]);
}

pub struct Tui {}

impl Tui {
    pub fn start() -> anyhow::Result<()> {
        // Configure Crossterm backend for tui
        let stdout = stdout();
        crossterm::terminal::enable_raw_mode()?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;
        terminal.clear()?;
        terminal.hide_cursor()?;

        // create app and run it
        let res = run_app(&mut terminal);

        // Restore the terminal and close application
        terminal.clear()?;
        terminal.show_cursor()?;
        crossterm::terminal::disable_raw_mode()?;

        Ok(())
    }
}
