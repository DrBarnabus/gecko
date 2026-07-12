use std::{
    collections::VecDeque,
    fmt,
    sync::{Arc, Mutex},
};

use tracing::{Event, Level, Subscriber};
use tracing_subscriber::{
    EnvFilter, Layer,
    fmt::{
        FmtContext,
        format::{DefaultFields, FormatEvent, FormatFields, Writer},
    },
    layer::Context,
    prelude::*,
    registry::LookupSpan,
};

pub const DEFAULT_BUFFER_CAPACITY: usize = 10_000;
pub const DEFAULT_FILTER: &str = "debug,wgpu_core=info,wgpu_hal=info,naga=info,dear-imgui-wgpu=info";

pub fn init() -> Arc<LogBuffer> {
    let buffer = Arc::new(LogBuffer::new(DEFAULT_BUFFER_CAPACITY));

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(DEFAULT_FILTER));

    let registry = tracing_subscriber::registry()
        .with(BufferLayer { buffer: buffer.clone() })
        .with(filter)
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(std::io::stderr)
                .event_format(StripSelfTagFormat {
                    inner: tracing_subscriber::fmt::format::Format::default(),
                }),
        );

    #[cfg(feature = "tracy")]
    let registry = registry.with(tracing_tracy::TracyLayer::default());

    registry.init();

    buffer
}

pub struct LogEntry {
    pub seq: u64,
    pub level: Level,
    pub display: String,
    pub filter_key: String,
}

pub struct LogBufferInner {
    entries: VecDeque<LogEntry>,
    capacity: usize,
    next_seq: u64,
}

impl LogBufferInner {
    pub fn next_seq(&self) -> u64 {
        self.next_seq
    }

    pub fn head_seq(&self) -> u64 {
        self.next_seq - self.entries.len() as u64
    }

    pub fn get(&self, seq: u64) -> Option<&LogEntry> {
        let head_seq = self.head_seq();
        if seq < head_seq {
            return None;
        }

        self.entries.get((seq - head_seq) as usize)
    }

    pub fn entries_from(&self, from: u64) -> impl Iterator<Item = &LogEntry> {
        let skip = from.saturating_sub(self.head_seq()) as usize;
        self.entries.iter().skip(skip)
    }
}

pub struct LogBuffer {
    inner: Mutex<LogBufferInner>,
}

impl LogBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            inner: Mutex::new(LogBufferInner {
                entries: VecDeque::with_capacity(capacity),
                capacity,
                next_seq: 0,
            }),
        }
    }

    pub fn push(&self, level: Level, target: &str, span: Option<String>, message: &str) {
        let Ok(mut inner) = self.inner.lock() else { return };

        if inner.entries.len() >= inner.capacity {
            inner.entries.pop_front();
        }

        let seq = inner.next_seq;
        inner.next_seq += 1;

        let display = match &span {
            Some(span) => format!("[{level:5}] {target} [{span}]: {message}"),
            None => format!("[{level:5}] {target}: {message}"),
        };

        let mut filter_key = target.to_lowercase();
        if let Some(span) = &span {
            filter_key.push('\u{1}');
            filter_key.push_str(&span.to_lowercase());
        }
        filter_key.push('\u{1}');
        filter_key.push_str(&message.to_lowercase());

        inner.entries.push_back(LogEntry {
            seq,
            level,
            display,
            filter_key,
        })
    }

    pub fn read<R>(&self, f: impl FnOnce(&LogBufferInner) -> R) -> R {
        let inner = self.inner.lock().expect("log buffer poisoned");
        f(&inner)
    }
}

struct BufferLayer {
    buffer: Arc<LogBuffer>,
}

/// Some crates bake their own `[target][level]` tag into the message text, duplicating
/// what `target`/`level` already convey; both consumers of raw log text strip it if present.
fn self_tag(target: &str, level: Level) -> String {
    format!("[{target}][{}] ", level.to_string().to_lowercase())
}

impl<S> Layer<S> for BufferLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        let metadata = event.metadata();
        let target = metadata.target();
        let level = *metadata.level();

        let mut message = String::new();
        let _ = DefaultFields::new().format_fields(Writer::new(&mut message), event);
        let message = message
            .strip_prefix(self_tag(target, level).as_str())
            .unwrap_or(&message);

        let span = ctx
            .event_scope(event)
            .map(|scope| scope.from_root().map(|s| s.name()).collect::<Vec<_>>().join(":"))
            .filter(|s| !s.is_empty());

        self.buffer.push(level, target, span, message);
    }
}

struct StripSelfTagFormat<F> {
    inner: F,
}

impl<S, N, F> FormatEvent<S, N> for StripSelfTagFormat<F>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
    F: FormatEvent<S, N>,
{
    fn format_event(&self, ctx: &FmtContext<'_, S, N>, mut writer: Writer<'_>, event: &Event<'_>) -> fmt::Result {
        let mut rendered = String::new();
        self.inner.format_event(ctx, Writer::new(&mut rendered), event)?;

        let metadata = event.metadata();
        let tag = self_tag(metadata.target(), *metadata.level());
        if let Some(pos) = rendered.find(tag.as_str()) {
            rendered.replace_range(pos..pos + tag.len(), "");
        }

        writer.write_str(&rendered)
    }
}
