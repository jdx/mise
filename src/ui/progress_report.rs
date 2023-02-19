use crate::ui::color::Color;
use atty::Stream;
use indicatif::ProgressBar;
use once_cell::sync::Lazy;
use std::time::Duration;

#[derive(Debug)]
pub struct ProgressReport {
    pub pb: Option<ProgressBar>,
    prefix: String,
}

impl ProgressReport {
    pub fn new(verbose: bool) -> ProgressReport {
        let pb = match verbose {
            true => None,
            false => Some(ProgressBar::new(0)),
        };
        ProgressReport {
            pb,
            prefix: String::new(),
        }
    }

    pub fn enable_steady_tick(&self) {
        match &self.pb {
            Some(pb) => pb.enable_steady_tick(Duration::from_millis(100)),
            None => (),
        }
    }

    pub fn set_prefix(&mut self, prefix: String) {
        match &self.pb {
            Some(pb) => pb.set_prefix(prefix),
            None => {
                self.prefix = prefix;
            }
        }
    }

    pub fn set_style(&self, style: indicatif::ProgressStyle) {
        match &self.pb {
            Some(pb) => {
                pb.set_style(style);
                pb.set_prefix(COLOR.dimmed("rtx"));
            }
            None => (),
        }
    }
    pub fn set_message(&self, message: String) {
        match &self.pb {
            Some(pb) => pb.set_message(message),
            None => eprintln!("{}{message}", self.prefix),
        }
    }
    pub fn println(&self, message: String) {
        match &self.pb {
            Some(pb) => pb.println(message),
            None => eprintln!("{message}"),
        }
    }
    pub fn finish_with_message(&self, message: String) {
        match &self.pb {
            Some(pb) => pb.finish_with_message(message),
            None => eprintln!("{}{message}", self.prefix),
        }
    }
}

static COLOR: Lazy<Color> = Lazy::new(|| Color::new(Stream::Stderr));
