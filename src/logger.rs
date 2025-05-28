use crate::config::{Config, Settings};
use eyre::Result;
use std::fs::{File, OpenOptions, create_dir_all};
use std::path::Path;
use std::sync::Mutex;
use std::thread;
use std::{io::Write, sync::OnceLock};

use crate::{config, env, ui};
use log::{Level, LevelFilter, Metadata, Record};

#[derive(Debug)]
struct Logger {
    level: Mutex<LevelFilter>,
    term_level: Mutex<LevelFilter>,
    file_level: LevelFilter,
    log_file: Option<Mutex<File>>,
}

impl log::Log for Logger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= *self.level.lock().unwrap()
    }

    fn log(&self, record: &Record) {
        if record.level() <= self.file_level {
            if let Some(log_file) = &self.log_file {
                let mut log_file = log_file.lock().unwrap();
                let out = self.render(record, self.file_level);
                if !out.is_empty() {
                    let _ = writeln!(log_file, "{}", console::strip_ansi_codes(&out));
                }
            }
        }
        let term_level = *self.term_level.lock().unwrap();
        if record.level() <= term_level {
            ui::multi_progress_report::MultiProgressReport::suspend_if_active(|| {
                let out = self.render(record, term_level);
                if !out.is_empty() {
                    eprintln!("{}", self.render(record, term_level));
                }
            });
        }
    }

    fn flush(&self) {}
}

impl Logger {
    fn init(term_level: LevelFilter, file_level: LevelFilter) -> Self {
        let mut logger = Logger {
            level: Mutex::new(std::cmp::max(term_level, file_level)),
            file_level,
            term_level: Mutex::new(term_level),
            log_file: None,
        };

        if let Some(log_file) = &*env::MISE_LOG_FILE {
            if let Ok(log_file) = init_log_file(log_file) {
                logger.log_file = Some(Mutex::new(log_file));
            } else {
                eprintln!("mise: could not open log file: {log_file:?}");
            }
        }

        logger
    }

    fn render(&self, record: &Record, level: LevelFilter) -> String {
        let mut args = record.args().to_string();
        if config::is_loaded() {
            let config = Config::get_();
            args = config.redact(args);
        }
        match level {
            LevelFilter::Off => "".to_string(),
            LevelFilter::Trace => {
                let level = record.level();
                let file = record.file().unwrap_or("<unknown>");
                if level == LevelFilter::Trace && file.contains("/expr-lang-") {
                    return "".to_string();
                };
                let meta = ui::style::edim(format!(
                    "{thread_id:>2} [{file}:{line}]",
                    thread_id = thread_id(),
                    line = record.line().unwrap_or(0),
                ));
                format!("{level} {meta} {args}", level = self.styled_level(level),)
            }
            LevelFilter::Debug => {
                format!("{level} {args}", level = self.styled_level(record.level()),)
            }
            _ => {
                let mise = match record.level() {
                    Level::Error => ui::style::ered("mise"),
                    Level::Warn => ui::style::eyellow("mise"),
                    _ => ui::style::edim("mise"),
                };
                match record.level() {
                    Level::Info => format!("{mise} {args}"),
                    _ => format!(
                        "{mise} {level} {args}",
                        level = self.styled_level(record.level()),
                    ),
                }
            }
        }
    }

    fn styled_level(&self, level: Level) -> String {
        let level = match level {
            Level::Error => ui::style::ered("ERROR").to_string(),
            Level::Warn => ui::style::eyellow("WARN").to_string(),
            Level::Info => ui::style::ecyan("INFO").to_string(),
            Level::Debug => ui::style::emagenta("DEBUG").to_string(),
            Level::Trace => ui::style::edim("TRACE").to_string(),
        };
        console::pad_str(&level, 5, console::Alignment::Left, None).to_string()
    }
}

pub fn thread_id() -> String {
    let id = format!("{:?}", thread::current().id());
    let id = id.replace("ThreadId(", "");
    id.replace(")", "")
}

pub fn init() {
    static LOGGER: OnceLock<Logger> = OnceLock::new();
    let settings = Settings::try_get().unwrap_or_else(|_| Default::default());
    let term_level = settings.log_level();
    if let Some(logger) = LOGGER.get() {
        *logger.term_level.lock().unwrap() = term_level;
        *logger.level.lock().unwrap() = std::cmp::max(term_level, logger.file_level);
    } else {
        let file_level = env::MISE_LOG_FILE_LEVEL.unwrap_or(settings.log_level());
        let logger = LOGGER.get_or_init(|| Logger::init(term_level, file_level));
        if let Err(err) = log::set_logger(logger) {
            eprintln!("mise: could not initialize logger: {err}");
        }
    }
    log::set_max_level(term_level);
}

fn init_log_file(log_file: &Path) -> Result<File> {
    if let Some(log_dir) = log_file.parent() {
        create_dir_all(log_dir)?;
    }
    Ok(OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_file)?)
}

#[cfg(test)]
mod tests {
    use crate::config::Config;

    use super::*;

    #[tokio::test]
    async fn test_init() {
        let _config = Config::get().await.unwrap();
        init();
    }
}
