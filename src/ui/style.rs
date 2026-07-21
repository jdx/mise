use std::path::Path;
use std::sync::LazyLock;

use crate::file::display_path;
use console::{Color, StyledObject, style};

pub fn estyle<D>(val: D) -> StyledObject<D> {
    style(val).for_stderr()
}

pub fn ecyan<D>(val: D) -> StyledObject<D> {
    estyle(val).cyan()
}

pub fn eblue<D>(val: D) -> StyledObject<D> {
    estyle(val).blue()
}

pub fn emagenta<D>(val: D) -> StyledObject<D> {
    estyle(val).magenta()
}

pub fn egreen<D>(val: D) -> StyledObject<D> {
    estyle(val).green()
}

pub fn eyellow<D>(val: D) -> StyledObject<D> {
    estyle(val).yellow()
}

pub fn ered<D>(val: D) -> StyledObject<D> {
    estyle(val).red()
}

pub fn eblack<D>(val: D) -> StyledObject<D> {
    estyle(val).black()
}

pub fn eunderline<D>(val: D) -> StyledObject<D> {
    estyle(val).underlined()
}

pub fn edim<D>(val: D) -> StyledObject<D> {
    estyle(val).dim()
}

pub fn ebold<D>(val: D) -> StyledObject<D> {
    estyle(val).bold()
}

pub fn nbold<D>(val: D) -> StyledObject<D> {
    nstyle(val).bold()
}

pub fn epath(path: &Path) -> StyledObject<String> {
    estyle(display_path(path))
}

pub fn nstyle<D>(val: D) -> StyledObject<D> {
    style(val).for_stdout()
}

pub fn ncyan<D>(val: D) -> StyledObject<D> {
    nstyle(val).cyan()
}

pub fn nunderline<D>(val: D) -> StyledObject<D> {
    nstyle(val).underlined()
}

pub fn nyellow<D>(val: D) -> StyledObject<D> {
    nstyle(val).yellow()
}

pub fn nred<D>(val: D) -> StyledObject<D> {
    nstyle(val).red()
}

pub fn ndim<D>(val: D) -> StyledObject<D> {
    nstyle(val).dim()
}

pub fn nbright<D>(val: D) -> StyledObject<D> {
    nstyle(val).bright()
}

pub fn prefix(label: impl Into<String>, hash_key: impl AsRef<str>, stderr: bool) -> String {
    static COLORS: LazyLock<Vec<Color>> =
        LazyLock::new(|| vec![Color::Blue, Color::Magenta, Color::Cyan, Color::Green]);

    let label = label.into();
    let hash = hash_key.as_ref().chars().map(|c| c as usize).sum::<usize>();
    let styled = style(label).fg(COLORS[hash % COLORS.len()]);
    let mut styled = if stderr {
        styled.for_stderr()
    } else {
        styled.for_stdout()
    };
    match (hash / COLORS.len()) % 4 {
        1 => styled = styled.bold(),
        2 => styled = styled.dim(),
        3 => styled = styled.bright(),
        _ => {}
    }

    styled.to_string()
}
