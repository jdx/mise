use std::path::Path;

use console::{style, StyledObject};

use crate::file::display_path;

pub fn estyle<D>(val: D) -> StyledObject<D> {
    style(val).for_stderr()
}

pub fn ecyan<D>(val: D) -> StyledObject<D> {
    estyle(val).cyan()
}

pub fn eblue<D>(val: D) -> StyledObject<D> {
    estyle(val).blue()
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

pub fn nblue<D>(val: D) -> StyledObject<D> {
    nstyle(val).cyan()
}

pub fn nbold<D>(val: D) -> StyledObject<D> {
    nstyle(val).bold()
}
