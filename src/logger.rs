use crate::config::Settings;
use eyre::Result;
use std::fs::{create_dir_all, File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;
use std::thread;

use crate::{env, ui};
use log::{Level, LevelFilter, Metadata, Record};
use once_cell::sync::Lazy;

#[derive(Debug)]
struct Logger {
    level: LevelFilter,
    term_level: LevelFilter,
    file_level: LevelFilter,
    log_file: Option<Mutex<File>>,
}

impl log::Log for Logger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= self.level
    }

    fn log(&self, record: &Record) {
        if record.level() <= self.file_level {
            if let Some(log_file) = &self.log_file {
                let mut log_file = log_file.lock().unwrap();
                let out = self.render(record, self.file_level);
                let _ = writeln!(log_file, "{}", console::strip_ansi_codes(&out));
            }
        }
        if record.level() <= self.term_level {
            ui::multi_progress_report::MultiProgressReport::suspend_if_active(|| {
                eprintln!("{}", self.render(record, self.term_level));
            });
        }
    }

    fn flush(&self) {}
}

static LOGGER: Lazy<Logger> = Lazy::new(Logger::init);

impl Logger {
    fn init() -> Self {
        let settings = Settings::try_get().unwrap_or_else(|_| Default::default());

        let term_level = settings.log_level();
        let file_level = env::MISE_LOG_FILE_LEVEL.unwrap_or(settings.log_level());

        let mut logger = Logger {
            level: std::cmp::max(term_level, file_level),
            file_level,
            term_level,
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
        match level {
            LevelFilter::Off => "".to_string(),
            LevelFilter::Trace => {
                let meta = ui::style::edim(format!(
                    "{thread_id:>2} [{file}:{line}]",
                    thread_id = self.thread_id(),
                    file = record.file().unwrap_or("<unknown>"),
                    line = record.line().unwrap_or(0),
                ));
                format!(
                    "{level} {meta} {args}",
                    level = self.styled_level(record.level()),
                    args = record.args()
                )
            }
            LevelFilter::Debug => format!(
                "{level} {module_path} {args}",
                level = self.styled_level(record.level()),
                module_path = record.module_path().unwrap_or_default(),
                args = record.args()
            ),
            _ => {
                let mise = match record.level() {
                    Level::Error => ui::style::ered("mise"),
                    Level::Warn => ui::style::eyellow("mise"),
                    _ => ui::style::edim("mise"),
                };
                match record.level() {
                    Level::Info => format!("{mise} {args}", args = record.args()),
                    _ => format!(
                        "{mise} {level} {args}",
                        level = self.styled_level(record.level()),
                        args = record.args()
                    ),
                }
            }
        }
    }

    fn thread_id(&self) -> String {
        let id = format!("{:?}", thread::current().id());
        let id = id.replace("ThreadId(", "");
        id.replace(")", "")
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

pub fn init() {
    static INIT: std::sync::Once = std::sync::Once::new();
    INIT.call_once(|| {
        if let Err(err) = log::set_logger(&*LOGGER).map(|()| log::set_max_level(LOGGER.level)) {
            eprintln!("mise: could not initialize logger: {err}");
        }
    });
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
    use super::*;
    use crate::test::reset;

    #[test]
    fn test_init() {
        reset();
        init();
    }
}
