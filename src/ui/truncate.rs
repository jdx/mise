use crate::env::TERM_WIDTH;
use std::borrow::Cow;

pub fn screen_trunc(s: &str) -> Cow<str> {
    console::truncate_str(s, *TERM_WIDTH, "…")
}
