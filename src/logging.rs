use std::{
    collections::VecDeque,
    fmt, io,
    path::Path,
    sync::atomic::{AtomicUsize, Ordering},
    sync::{Mutex, OnceLock},
};
use tracing::level_filters::LevelFilter;
use tracing::{
    Event, Level, Metadata, Subscriber,
    field::{Field, Visit},
};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_indicatif::IndicatifLayer;
use tracing_subscriber::filter::filter_fn;
use tracing_subscriber::fmt::format::Writer;
use tracing_subscriber::fmt::time::{FormatTime, SystemTime};
use tracing_subscriber::layer::Context;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer};

use tokio::fs;

static ACTIVE_TUI_COUNT: AtomicUsize = AtomicUsize::new(0);
static LOG_BUFFER: OnceLock<Mutex<VecDeque<UiLogEntry>>> = OnceLock::new();
const MAX_LOG_LINES: usize = 256;

#[derive(Debug, Clone)]
pub struct UiLogEntry {
    pub timestamp: String,
    pub level: Level,
    pub target: String,
    pub message: String,
    pub fields: Vec<(String, String)>,
}

#[derive(Debug, Clone, Default)]
struct UiLogLayer;

#[derive(Debug, Default)]
struct EventFieldVisitor {
    message: Option<String>,
    fields: Vec<(String, String)>,
}

fn log_buffer() -> &'static Mutex<VecDeque<UiLogEntry>> {
    LOG_BUFFER.get_or_init(|| Mutex::new(VecDeque::with_capacity(MAX_LOG_LINES)))
}

fn trim_debug_quotes(value: String) -> String {
    if value.starts_with('"') && value.ends_with('"') && value.len() >= 2 {
        value[1..value.len() - 1].to_string()
    } else {
        value
    }
}

fn push_log_entry(entry: UiLogEntry) {
    let mut guard = log_buffer()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if guard.len() >= MAX_LOG_LINES {
        guard.pop_front();
    }
    guard.push_back(entry);
}

fn current_timestamp() -> String {
    let mut output = String::new();
    let mut writer = Writer::new(&mut output);
    let _ = SystemTime.format_time(&mut writer);
    output
}

impl EventFieldVisitor {
    fn record_value(&mut self, field: &Field, value: String) {
        let value = trim_debug_quotes(value);
        if field.name() == "message" {
            self.message = Some(value);
        } else {
            self.fields.push((field.name().to_string(), value));
        }
    }
}

impl Visit for EventFieldVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.record_value(field, format!("{value:?}"));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.record_value(field, value.to_string());
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.record_value(field, value.to_string());
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.record_value(field, value.to_string());
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.record_value(field, value.to_string());
    }

    fn record_f64(&mut self, field: &Field, value: f64) {
        self.record_value(field, value.to_string());
    }
}

impl<S> Layer<S> for UiLogLayer
where
    S: Subscriber,
{
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let metadata = event.metadata();
        let mut visitor = EventFieldVisitor::default();
        event.record(&mut visitor);
        push_log_entry(UiLogEntry {
            timestamp: current_timestamp(),
            level: *metadata.level(),
            target: metadata.target().to_string(),
            message: visitor.message.unwrap_or_default(),
            fields: visitor.fields,
        });
    }
}

fn file_filter(metadata: &Metadata) -> bool {
    if is_rig_metadata(metadata) {
        return false;
    }
    true
}

fn stderr_filter(metadata: &Metadata) -> bool {
    if tui_active() {
        return false;
    }
    if is_rig_metadata(metadata) {
        return false;
    }
    true
}

fn buffer_filter(metadata: &Metadata) -> bool {
    if is_rig_metadata(metadata) {
        return false;
    }
    true
}

fn is_rig_metadata(metadata: &Metadata) -> bool {
    let target = metadata.target();
    if target.starts_with("rig::") || target.starts_with("rig_core::") {
        return true;
    }
    metadata
        .module_path()
        .is_some_and(|module| module.starts_with("rig::") || module.starts_with("rig_core::"))
}

pub fn enter_tui() {
    ACTIVE_TUI_COUNT.fetch_add(1, Ordering::SeqCst);
}

pub fn exit_tui() {
    let current = ACTIVE_TUI_COUNT.load(Ordering::SeqCst);
    if current > 0 {
        ACTIVE_TUI_COUNT.fetch_sub(1, Ordering::SeqCst);
    }
}

pub fn tui_active() -> bool {
    ACTIVE_TUI_COUNT.load(Ordering::SeqCst) > 0
}

pub fn recent_logs(limit: usize) -> Vec<UiLogEntry> {
    let guard = log_buffer()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let take = limit.min(guard.len());
    guard
        .iter()
        .skip(guard.len().saturating_sub(take))
        .cloned()
        .collect()
}

/// 初始化日志输出
/// fixme: 在 windows 某些旧版的 terminal 中颜色代码一开始是乱码.
///
/// - `stderr`: 是否在 stderr 中输出, 如果为 false, 那么只在文件中输出.
pub async fn init(
    config_dir: impl AsRef<Path>,
    stderr: bool,
    debug: bool,
) -> Result<WorkerGuard, io::Error> {
    let logs_dir = config_dir.as_ref().join("logs");
    if !logs_dir.is_dir() {
        fs::create_dir(&logs_dir).await?;
    }
    let file_appender = RollingFileAppender::new(Rotation::DAILY, logs_dir, "howlto.log");
    let (logging_appender, guard) = tracing_appender::non_blocking(file_appender);
    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(logging_appender)
        .with_ansi(false)
        .with_filter(filter_fn(file_filter));
    let ui_level = if debug {
        LevelFilter::DEBUG
    } else {
        LevelFilter::INFO
    };
    let memory_layer = UiLogLayer
        .with_filter(filter_fn(buffer_filter))
        .with_filter(ui_level);
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let subs = tracing_subscriber::registry()
        .with(file_layer)
        .with(memory_layer)
        .with(env_filter);
    if stderr {
        let indicatif_layer = IndicatifLayer::new();
        let stderr_layer = tracing_subscriber::fmt::layer()
            .with_writer(indicatif_layer.get_stderr_writer())
            .with_filter(filter_fn(stderr_filter))
            .with_filter(ui_level);
        subs.with(stderr_layer)
            .with(
                indicatif_layer
                    .with_filter(filter_fn(stderr_filter))
                    .with_filter(ui_level),
            ) // 在进度条上不显示内容
            .init();
    } else {
        subs.init();
    }
    Ok(guard)
}
