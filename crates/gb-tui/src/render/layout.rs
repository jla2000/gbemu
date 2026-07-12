//! Screen + status sidebar + debug panel layout. Includes the
//! min-terminal-size check: if the terminal is smaller than required,
//! show a resize prompt instead of the emulator view.
//!
//! The video area and the always-on status sidebar (`debug::status`)
//! share the top row unconditionally, regardless of `App::debug_overlay`.
//! The heavier, F12-toggled debug panel (`App::debug_panel`, laid out by
//! `DebugOverlayWidget`) renders full-width *below* that row instead of
//! beside it, so toggling it on doesn't require an even wider terminal —
//! only a taller one.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::app::App;
use crate::debug::overlay::DebugOverlayWidget;
use crate::debug::status::{StatusSidebarWidget, SIDEBAR_COLS};
use crate::render::video::{VideoWidget, SCREEN_COLS, SCREEN_ROWS};

/// Minimum terminal size required to render the GB screen and the
/// always-on status sidebar at full size. The heavier debug panel needs
/// more height; see [`MIN_ROWS_WITH_OVERLAY`].
const MIN_COLS: u16 = SCREEN_COLS + SIDEBAR_COLS;
const MIN_ROWS: u16 = SCREEN_ROWS;

/// Minimum extra rows given to the F12-toggled debug panel, below the
/// video+sidebar row, when `App::debug_overlay` is on.
const MIN_OVERLAY_ROWS: u16 = 15;
const MIN_ROWS_WITH_OVERLAY: u16 = SCREEN_ROWS + MIN_OVERLAY_ROWS;

pub fn draw(frame: &mut Frame, app: &mut App) {
    let area = frame.area();

    let min_rows = if app.debug_overlay { MIN_ROWS_WITH_OVERLAY } else { MIN_ROWS };
    if area.width < MIN_COLS || area.height < min_rows {
        draw_resize_prompt(frame, area, min_rows);
        return;
    }

    let chunks = if app.debug_overlay {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(SCREEN_ROWS),
                Constraint::Min(MIN_OVERLAY_ROWS),
                Constraint::Length(1),
            ])
            .split(area)
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(SCREEN_ROWS), Constraint::Length(1)])
            .split(area)
    };
    let top_row = chunks[0];
    let status_area = chunks[chunks.len() - 1];

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(SCREEN_COLS), Constraint::Length(SIDEBAR_COLS)])
        .split(top_row);
    let framebuffer = app.system.mmu.ppu.framebuffer();
    frame.render_widget(VideoWidget::new(framebuffer, app.palette), cols[0]);
    frame.render_widget(StatusSidebarWidget::new(app), cols[1]);

    if app.debug_overlay {
        frame.render_widget(DebugOverlayWidget::new(app), chunks[1]);
    }

    draw_status_line(frame, app, status_area);
}

fn draw_resize_prompt(frame: &mut Frame, area: Rect, min_rows: u16) {
    let msg = format!(
        "Terminal too small. Need at least {MIN_COLS}x{min_rows}, have {}x{}. Resize to continue.",
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
        format!(" {rom} | Tab: panel  Space: step  F: frame  F5: run/pause  B: breakpoint@PC  W: watch  F12: hide debugger, q: quit ")
    } else {
        format!(" {rom} | F12: debugger, q: quit ")
    };
    frame.render_widget(Paragraph::new(text), area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    use crate::palette::Palette;

    #[test]
    fn min_cols_covers_the_video_area_and_the_full_sidebar_width() {
        assert_eq!(MIN_COLS, SCREEN_COLS + SIDEBAR_COLS);
    }

    #[test]
    fn resize_prompt_shows_when_narrower_than_min_cols() {
        let backend = TestBackend::new(MIN_COLS - 1, MIN_ROWS + 10);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(None, Palette::Classic);
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let content = terminal.backend().buffer().content.iter().map(|c| c.symbol()).collect::<String>();
        assert!(content.contains("too small"));
    }

    #[test]
    fn resize_prompt_requires_extra_rows_once_the_debug_overlay_is_on() {
        let backend = TestBackend::new(MIN_COLS, MIN_ROWS + 5); // enough with overlay off...
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(None, Palette::Classic);
        app.debug_overlay = true; // ...but not with it on
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let content = terminal.backend().buffer().content.iter().map(|c| c.symbol()).collect::<String>();
        assert!(content.contains("too small"));
    }

    #[test]
    fn full_layout_renders_without_panicking_once_large_enough() {
        let backend = TestBackend::new(MIN_COLS, MIN_ROWS_WITH_OVERLAY);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(None, Palette::Classic);
        app.debug_overlay = true;
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    }
}
