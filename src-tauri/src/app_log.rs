use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::OnceLock,
};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{
    filter::LevelFilter, filter::Targets, fmt, fmt::time::ChronoLocal, prelude::*,
};

const LOG_DIR_NAME: &str = "logs";
const LOG_FILE_NAME: &str = "assistant.log";
const MAX_LOG_FILE_BYTES: u64 = 500 * 1024;
const LOG_TIME_FORMAT: &str = "%Y-%m-%d %H:%M:%S";

static LOG_PATH: OnceLock<PathBuf> = OnceLock::new();
static LOG_GUARD: OnceLock<WorkerGuard> = OnceLock::new();

pub fn init(app_data_dir: PathBuf) -> PathBuf {
    let log_dir = app_data_dir.join(LOG_DIR_NAME);
    let log_path = log_dir.join(LOG_FILE_NAME);
    let _ = LOG_PATH.set(log_path.clone());

    let file_appender = SizeLimitedLogWriter::new(log_path.clone())
        .unwrap_or_else(|error| panic!("日志文件初始化失败：{error}"));
    let (file_writer, guard) = tracing_appender::non_blocking(file_appender);
    let _ = LOG_GUARD.set(guard);

    let stdout_layer = fmt::layer()
        .with_timer(local_log_timer())
        .with_writer(std::io::stdout)
        .with_ansi(false)
        .with_target(false)
        .with_thread_ids(true)
        .with_level(true);
    let file_layer = fmt::layer()
        .with_timer(local_log_timer())
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

fn local_log_timer() -> ChronoLocal {
    ChronoLocal::new(LOG_TIME_FORMAT.to_string())
}

struct SizeLimitedLogWriter {
    path: PathBuf,
    file: File,
    current_size: u64,
}

impl SizeLimitedLogWriter {
    fn new(path: PathBuf) -> io::Result<Self> {
        let mut file = open_log_file(&path)?;
        let mut current_size = file.metadata()?.len();
        if current_size > MAX_LOG_FILE_BYTES {
            current_size = roll_log_file(&mut file, &[])? as u64;
        }

        Ok(Self {
            path,
            file,
            current_size,
        })
    }

    fn reopen(&mut self) -> io::Result<()> {
        self.file = open_log_file(&self.path)?;
        self.current_size = self.file.metadata()?.len();
        Ok(())
    }
}

impl Write for SizeLimitedLogWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.current_size + buf.len() as u64 > MAX_LOG_FILE_BYTES {
            self.current_size = roll_log_file(&mut self.file, buf).or_else(|_| {
                self.reopen()?;
                roll_log_file(&mut self.file, buf)
            })? as u64;
            return Ok(buf.len());
        }

        if let Err(error) = self.file.write_all(buf) {
            self.reopen()?;
            self.file.write_all(buf).map_err(|_| error)?;
        }
        self.current_size += buf.len() as u64;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

fn open_log_file(path: &Path) -> io::Result<File> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(path)?;
    file.seek(SeekFrom::End(0))?;
    Ok(file)
}

fn roll_log_file(file: &mut File, incoming: &[u8]) -> io::Result<usize> {
    let max_len = MAX_LOG_FILE_BYTES as usize;

    if incoming.len() >= max_len {
        let window = &incoming[incoming.len() - max_len..];
        let start = find_next_line_start(window);
        let bytes = &window[start..];
        rewrite_log_file(file, bytes)?;
        return Ok(bytes.len());
    }

    let keep_len = max_len - incoming.len();
    let current_len = file.metadata()?.len() as usize;
    let read_len = current_len.min(keep_len);
    let mut bytes = vec![0; read_len];

    if read_len > 0 {
        file.seek(SeekFrom::End(-(read_len as i64)))?;
        file.read_exact(&mut bytes)?;
        if current_len > keep_len {
            let start = find_next_line_start(&bytes);
            bytes.drain(0..start);
        }
    }

    bytes.extend_from_slice(incoming);
    if bytes.len() > max_len {
        let overflow = bytes.len() - max_len;
        let start = overflow + find_next_line_start(&bytes[overflow..]);
        bytes.drain(0..start);
    }

    rewrite_log_file(file, &bytes)?;
    Ok(bytes.len())
}

fn find_next_line_start(bytes: &[u8]) -> usize {
    match bytes.iter().position(|byte| *byte == b'\n') {
        Some(index) if index + 1 < bytes.len() => index + 1,
        _ => 0,
    }
}

fn rewrite_log_file(file: &mut File, bytes: &[u8]) -> io::Result<()> {
    file.set_len(0)?;
    file.seek(SeekFrom::Start(0))?;
    file.write_all(bytes)?;
    file.flush()?;
    file.seek(SeekFrom::End(0))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn writer_keeps_recent_logs_when_size_limit_is_reached() {
        let path = test_log_path("rolling");
        let mut writer = SizeLimitedLogWriter::new(path.clone()).unwrap();
        let old_line = format!("{}\n", "old-log-line".repeat(256));
        let recent_line = vec![b'b'; 200 * 1024];

        for _ in 0..140 {
            writer.write_all(old_line.as_bytes()).unwrap();
        }
        writer.write_all(&recent_line).unwrap();
        writer.flush().unwrap();
        drop(writer);

        let bytes = fs::read(&path).unwrap();
        assert!(bytes.len() <= MAX_LOG_FILE_BYTES as usize);
        assert!(bytes.ends_with(&recent_line));
        assert!(bytes.len() < old_line.len() * 140 + recent_line.len());

        let _ = fs::remove_file(path);
    }

    fn test_log_path(name: &str) -> PathBuf {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis();
        std::env::temp_dir().join(format!("df-assistant-{name}-{millis}.log"))
    }
}
