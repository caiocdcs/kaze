//! The last log lines, kept in memory for Settings → System → Logs.
//!
//! A `tracing` layer beside the stderr one: it sees what the env filter
//! lets through, so the UI shows what the terminal shows. Nothing goes to
//! disk; Docker or journald keep the full log.

use std::{
    collections::VecDeque,
    fmt::Write as _,
    sync::{Arc, Mutex, PoisonError},
};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::{
    Event, Subscriber,
    field::{Field, Visit},
    span,
};
use tracing_subscriber::{Layer, layer::Context, registry::LookupSpan};
use ts_rs::TS;
use utoipa::ToSchema;

/// Most severe first, so `level <= wanted` keeps the more severe ones.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS, ToSchema,
)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl From<tracing::Level> for LogLevel {
    fn from(level: tracing::Level) -> Self {
        match level {
            tracing::Level::ERROR => Self::Error,
            tracing::Level::WARN => Self::Warn,
            tracing::Level::INFO => Self::Info,
            tracing::Level::DEBUG => Self::Debug,
            tracing::Level::TRACE => Self::Trace,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS, ToSchema)]
#[ts(export)]
pub struct LogLine {
    pub time: DateTime<Utc>,
    pub level: LogLevel,
    pub target: String,
    /// The message, then its fields as `key=value`.
    pub message: String,
    /// The spans it happened in, outermost first, e.g. `job{job_id=7}`.
    pub spans: String,
}

/// The shared ring; cheap to clone.
#[derive(Debug, Clone)]
pub struct RecentLogs {
    // A std mutex: held only to push or copy, never across an `.await`.
    lines: Arc<Mutex<VecDeque<LogLine>>>,
    capacity: usize,
}

impl RecentLogs {
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            lines: Arc::new(Mutex::new(VecDeque::with_capacity(capacity))),
            capacity,
        }
    }

    /// The layer that fills this ring.
    #[must_use]
    pub fn layer(&self) -> RecentLayer {
        RecentLayer { logs: self.clone() }
    }

    /// Lines at `level` or more severe, newest first.
    #[must_use]
    pub fn lines(&self, level: LogLevel) -> Vec<LogLine> {
        let lines = self.lines.lock().unwrap_or_else(PoisonError::into_inner);
        lines
            .iter()
            .rev()
            .filter(|l| l.level <= level)
            .cloned()
            .collect()
    }

    fn push(&self, line: LogLine) {
        let mut lines = self.lines.lock().unwrap_or_else(PoisonError::into_inner);
        if lines.len() == self.capacity {
            lines.pop_front();
        }
        lines.push_back(line);
    }
}

pub struct RecentLayer {
    logs: RecentLogs,
}

/// A span's fields as text, kept in its extensions.
struct SpanFields(String);

impl<S: Subscriber + for<'a> LookupSpan<'a>> Layer<S> for RecentLayer {
    fn on_new_span(&self, attrs: &span::Attributes<'_>, id: &span::Id, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(id) else { return };
        let mut fields = Fields::default();
        attrs.record(&mut fields);
        span.extensions_mut().insert(SpanFields(fields.rest));
    }

    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        let mut fields = Fields::default();
        event.record(&mut fields);
        let mut message = fields.message;
        if !fields.rest.is_empty() {
            if !message.is_empty() {
                message.push(' ');
            }
            message.push_str(&fields.rest);
        }
        let mut spans = String::new();
        for span in ctx
            .event_scope(event)
            .into_iter()
            .flat_map(tracing_subscriber::registry::Scope::from_root)
        {
            if !spans.is_empty() {
                spans.push(':');
            }
            spans.push_str(span.name());
            if let Some(SpanFields(f)) = span.extensions().get::<SpanFields>()
                && !f.is_empty()
            {
                let _ = write!(spans, "{{{f}}}");
            }
        }
        let meta = event.metadata();
        self.logs.push(LogLine {
            time: Utc::now(),
            level: (*meta.level()).into(),
            target: meta.target().to_owned(),
            message,
            spans,
        });
    }
}

#[derive(Default)]
struct Fields {
    message: String,
    rest: String,
}

impl Visit for Fields {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message.push_str(value);
        } else {
            self.add(field, &value);
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            let _ = write!(self.message, "{value:?}");
        } else {
            self.add(field, value);
        }
    }
}

impl Fields {
    fn add(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if !self.rest.is_empty() {
            self.rest.push(' ');
        }
        let _ = write!(self.rest, "{}={value:?}", field.name());
    }
}

#[cfg(test)]
mod tests {
    use tracing_subscriber::layer::SubscriberExt as _;

    use super::*;

    #[test]
    fn keeps_the_newest_lines_with_their_spans() {
        let logs = RecentLogs::new(3);
        let subscriber = tracing_subscriber::registry().with(logs.layer());
        tracing::subscriber::with_default(subscriber, || {
            tracing::info!("first");
            let span = tracing::info_span!("job", job_id = 7);
            let _in = span.enter();
            tracing::warn!(series = "Berserk", count = 2, "found chapters");
            tracing::debug!("detail");
            tracing::error!("broke");
        });

        let all = logs.lines(LogLevel::Trace);
        let messages: Vec<&str> = all.iter().map(|l| l.message.as_str()).collect();
        assert_eq!(
            messages,
            [
                "broke",
                "detail",
                "found chapters series=\"Berserk\" count=2"
            ],
            "newest first, oldest dropped"
        );
        assert_eq!(all[0].spans, "job{job_id=7}");
        let warnings: Vec<_> = logs
            .lines(LogLevel::Warn)
            .into_iter()
            .map(|l| l.level)
            .collect();
        assert_eq!(warnings, [LogLevel::Error, LogLevel::Warn]);
    }
}
