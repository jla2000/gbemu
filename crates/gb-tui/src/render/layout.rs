//! Screen + debug panel layout. Includes the min-terminal-size check: if the
//! terminal is smaller than the GB screen area requires, show a resize
//! prompt instead of the emulator view. When `App::debug_overlay` is on,
//! the video area shares the row with the focused debug panel
//! (`App::debug_panel`) to its right.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::app::App;
use crate::debug::overlay::{status_summary, DebugOverlayWidget};
use crate::render::video::{VideoWidget, SCREEN_COLS, SCREEN_ROWS};

/// Minimum terminal size required to render the GB screen at full
/// resolution. Debug panels need more; this is the hard floor.
const MIN_COLS: u16 = SCREEN_COLS;
const MIN_ROWS: u16 = SCREEN_ROWS;

/// Minimum extra width given to a debug panel alongside the video area.
const MIN_DEBUG_PANEL_COLS: u16 = 24;

pub fn draw(frame: &mut Frame, app: &mut App) {
    let area = frame.area();

    if area.width < MIN_COLS || area.height < MIN_ROWS {
        draw_resize_prompt(frame, area);
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(SCREEN_ROWS), Constraint::Length(1)])
        .split(area);
    let content_area = chunks[0];
    let status_area = chunks[1];

    if app.debug_overlay {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(SCREEN_COLS), Constraint::Min(MIN_DEBUG_PANEL_COLS)])
            .split(content_area);
        let framebuffer = app.system.mmu.ppu.framebuffer();
        frame.render_widget(VideoWidget::new(framebuffer, app.palette), cols[0]);
        frame.render_widget(DebugOverlayWidget::new(app), cols[1]);
    } else {
        let framebuffer = app.system.mmu.ppu.framebuffer();
        frame.render_widget(VideoWidget::new(framebuffer, app.palette), content_area);
    }

    draw_status_line(frame, app, status_area);
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
    let text = if app.debug_overlay {
        format!(" {rom} | {} | F12: hide debugger, q: quit ", status_summary(app))
    } else {
        format!(" {rom} | F12: debugger, q: quit ")
    };
    frame.render_widget(Paragraph::new(text), area);
}
