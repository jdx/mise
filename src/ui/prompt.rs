use crate::ui::color::Color;
use atty::Stream;
use once_cell::sync::Lazy;
use std::io::stdin;

static COLOR: Lazy<Color> = Lazy::new(|| Color::new(Stream::Stderr));

pub fn prompt() -> String {
    let mut input = String::new();
    stdin().read_line(&mut input).expect("error reading stdin");

    input.trim().to_string()
}

pub fn prompt_for_install(thing: &str) -> bool {
    match is_tty() {
        true => {
            eprint!(
                "{} Would you like to install {}? [Y/n] ",
                COLOR.dimmed("rtx:"),
                thing,
            );
            matches!(prompt().to_lowercase().as_str(), "" | "y" | "yes")
        }
        false => false,
    }
}

pub fn is_tty() -> bool {
    atty::is(atty::Stream::Stdin)
        && atty::is(atty::Stream::Stderr)
        && atty::is(atty::Stream::Stdout)
}
