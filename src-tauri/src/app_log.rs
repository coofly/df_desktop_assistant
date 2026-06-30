use std::{path::PathBuf, sync::OnceLock};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{filter::LevelFilter, filter::Targets, fmt, prelude::*};

const LOG_DIR_NAME: &str = "logs";
const LOG_FILE_NAME: &str = "assistant.log";

static LOG_PATH: OnceLock<PathBuf> = OnceLock::new();
static LOG_GUARD: OnceLock<WorkerGuard> = OnceLock::new();

pub fn init(app_data_dir: PathBuf) -> PathBuf {
    let log_dir = app_data_dir.join(LOG_DIR_NAME);
    let log_path = log_dir.join(LOG_FILE_NAME);
    let _ = LOG_PATH.set(log_path.clone());

    let file_appender = tracing_appender::rolling::never(log_dir, LOG_FILE_NAME);
    let (file_writer, guard) = tracing_appender::non_blocking(file_appender);
    let _ = LOG_GUARD.set(guard);

    let stdout_layer = fmt::layer()
        .with_writer(std::io::stdout)
        .with_ansi(false)
        .with_target(false)
        .with_thread_ids(true)
        .with_level(true);
    let file_layer = fmt::layer()
        .with_writer(file_writer)
        .with_ansi(false)
        .with_target(false)
        .with_thread_ids(true)
        .with_level(true);
    let filter = Targets::new()
        .with_default(LevelFilter::WARN)
        .with_target("df_desktop_assistant_lib", LevelFilter::INFO)
        .with_target("df_desktop_assistant", LevelFilter::INFO);

    if tracing_subscriber::registry()
        .with(filter)
        .with(stdout_layer)
        .with(file_layer)
        .try_init()
        .is_err()
    {
        eprintln!("日志系统初始化提示：tracing subscriber 已存在，将继续使用已有日志订阅器");
    }

    info(format!(
        "日志系统初始化完成，日志文件：{}",
        log_path.display()
    ));
    log_path
}

pub fn info(message: impl AsRef<str>) {
    tracing::info!("{}", message.as_ref());
}

pub fn warn(message: impl AsRef<str>) {
    tracing::warn!("{}", message.as_ref());
}

pub fn error(message: impl AsRef<str>) {
    tracing::error!("{}", message.as_ref());
}

pub fn debug(message: impl AsRef<str>) {
    tracing::debug!("{}", message.as_ref());
}

pub fn log_path() -> Option<PathBuf> {
    LOG_PATH.get().cloned()
}
