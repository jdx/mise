use owo_colors::OwoColorize;
use std::io::stdin;

pub fn prompt() -> String {
    let mut input = String::new();
    stdin().read_line(&mut input).expect("error reading stdin");

    input.trim().to_string()
}

pub fn prompt_for_install(thing: &str) -> bool {
    match is_tty() {
        true => {
            eprint!(
                "rtx: Would you like to install plugin {}? [Y/n] ",
                thing.cyan()
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
