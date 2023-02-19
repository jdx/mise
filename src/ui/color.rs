use atty::Stream;
use owo_colors::OwoColorize;

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

    fn bold(&self, s: &str) -> String {
        s.if_supports_color(self.stream, |s| s.bold()).to_string()
    }

    fn underline(&self, s: &str) -> String {
        s.if_supports_color(self.stream, |s| s.underline())
            .to_string()
    }
}
