use atty::Stream;
use owo_colors::OwoColorize;

pub fn dimmed(stream: Stream, s: &str) -> String {
    s.if_supports_color(stream, |s| s.dimmed()).to_string()
}
pub fn _yellow(stream: Stream, s: &str) -> String {
    s.if_supports_color(stream, |s| s.yellow()).to_string()
}
pub fn cyan(stream: Stream, s: &str) -> String {
    s.if_supports_color(stream, |s| s.cyan()).to_string()
}
pub fn green(stream: Stream, s: &str) -> String {
    s.if_supports_color(stream, |s| s.green()).to_string()
}
pub fn _bright_green(stream: Stream, s: &str) -> String {
    s.if_supports_color(stream, |s| s.bright_green())
        .to_string()
}

pub fn red(stream: Stream, s: &str) -> String {
    s.if_supports_color(stream, |s| s.red()).to_string()
}
