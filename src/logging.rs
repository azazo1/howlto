use std::{io, path::Path};
use tracing::level_filters::LevelFilter;
use tracing::{Level, Metadata};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_indicatif::IndicatifLayer;
use tracing_subscriber::filter::filter_fn;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer};

use tokio::fs;

fn file_filter(metadata: &Metadata) -> bool {
    if let Some(module) = metadata.module_path()
        && module.starts_with("rig::")
    {
        return false;
    }
    true
}

fn stderr_filter(metadata: &Metadata) -> bool {
    // 忽略 rig 的 tracing, 因为它每次调用 api 都会输出 INFO, 不符合使用常理.
    if let Some(module) = metadata.module_path()
        && module.starts_with("rig::")
    {
        return false;
    }
    true
}

/// 初始化日志输出
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
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("debug"));
    let subs = tracing_subscriber::registry()
        .with(file_layer)
        .with(env_filter);
    if stderr {
        let stderr_level = if debug {
            LevelFilter::DEBUG
        } else {
            LevelFilter::INFO
        };
        let indicatif_layer = IndicatifLayer::new();
        let stderr_layer = tracing_subscriber::fmt::layer()
            .with_target(false)
            .without_time()
            .with_writer(indicatif_layer.get_stderr_writer())
            .with_filter(filter_fn(stderr_filter))
            .with_filter(stderr_level);
        subs.with(stderr_layer)
            .with(
                indicatif_layer
                    .with_filter(filter_fn(stderr_filter))
                    .with_filter(stderr_level),
            ) // 在进度条上不显示内容
            .init();
    } else {
        subs.init();
    }
    Ok(guard)
}
