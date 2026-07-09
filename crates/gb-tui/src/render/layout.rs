//! Screen + debug panel layout. Includes the min-terminal-size check: if the
//! terminal is smaller than the GB screen area requires, show a resize
//! prompt instead of the emulator view. Full debug panel layout lands in
//! M6.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::app::App;
use crate::render::video::{VideoWidget, SCREEN_COLS, SCREEN_ROWS};

/// Minimum terminal size required to render the GB screen at full
/// resolution. Debug panels need more; this is the hard floor.
const MIN_COLS: u16 = SCREEN_COLS;
const MIN_ROWS: u16 = SCREEN_ROWS;

pub fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();

    if area.width < MIN_COLS || area.height < MIN_ROWS {
        draw_resize_prompt(frame, area);
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(SCREEN_ROWS), Constraint::Length(1)])
        .split(area);

    frame.render_widget(VideoWidget, chunks[0]);
    draw_status_line(frame, app, chunks[1]);
}

fn draw_resize_prompt(frame: &mut Frame, area: Rect) {
    let msg = format!(
        "Terminal too small. Need at least {MIN_COLS}x{MIN_ROWS}, have {}x{}. Resize to continue.",
        area.width, area.height
    );
    let block = Block::default().borders(Borders::ALL).title("gbemu");
    let paragraph = Paragraph::new(msg)
        .block(block)
        .style(Style::default().fg(Color::Yellow));
    frame.render_widget(paragraph, area);
}

fn draw_status_line(frame: &mut Frame, app: &App, area: Rect) {
    let rom = app
        .rom_path
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "no ROM loaded".to_string());
    let text = format!(" {rom} | q to quit ");
    frame.render_widget(Paragraph::new(text), area);
}
