use atty::Stream;
use owo_colors::OwoColorize;

pub fn cyan(stream: Stream, s: &str) -> String {
    s.if_supports_color(stream, |s| s.cyan()).to_string()
}

pub struct Color {
    stream: Stream,
}

impl Color {
    pub fn new(stream: Stream) -> Self {
        Self { stream }
    }

    pub fn header(&self, title: &str) -> String {
        self.underline(&self.bold(title))
    }

    pub fn dimmed(&self, s: &str) -> String {
        s.if_supports_color(self.stream, |s| s.dimmed()).to_string()
    }

    pub fn bold(&self, s: &str) -> String {
        s.if_supports_color(self.stream, |s| s.bold()).to_string()
    }

    pub fn underline(&self, s: &str) -> String {
        s.if_supports_color(self.stream, |s| s.underline())
            .to_string()
    }

    pub fn cyan(&self, s: &str) -> String {
        s.if_supports_color(self.stream, |s| s.cyan()).to_string()
    }

    pub fn green(&self, s: &str) -> String {
        s.if_supports_color(self.stream, |s| s.green()).to_string()
    }
    pub fn red(&self, s: &str) -> String {
        s.if_supports_color(self.stream, |s| s.red()).to_string()
    }
    pub fn bright_yellow(&self, s: &str) -> String {
        s.if_supports_color(self.stream, |s| s.bright_yellow())
            .to_string()
    }
}
