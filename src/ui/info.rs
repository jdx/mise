use crate::file;
use console::style;
use indenter::indented;
use std::fmt::{Display, Write};

pub fn section<S: Display>(header: &str, body: S) -> eyre::Result<()> {
    let body = file::replace_paths_in_string(body);
    let out = format!("\n{}: \n{}", style(header).bold(), indent_by(body, "  "));
    miseprintln!("{}", trim_line_end_whitespace(&out));
    Ok(())
}

pub fn inline_section<S: Display>(header: &str, body: S) -> eyre::Result<()> {
    let body = file::replace_paths_in_string(body);
    let out = format!("{}: {body}", style(header).bold());
    miseprintln!("{}", trim_line_end_whitespace(&out));
    Ok(())
}

pub fn indent_by<S: Display>(s: S, ind: &'static str) -> String {
    let mut out = String::new();
    write!(indented(&mut out).with_str(ind), "{s}").unwrap();
    out
}

pub fn trim_line_end_whitespace(s: &str) -> String {
    s.lines().map(str::trim_end).collect::<Vec<_>>().join("\n")
}
