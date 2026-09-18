//! One log file per install, appended by both the app and the CLI, plus an optional live tap
//! for the app's log panel. Panics are logged too, so "nothing happened" always has a trace.
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

/// Receives every log line as it is written (the app's live log panel).
pub type Tap = Box<dyn Fn(&str) + Send + Sync>;

struct FileLog {
    file: Mutex<File>,
    tap: Option<Tap>,
}

impl log::Log for FileLog {
    fn enabled(&self, m: &log::Metadata) -> bool {
        m.level() <= log::Level::Info
    }
    fn log(&self, r: &log::Record) {
        if !self.enabled(r.metadata()) || !r.target().starts_with("forge") {
            return;
        }
        let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let line = format!("{} {:<5} {}", stamp(secs), r.level(), r.args());
        if let Ok(mut f) = self.file.lock() {
            let _ = writeln!(f, "{line}");
        }
        if let Some(t) = &self.tap {
            t(&line);
        }
    }
    fn flush(&self) {}
}

// ponytail: UTC hh:mm:ss + date, no chrono dependency for a log stamp.
fn stamp(secs: u64) -> String {
    let days = secs / 86_400;
    let (h, m, s) = ((secs / 3600) % 24, (secs / 60) % 60, secs % 60);
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mo <= 2 { y + 1 } else { y };
    format!("{y:04}-{mo:02}-{d:02} {h:02}:{m:02}:{s:02}Z")
}

pub fn default_path() -> PathBuf {
    dirs::data_local_dir().unwrap_or_else(|| PathBuf::from(".")).join("husky-forge").join("logs").join("husky-forge.log")
}

/// Install the logger (idempotent). `tap` receives every line as it is written.
pub fn init(tap: Option<Tap>) -> PathBuf {
    let path = default_path();
    if let Some(p) = path.parent() {
        let _ = std::fs::create_dir_all(p);
    }
    // Rotate once past 5 MB: keep one previous log.
    if std::fs::metadata(&path).map(|m| m.len() > 5 * 1024 * 1024).unwrap_or(false) {
        let _ = std::fs::rename(&path, path.with_extension("1.log"));
    }
    if let Ok(file) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = log::set_boxed_logger(Box::new(FileLog { file: Mutex::new(file), tap }));
        log::set_max_level(log::LevelFilter::Info);
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            log::error!("panic: {info}");
            prev(info);
        }));
    }
    path
}

#[cfg(test)]
mod tests {
    #[test]
    fn stamp_is_civil_utc() {
        assert_eq!(super::stamp(0), "1970-01-01 00:00:00Z");
        assert_eq!(super::stamp(1_789_734_084), "2026-09-18 12:21:24Z");
    }
}
