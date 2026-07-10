//! Log panel: renders the most recent lines from `log_ring`'s in-memory
//! `tracing` buffer.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph, Widget};

/// The last `max_lines` log lines, oldest first (matches what's visible
/// scrolling to the bottom of the panel).
pub fn recent_lines(max_lines: usize) -> Vec<String> {
    let all = crate::log_ring::snapshot();
    let start = all.len().saturating_sub(max_lines);
    all[start..].to_vec()
}

pub struct LogPanelWidget;

impl Widget for LogPanelWidget {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let block = Block::default().borders(Borders::ALL).title("Log");
        let rows = area.height.saturating_sub(2) as usize;
        let text: Vec<Line> = recent_lines(rows).into_iter().map(Line::from).collect();
        Paragraph::new(text).block(block).render(area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_lines_never_exceeds_the_requested_cap() {
        // log_ring's buffer is a shared global that other tests running
        // in parallel also write to via tracing, so this only checks the
        // cap invariant, not exact content.
        let out = recent_lines(3);
        assert!(out.len() <= 3);
    }
}
