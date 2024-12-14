use std::path::Path;

use crate::file::display_path;
use console::{style, StyledObject};

pub fn ereset() -> String {
    if console::colors_enabled_stderr() {
        "\x1b[0m".to_string()
    } else {
        "".to_string()
    }
}

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
