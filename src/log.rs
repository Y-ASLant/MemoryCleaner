use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static DEBUG_ENABLED: AtomicBool = AtomicBool::new(false);
static LOG_FILE_ERROR_REPORTED: AtomicBool = AtomicBool::new(false);
static LOG_STATE: Mutex<LogState> = Mutex::new(LogState { last_purge: 0 });

/// Drop log lines whose `[unix_secs.millis]` timestamp is older than this.
pub const LOG_RETENTION_SECS: u64 = 7 * 24 * 60 * 60;
const LOG_PURGE_INTERVAL_SECS: u64 = 60 * 60;
const MAX_UNTIMESTAMPED_LOG_LINES: usize = 256;
const MAX_UNTIMESTAMPED_LOG_BYTES: usize = 64 * 1024;

struct LogState {
    last_purge: u64,
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn OutputDebugStringA(lp_output_string: *const u8);
}

fn write_debug_output(msg: &str) {
    let bytes = format!("{msg}\n\0").into_bytes();
    unsafe {
        OutputDebugStringA(bytes.as_ptr());
    }
    eprintln!("{msg}");
}

/// Write a diagnostic message to the Windows debug stream (viewable via DebugView)
/// and, when stderr is attached, also to stderr.
pub fn log_msg(msg: &str) {
    write_debug_output(msg);
    write(msg);
}

pub fn set_debug_enabled(enabled: bool) {
    DEBUG_ENABLED.store(enabled, Ordering::Relaxed);
    if enabled {
        if let Ok(mut state) = LOG_STATE.lock() {
            state.last_purge = 0;
        }
        LOG_FILE_ERROR_REPORTED.store(false, Ordering::Relaxed);
    }
}

pub fn log_file_path() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("App.log")))
        .unwrap_or_else(|| PathBuf::from("App.log"))
}

pub(crate) fn parse_line_timestamp(line: &str) -> Option<u64> {
    let inner = line.strip_prefix('[')?.split(']').next()?;
    inner.split('.').next()?.parse().ok()
}

fn retained_log_content(content: &str, now: u64) -> Option<String> {
    let lines: Vec<&str> = content.split_inclusive('\n').collect();
    let mut keep_untimestamped = vec![false; lines.len()];
    let mut untimestamped_lines = 0usize;
    let mut untimestamped_bytes = 0usize;

    for (index, line) in lines.iter().enumerate().rev() {
        if line.trim().is_empty() || parse_line_timestamp(line).is_some() {
            continue;
        }
        let next_bytes = untimestamped_bytes.saturating_add(line.len());
        if untimestamped_lines < MAX_UNTIMESTAMPED_LOG_LINES
            && next_bytes <= MAX_UNTIMESTAMPED_LOG_BYTES
        {
            keep_untimestamped[index] = true;
            untimestamped_lines += 1;
            untimestamped_bytes = next_bytes;
        }
    }

    let cutoff = now.saturating_sub(LOG_RETENTION_SECS);
    let mut kept = String::with_capacity(content.len());
    let mut changed = false;
    for (index, line) in lines.into_iter().enumerate() {
        if line.trim().is_empty() {
            changed = true;
            continue;
        }
        match parse_line_timestamp(line) {
            Some(timestamp) if timestamp < cutoff => changed = true,
            Some(_) => kept.push_str(line),
            None if keep_untimestamped[index] => kept.push_str(line),
            None => changed = true,
        }
    }

    changed.then_some(kept)
}

fn purge_stale_entries(path: &Path, now: u64) -> io::Result<()> {
    let content = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    let Some(kept) = retained_log_content(&content, now) else {
        return Ok(());
    };

    if kept.is_empty() {
        return match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        };
    }

    let mut file = OpenOptions::new().write(true).truncate(true).open(path)?;
    file.write_all(kept.as_bytes())
}

fn timestamp() -> (String, u64) {
    let Ok(duration) = SystemTime::now().duration_since(UNIX_EPOCH) else {
        return ("unknown".into(), 0);
    };
    let secs = duration.as_secs();
    (format!("{secs}.{:03}", duration.subsec_millis()), secs)
}

fn purge_due(last_purge: u64, now: u64) -> bool {
    last_purge == 0 || now < last_purge || now.saturating_sub(last_purge) >= LOG_PURGE_INTERVAL_SECS
}

fn append_log_line(path: &Path, line: &[u8]) -> io::Result<()> {
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(line)
}

fn report_log_file_error(error: &io::Error) {
    if !LOG_FILE_ERROR_REPORTED.swap(true, Ordering::Relaxed) {
        write_debug_output(&format!("[log] file output failed: {error}"));
    }
}

/// Append a line to `App.log` when debug logging is enabled.
pub fn write(msg: &str) {
    if !DEBUG_ENABLED.load(Ordering::Relaxed) {
        return;
    }

    let Ok(mut state) = LOG_STATE.lock() else {
        write_debug_output("[log] file output lock poisoned");
        return;
    };

    let (timestamp, now) = timestamp();
    let path = log_file_path();
    let mut succeeded = true;
    if purge_due(state.last_purge, now) {
        state.last_purge = now;
        if let Err(error) = purge_stale_entries(&path, now) {
            report_log_file_error(&error);
            succeeded = false;
        }
    }

    let line = format!("[{timestamp}] {msg}\n");
    if let Err(error) = append_log_line(&path, line.as_bytes()) {
        report_log_file_error(&error);
        succeeded = false;
    }
    if succeeded {
        LOG_FILE_ERROR_REPORTED.store(false, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_line_timestamp_reads_unix_prefix() {
        assert_eq!(
            parse_line_timestamp("[1700000000.123] hello\n"),
            Some(1_700_000_000)
        );
        assert_eq!(parse_line_timestamp("no timestamp"), None);
    }

    #[test]
    fn retention_window_is_seven_days() {
        assert_eq!(LOG_RETENTION_SECS, 7 * 24 * 60 * 60);
    }

    #[test]
    fn purge_is_throttled_after_the_first_write() {
        assert!(purge_due(0, 100));
        assert!(!purge_due(100, 100 + LOG_PURGE_INTERVAL_SECS - 1));
        assert!(purge_due(100, 100 + LOG_PURGE_INTERVAL_SECS));
        assert!(purge_due(200, 100));
    }

    #[test]
    fn retention_removes_stale_timestamped_lines() {
        let now = LOG_RETENTION_SECS + 100;
        let content = format!("[1.000] stale\n[{now}.000] current\n");
        let retained = retained_log_content(&content, now).expect("content should change");
        assert_eq!(retained, format!("[{now}.000] current\n"));
    }

    #[test]
    fn retention_bounds_untimestamped_content() {
        let content: String = (0..300)
            .map(|index| format!("malformed-{index}\n"))
            .collect();
        let retained = retained_log_content(&content, 1).expect("content should be bounded");
        assert_eq!(retained.lines().count(), MAX_UNTIMESTAMPED_LOG_LINES);
        assert!(!retained.contains("malformed-0\n"));
        assert!(retained.contains("malformed-299\n"));
    }

    #[test]
    fn retention_drops_oversized_untimestamped_line() {
        let content = "x".repeat(MAX_UNTIMESTAMPED_LOG_BYTES + 1);
        let retained = retained_log_content(&content, 1).expect("content should be bounded");
        assert!(retained.is_empty());
    }

    #[test]
    fn append_returns_unwritable_destination_error() {
        let directory = tempfile::tempdir().expect("create temporary directory");
        assert!(append_log_line(directory.path(), b"log line\n").is_err());
    }
}
