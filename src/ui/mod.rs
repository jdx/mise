pub mod prompt;

pub fn is_tty() -> bool {
    atty::is(atty::Stream::Stdin)
        && atty::is(atty::Stream::Stderr)
        && atty::is(atty::Stream::Stdout)
}
