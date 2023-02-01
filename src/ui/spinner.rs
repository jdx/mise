use spinoff::{spinners, Color, Streams};

pub struct Spinner {
    spinner: Option<spinoff::Spinner>,
}

impl Spinner {
    pub fn start(message: String, verbose: bool) -> Spinner {
        let sp = match verbose {
            true => {
                eprintln!("{message}");
                None
            }
            false => Some(spinoff::Spinner::new_with_stream(
                spinners::Dots10,
                message,
                Color::Blue,
                Streams::Stderr,
            )),
        };
        Spinner { spinner: sp }
    }

    pub fn success(&mut self, message: String) {
        if let Some(sp) = self.spinner.take() {
            sp.success(&message);
        }
    }
}
