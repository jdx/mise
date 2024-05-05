extern crate simplelog;

use std::fs::{create_dir_all, File, OpenOptions};
use std::path::Path;

use eyre::Result;
use simplelog::*;

use crate::config::Settings;
use crate::env;

pub fn init() {
    static INIT: std::sync::Once = std::sync::Once::new();
    INIT.call_once(_init);
}

pub fn _init() {
    if cfg!(test) {
        return;
    }
    let settings = Settings::try_get().unwrap_or_else(|_| Default::default());
    let mut loggers: Vec<Box<dyn SharedLogger>> = vec![];
    let level = settings.log_level.parse().unwrap_or(LevelFilter::Info);
    loggers.push(init_term_logger(level));

    if let Some(log_file) = &*env::MISE_LOG_FILE {
        let file_level = env::MISE_LOG_FILE_LEVEL.unwrap_or(level);
        if let Some(logger) = init_write_logger(file_level, log_file) {
            loggers.push(logger)
        }
    }
    CombinedLogger::init(loggers).unwrap_or_else(|err| {
        eprintln!("mise: could not initialize logger: {err}");
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

fn init_term_logger(level: LevelFilter) -> Box<dyn SharedLogger> {
    let trace_level = if level >= LevelFilter::Trace {
        LevelFilter::Error
    } else {
        LevelFilter::Off
    };
    TermLogger::new(
        level,
        ConfigBuilder::new()
            .set_time_level(LevelFilter::Off)
            .set_thread_level(trace_level)
            .set_location_level(trace_level)
            .set_target_level(trace_level)
            .add_filter_ignore(String::from("globset")) // debug!() statements break outputs
            .build(),
        TerminalMode::Stderr,
        ColorChoice::Auto,
    )
}

fn init_write_logger(level: LevelFilter, log_path: &Path) -> Option<Box<dyn SharedLogger>> {
    match init_log_file(log_path) {
        Ok(log_file) => Some(WriteLogger::new(
            level,
            ConfigBuilder::new()
                .set_thread_level(LevelFilter::Trace)
                .build(),
            log_file,
        )),
        Err(err) => {
            eprintln!("mise: could not write to log file: {err}");

            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init() {
        init();
    }
}
