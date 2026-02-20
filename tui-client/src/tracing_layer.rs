use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;

use crate::log_widget::LOGS;

pub struct TuiLayer;

impl TuiLayer {
    pub fn new() -> Self {
        Self
    }
}

impl<S> Layer<S> for TuiLayer
where
    S: tracing::Subscriber,
{
    fn on_event(&self, event: &tracing::Event, _ctx: Context<'_, S>) {
        let level = event.metadata().level();
        let target = event.metadata().target();
        let file = event.metadata().file().unwrap_or("unknown");
        let line = event.metadata().line().unwrap_or(0);

        let mut message = String::new();
        let mut visitor = MessageVisitor {
            message: &mut message,
        };
        event.record(&mut visitor);

        let log_entry = if message.is_empty() {
            format!("[{}] {} ({}:{})", level, target, file, line)
        } else {
            format!("[{}] {}: {} ({}:{})", level, target, message, file, line)
        };

        if let Ok(mut logs) = LOGS.lock() {
            if logs.len() >= 1000 {
                logs.pop_front();
            }
            logs.push_back(log_entry);
        }
    }
}

struct MessageVisitor<'a> {
    message: &'a mut String,
}

impl<'a> tracing::field::Visit for MessageVisitor<'a> {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.message.push_str(value);
        }
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" || field.name() == "value" {
            use std::fmt::Write;
            let _ = write!(self.message, "{:?}", value);
        }
    }
}
