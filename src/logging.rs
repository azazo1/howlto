use std::{
    collections::HashSet,
    io,
    path::{Path, PathBuf},
};
use time::{Date, Duration, Month, OffsetDateTime};
use tracing::{info, warn, Metadata};
use tracing::level_filters::LevelFilter;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_indicatif::IndicatifLayer;
use tracing_subscriber::filter::filter_fn;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer};

use tokio::fs;

const LOG_FILE_PREFIX: &str = "howlto";
const LOG_FILE_SUFFIX: &str = "log";
const LOG_RETENTION_DAYS: i64 = 3;
const LOG_MAX_FILES: usize = 3;
const LOG_MAX_BYTES: u64 = 10 * 1024 * 1024;

#[derive(Debug, Default)]
struct CleanupReport {
    deleted_files: usize,
    deleted_bytes: u64,
    failed_deletions: usize,
    skipped_files: usize,
    total_bytes: u64,
}

#[derive(Debug)]
struct LogFile {
    date: Date,
    path: PathBuf,
    size: u64,
    protected: bool,
}

fn file_filter(metadata: &Metadata) -> bool {
    !is_rig_metadata(metadata)
}

fn stderr_filter(metadata: &Metadata) -> bool {
    // 忽略 rig_core 的 tracing, 因为它每次调用 api 都会输出 INFO, 不符合使用常理.
    !is_rig_metadata(metadata)
}

fn is_rig_metadata(metadata: &Metadata) -> bool {
    let is_rig_module = metadata
        .module_path()
        .is_some_and(|module| module.starts_with("rig_core::"));
    let target = metadata.target();
    is_rig_module || target.starts_with("rig_core::")
}

fn current_log_date() -> Date {
    OffsetDateTime::now_utc().date()
}

fn log_file_name(date: Date) -> String {
    format!("{LOG_FILE_PREFIX}.{date}.{LOG_FILE_SUFFIX}")
}

fn parse_log_date(file_name: &str) -> Option<Date> {
    let date = file_name
        .strip_prefix(&format!("{LOG_FILE_PREFIX}."))?
        .strip_suffix(&format!(".{LOG_FILE_SUFFIX}"))?;
    let bytes = date.as_bytes();
    if bytes.len() != 10
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || !bytes[..4].iter().all(u8::is_ascii_digit)
        || !bytes[5..7].iter().all(u8::is_ascii_digit)
        || !bytes[8..].iter().all(u8::is_ascii_digit)
    {
        return None;
    }

    let year = date[0..4].parse().ok()?;
    let month = Month::try_from(date[5..7].parse::<u8>().ok()?).ok()?;
    let day = date[8..].parse().ok()?;
    Date::from_calendar_date(year, month, day).ok()
}

async fn remove_log_file(file: &LogFile, report: &mut CleanupReport) -> bool {
    match fs::remove_file(&file.path).await {
        Ok(()) => {
            report.deleted_files += 1;
            report.deleted_bytes = report.deleted_bytes.saturating_add(file.size);
            report.total_bytes = report.total_bytes.saturating_sub(file.size);
            true
        }
        Err(_) => {
            report.failed_deletions += 1;
            false
        }
    }
}

async fn cleanup_log_files(
    logs_dir: &Path,
    current_date: Date,
    protected_dates: &[Date],
) -> io::Result<CleanupReport> {
    let mut entries = fs::read_dir(logs_dir).await?;
    let mut files = Vec::new();
    let mut report = CleanupReport::default();
    let protected_names = protected_dates
        .iter()
        .map(|date| log_file_name(*date))
        .collect::<HashSet<_>>();

    while let Some(entry) = entries.next_entry().await? {
        let file_type = match entry.file_type().await {
            Ok(file_type) => file_type,
            Err(_) => {
                report.skipped_files += 1;
                continue;
            }
        };
        if !file_type.is_file() {
            continue;
        }

        let Some(file_name) = entry.file_name().to_str().map(str::to_owned) else {
            report.skipped_files += 1;
            continue;
        };
        let Some(date) = parse_log_date(&file_name) else {
            continue;
        };
        let metadata = match entry.metadata().await {
            Ok(metadata) => metadata,
            Err(_) => {
                report.skipped_files += 1;
                continue;
            }
        };
        let size = metadata.len();
        report.total_bytes = report.total_bytes.saturating_add(size);
        files.push(LogFile {
            date,
            path: entry.path(),
            size,
            protected: protected_names.contains(&file_name),
        });
    }

    files.sort_by_key(|file| file.date);
    let oldest_kept_date = current_date - Duration::days(LOG_RETENTION_DAYS - 1);
    let mut failed_paths = HashSet::new();
    let mut remaining_files = Vec::with_capacity(files.len());

    for file in files {
        if file.date < oldest_kept_date && !file.protected {
            if !remove_log_file(&file, &mut report).await {
                failed_paths.insert(file.path.clone());
                remaining_files.push(file);
            }
        } else {
            remaining_files.push(file);
        }
    }

    if report.total_bytes > LOG_MAX_BYTES {
        for file in &remaining_files {
            if report.total_bytes <= LOG_MAX_BYTES
                || file.protected
                || failed_paths.contains(&file.path)
            {
                continue;
            }
            remove_log_file(file, &mut report).await;
        }
    }

    Ok(report)
}

fn report_cleanup(report: &CleanupReport, logs_dir: &Path) {
    if report.deleted_files > 0 {
        info!(
            deleted_files = report.deleted_files,
            deleted_bytes = report.deleted_bytes,
            remaining_bytes = report.total_bytes,
            "Log cleanup completed."
        );
    }
    if report.failed_deletions > 0
        || report.skipped_files > 0
        || report.total_bytes > LOG_MAX_BYTES
    {
        warn!(
            path = %logs_dir.display(),
            failed_deletions = report.failed_deletions,
            skipped_files = report.skipped_files,
            remaining_bytes = report.total_bytes,
            max_bytes = LOG_MAX_BYTES,
            "Log cleanup did not fully enforce the retention budget."
        );
    }
}

async fn cleanup_log_files_best_effort(
    logs_dir: &Path,
    current_date: Date,
    protected_dates: &[Date],
) {
    match cleanup_log_files(logs_dir, current_date, protected_dates).await {
        Ok(report) => report_cleanup(&report, logs_dir),
        Err(error) => warn!(
            path = %logs_dir.display(),
            error = %error,
            "Failed to inspect log files for cleanup."
        ),
    }
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
    let before_date = current_log_date();
    let file_appender = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix(LOG_FILE_PREFIX)
        .filename_suffix(LOG_FILE_SUFFIX)
        .max_log_files(LOG_MAX_FILES)
        .build(&logs_dir)
        .map_err(io::Error::other)?;
    let after_date = current_log_date();
    let protected_dates = if before_date == after_date {
        vec![after_date]
    } else {
        vec![before_date, after_date]
    };
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

    cleanup_log_files_best_effort(&logs_dir, after_date, &protected_dates).await;

    Ok(guard)
}

#[cfg(test)]
mod tests {
    use std::{
        io::ErrorKind,
        path::{Path, PathBuf},
    };

    use time::{Date, Duration, Month};
    use tokio::fs;
    use uuid::Uuid;

    use super::{
        cleanup_log_files, cleanup_log_files_best_effort, log_file_name, parse_log_date,
        LOG_MAX_BYTES,
    };

    fn temp_log_dir() -> PathBuf {
        std::env::temp_dir().join(format!("howlto-log-test-{}", Uuid::new_v4()))
    }

    async fn write_sized_file(path: &Path, size: u64) {
        let file = fs::File::create(path).await.unwrap();
        file.set_len(size).await.unwrap();
    }

    fn test_date() -> Date {
        Date::from_calendar_date(2026, Month::August, 4).unwrap()
    }

    #[test]
    fn log_filename_parser_accepts_only_daily_logs() {
        let date = parse_log_date("howlto.2026-08-04.log").unwrap();

        assert_eq!(log_file_name(date), "howlto.2026-08-04.log");
        assert!(parse_log_date("howlto.log.2026-08-04").is_none());
        assert!(parse_log_date("howlto.2026-8-4.log").is_none());
        assert!(parse_log_date("howlto.2026-02-29.log").is_none());
        assert!(parse_log_date("howlto.2026-08-04.txt").is_none());
    }

    #[tokio::test]
    async fn cleanup_removes_expired_logs_and_keeps_unrelated_files() {
        let dir = temp_log_dir();
        fs::create_dir_all(&dir).await.unwrap();
        let current = test_date();
        let active_path = dir.join(log_file_name(current));
        let recent_path = dir.join(log_file_name(current - Duration::days(1)));
        let expired_path = dir.join(log_file_name(current - Duration::days(3)));
        let old_name_path = dir.join("howlto.log.2026-08-04");
        let unrelated_path = dir.join("keep.txt");

        write_sized_file(&active_path, 1).await;
        write_sized_file(&recent_path, 1).await;
        write_sized_file(&expired_path, 1).await;
        fs::write(&old_name_path, b"old naming").await.unwrap();
        fs::write(&unrelated_path, b"keep").await.unwrap();

        let report = cleanup_log_files(&dir, current, &[current]).await.unwrap();

        assert_eq!(report.deleted_files, 1);
        assert!(fs::try_exists(&active_path).await.unwrap());
        assert!(fs::try_exists(&recent_path).await.unwrap());
        assert!(!fs::try_exists(&expired_path).await.unwrap());
        assert!(fs::try_exists(&old_name_path).await.unwrap());
        assert!(fs::try_exists(&unrelated_path).await.unwrap());

        fs::remove_dir_all(&dir).await.unwrap();
    }

    #[tokio::test]
    async fn cleanup_deletes_oldest_files_to_size_budget() {
        let dir = temp_log_dir();
        fs::create_dir_all(&dir).await.unwrap();
        let current = test_date();
        let active_path = dir.join(log_file_name(current));
        let recent_path = dir.join(log_file_name(current - Duration::days(1)));
        let oldest_path = dir.join(log_file_name(current - Duration::days(2)));
        let file_size = 4 * 1024 * 1024;

        write_sized_file(&active_path, file_size).await;
        write_sized_file(&recent_path, file_size).await;
        write_sized_file(&oldest_path, file_size).await;

        let report = cleanup_log_files(&dir, current, &[current]).await.unwrap();

        assert!(!fs::try_exists(&oldest_path).await.unwrap());
        assert!(fs::try_exists(&active_path).await.unwrap());
        assert!(fs::try_exists(&recent_path).await.unwrap());
        assert!(report.total_bytes <= LOG_MAX_BYTES);

        fs::remove_dir_all(&dir).await.unwrap();
    }

    #[tokio::test]
    async fn cleanup_keeps_active_file_when_it_exceeds_budget() {
        let dir = temp_log_dir();
        fs::create_dir_all(&dir).await.unwrap();
        let current = test_date();
        let active_path = dir.join(log_file_name(current));
        let recent_path = dir.join(log_file_name(current - Duration::days(1)));

        write_sized_file(&active_path, LOG_MAX_BYTES + 1).await;
        write_sized_file(&recent_path, 1).await;

        let report = cleanup_log_files(&dir, current, &[current]).await.unwrap();

        assert!(fs::try_exists(&active_path).await.unwrap());
        assert!(!fs::try_exists(&recent_path).await.unwrap());
        assert!(report.total_bytes > LOG_MAX_BYTES);

        fs::remove_dir_all(&dir).await.unwrap();
    }

    #[tokio::test]
    async fn cleanup_failure_does_not_propagate_to_startup() {
        let path = temp_log_dir();
        fs::write(&path, b"not a directory").await.unwrap();

        cleanup_log_files_best_effort(&path, test_date(), &[test_date()]).await;

        let error = cleanup_log_files(&path, test_date(), &[test_date()])
            .await
            .unwrap_err();
        assert!(matches!(error.kind(), ErrorKind::NotADirectory | ErrorKind::Other));

        fs::remove_file(&path).await.unwrap();
    }
}
