//! In-memory ring buffer for log lines, fed by `tracing`. The log panel
//! widget (M6) reads from this buffer. No file output — stdout/stderr are
//! owned by the TUI.

use std::collections::VecDeque;
use std::fmt;
use std::sync::{Mutex, OnceLock};

use tracing::field::{Field, Visit};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::layer::{Context, Layer};

const MAX_LINES: usize = 1000;

static LOG_BUFFER: OnceLock<Mutex<VecDeque<String>>> = OnceLock::new();

fn buffer() -> &'static Mutex<VecDeque<String>> {
    LOG_BUFFER.get_or_init(|| Mutex::new(VecDeque::with_capacity(MAX_LINES)))
}

/// Snapshot of the most recent log lines, oldest first.
pub fn snapshot() -> Vec<String> {
    buffer().lock().unwrap().iter().cloned().collect()
}

fn push_line(line: String) {
    let mut buf = buffer().lock().unwrap();
    if buf.len() >= MAX_LINES {
        buf.pop_front();
    }
    buf.push_back(line);
}

/// A `tracing_subscriber::Layer` that appends formatted events to the
/// in-memory ring buffer instead of writing to a file or stdout.
struct RingBufferLayer;

impl<S: Subscriber> Layer<S> for RingBufferLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let mut visitor = MessageVisitor::default();
        event.record(&mut visitor);
        let level = *event.metadata().level();
        let target = event.metadata().target();
        push_line(format!(
            "[{}] {} {}",
            level_label(level),
            target,
            visitor.message
        ));
    }
}

fn level_label(level: Level) -> &'static str {
    match level {
        Level::ERROR => "ERROR",
        Level::WARN => "WARN ",
        Level::INFO => "INFO ",
        Level::DEBUG => "DEBUG",
        Level::TRACE => "TRACE",
    }
}

#[derive(Default)]
struct MessageVisitor {
    message: String,
}

impl Visit for MessageVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        if field.name() == "message" {
            self.message = format!("{value:?}");
        }
    }
}

/// Initialize the global `tracing` subscriber to feed the in-memory ring
/// buffer consumed by the log panel widget.
pub fn init_tracing() {
    use tracing_subscriber::prelude::*;

    let registry = tracing_subscriber::registry().with(RingBufferLayer);
    // Ignore error if a global subscriber is already set (e.g. in tests).
    let _ = registry.try_init();
}
