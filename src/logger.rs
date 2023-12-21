extern crate simplelog;

use std::fs::{create_dir_all, File, OpenOptions};
use std::path::PathBuf;

use eyre::Result;
use simplelog::*;
use tracing_log::LogTracer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::Registry;
use tracing_tree::HierarchicalLayer;

#[cfg(not(feature = "tracing"))]
pub fn init(log_level: LevelFilter, log_file_level: LevelFilter) {
    let mut loggers: Vec<Box<dyn SharedLogger>> = vec![];
    loggers.push(init_term_logger(log_level));

    if let Ok(log) = env::var("RTX_LOG_FILE") {
        let log_file = PathBuf::from(log);
        if let Some(logger) = init_write_logger(log_file_level, log_file) {
            loggers.push(logger)
        }
    }
    CombinedLogger::init(loggers).unwrap_or_else(|err| {
        eprintln!("rtx: could not initialize logger: {err}");
    });
}

#[cfg(feature = "tracing")]
pub fn init(_log_level: LevelFilter, _log_file_level: LevelFilter) {
    LogTracer::init().unwrap();

    let sub = Registry::default().with(
        HierarchicalLayer::default()
            .with_indent_lines(true)
            .with_targets(true)
            .with_thread_ids(true)
            .with_thread_names(true),
    );
    tracing::subscriber::set_global_default(sub).unwrap();
}

fn init_log_file(log_file: PathBuf) -> Result<File> {
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
            .build(),
        TerminalMode::Stderr,
        ColorChoice::Auto,
    )
}

fn init_write_logger(level: LevelFilter, log_path: PathBuf) -> Option<Box<dyn SharedLogger>> {
    match init_log_file(log_path) {
        Ok(log_file) => Some(WriteLogger::new(
            level,
            ConfigBuilder::new()
                .set_thread_level(LevelFilter::Trace)
                .build(),
            log_file,
        )),
        Err(err) => {
            eprintln!("rtx: could not write to log file: {err}");

            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init() {
        init(LevelFilter::Debug, LevelFilter::Debug);
    }
}
