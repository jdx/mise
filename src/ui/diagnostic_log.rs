use std::fs::{File, OpenOptions, create_dir_all};
use std::io::Write;
use std::sync::{Mutex, OnceLock};

use crate::env;

static WRITER: OnceLock<Mutex<File>> = OnceLock::new();

pub fn init() {
    if let Some(path) = &*env::MISE_DIAGNOSTIC_LOG {
        if let Some(parent) = path.parent() {
            let _ = create_dir_all(parent);
        }
        match OpenOptions::new().create(true).append(true).open(path) {
            Ok(file) => {
                let _ = WRITER.set(Mutex::new(file));
            }
            Err(e) => {
                eprintln!("mise: could not open diagnostic log file {path:?}: {e}");
            }
        }
    }
}

pub fn is_enabled() -> bool {
    WRITER.get().is_some()
}

fn write_line(json: &str) {
    if let Some(writer) = WRITER.get()
        && let Ok(mut f) = writer.lock()
    {
        let _ = writeln!(f, "{}", json);
        let _ = f.flush();
    }
}

fn timestamp() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

pub fn session_start(total_tools: usize, reason: &str) {
    if !is_enabled() {
        return;
    }
    let ts = timestamp();
    let reason = serde_json::to_string(reason).unwrap_or_default();
    write_line(&format!(
        r#"{{"ts":"{ts}","event":"session_start","total_tools":{total_tools},"reason":{reason}}}"#
    ));
}

pub fn session_end() {
    if !is_enabled() {
        return;
    }
    let ts = timestamp();
    write_line(&format!(r#"{{"ts":"{ts}","event":"session_end"}}"#));
}

pub fn tool_start(tool: &str) {
    if !is_enabled() {
        return;
    }
    let ts = timestamp();
    let tool = serde_json::to_string(tool).unwrap_or_default();
    write_line(&format!(
        r#"{{"ts":"{ts}","event":"tool_start","tool":{tool}}}"#
    ));
}

pub fn message(tool: &str, message: &str) {
    if !is_enabled() {
        return;
    }
    let ts = timestamp();
    let tool = serde_json::to_string(tool).unwrap_or_default();
    let message = serde_json::to_string(message).unwrap_or_default();
    write_line(&format!(
        r#"{{"ts":"{ts}","event":"message","tool":{tool},"message":{message}}}"#
    ));
}

pub fn progress(tool: &str, bytes_current: u64, bytes_total: u64) {
    if !is_enabled() {
        return;
    }
    let ts = timestamp();
    let tool = serde_json::to_string(tool).unwrap_or_default();
    let progress_pct = if bytes_total > 0 {
        (bytes_current as f64 / bytes_total as f64 * 100.0).clamp(0.0, 100.0)
    } else {
        0.0
    };
    write_line(&format!(
        r#"{{"ts":"{ts}","event":"progress","tool":{tool},"bytes_current":{bytes_current},"bytes_total":{bytes_total},"progress_pct":{progress_pct:.1}}}"#
    ));
}

pub fn operation(tool: &str, operation_num: u32, length: u64) {
    if !is_enabled() {
        return;
    }
    let ts = timestamp();
    let tool = serde_json::to_string(tool).unwrap_or_default();
    write_line(&format!(
        r#"{{"ts":"{ts}","event":"operation","tool":{tool},"operation_num":{operation_num},"length":{length}}}"#
    ));
}

pub fn tool_complete(tool: &str, status: &str) {
    if !is_enabled() {
        return;
    }
    let ts = timestamp();
    let tool = serde_json::to_string(tool).unwrap_or_default();
    let status = serde_json::to_string(status).unwrap_or_default();
    write_line(&format!(
        r#"{{"ts":"{ts}","event":"tool_complete","tool":{tool},"status":{status}}}"#
    ));
}

pub fn log_message(level: &str, message: &str) {
    if !is_enabled() {
        return;
    }
    let ts = timestamp();
    let level = serde_json::to_string(level).unwrap_or_default();
    let message = serde_json::to_string(message).unwrap_or_default();
    write_line(&format!(
        r#"{{"ts":"{ts}","event":"log_message","level":{level},"message":{message}}}"#
    ));
}
