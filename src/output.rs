use atty::Stream;
use owo_colors::OwoColorize;
use std::io;
use std::io::Write;
use std::process::ExitCode;

#[derive(Debug)]
pub enum OutputType {
    Stdout,
    Stderr,
}

#[derive(Debug)]
pub struct Output {
    pub stdout: OutputStream,
    pub stderr: OutputStream,
    pub status: ExitCode,
}

impl Output {
    pub fn new() -> Self {
        Self {
            stdout: OutputStream::new(OutputType::Stdout),
            stderr: OutputStream::new(OutputType::Stderr),
            status: ExitCode::from(0),
        }
    }

    #[cfg(test)]
    pub fn tracked() -> Self {
        let mut output = Self::new();
        output.stdout.track = true;
        output.stderr.track = true;

        output
    }
}

#[derive(Debug)]
pub struct OutputStream {
    pub content: String,
    pub output_type: OutputType,
    pub track: bool,
}

impl OutputStream {
    pub fn new(output_type: OutputType) -> Self {
        Self {
            content: Default::default(),
            track: false,
            output_type,
        }
    }
    pub fn write(&mut self, content: String) {
        if self.track {
            self.content.push_str(&content);
        } else {
            let _ = match self.output_type {
                OutputType::Stdout => io::stdout().write(content.as_bytes()),
                OutputType::Stderr => io::stderr().write(content.as_bytes()),
            };
        }
    }

    pub fn writeln(&mut self, content: String) {
        self.write(format!("{content}\n"));
    }
}

pub fn dim(stream: Stream, s: &str) -> String {
    s.if_supports_color(stream, |s| s.dimmed()).to_string()
}

#[macro_export]
macro_rules! rtxprintln {
    () => {
        rtxprint!("\n")
    };
    ($out:ident, $($arg:tt)*) => {{
        $out.stdout.writeln(format!($($arg)*));
    }};
}

#[macro_export]
macro_rules! rtxprint {
    ($out:ident, $($arg:tt)*) => {{
        $out.stdout.write(format!($($arg)*));
    }};
}

#[macro_export]
macro_rules! rtxstatusln {
    ($out:ident, $($arg:tt)*) => {{
        let rtx = $crate::output::dim(atty::Stream::Stderr, "rtx: ");
        $out.stderr.writeln(format!("{}{}", rtx, format!($($arg)*)));
    }};
}
