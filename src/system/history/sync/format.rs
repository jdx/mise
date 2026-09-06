//! The explicit versioned marker of a history-enabled setup repository:
//! `.mise-history/format.toml` with `format = 1`. A repository without it is
//! an ordinary repository and keeps its old `--from`/`--from-git`
//! behaviour; a newer format stops with an upgrade message.

use eyre::{Result, bail};
use serde::Deserialize;

use super::layout::MARKER_PATH;
use crate::system::history::shadow::HistoryRepo;

pub(crate) const FORMAT: u32 = 1;

#[derive(Deserialize)]
struct Marker {
    format: u32,
}

pub(crate) fn marker_content() -> String {
    format!(
        "# This repository is a mise setup repository (`mise bootstrap dotfiles origin set`).\n# Do not edit: mise reads this to recognize the layout it publishes.\nformat = {FORMAT}\n"
    )
}

pub(crate) fn parse_marker(bytes: &[u8]) -> Result<u32> {
    let text = String::from_utf8_lossy(bytes);
    let marker: Marker = toml::from_str(&text)?;
    Ok(marker.format)
}

/// What the fetched setup branch is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum RepoState {
    /// No branch yet: the first publication creates it with the marker.
    Empty,
    /// A history-enabled repository of the given format.
    Marked(u32),
    /// An existing, non-empty repository without the marker: adopted only
    /// after a deliberate confirmation.
    Unmarked,
}

impl RepoState {
    /// Fails for a format this mise does not understand.
    pub(crate) fn check(&self) -> Result<()> {
        if let Self::Marked(format) = self
            && *format != FORMAT
        {
            bail!(
                "this setup repository uses format {format}; upgrade mise (this version supports {FORMAT})"
            );
        }
        Ok(())
    }
}

pub(crate) fn detect(repo: &HistoryRepo, upstream: Option<&str>) -> Result<RepoState> {
    let Some(commit) = upstream else {
        return Ok(RepoState::Empty);
    };
    match repo.object_at(commit, MARKER_PATH)? {
        Some((_, oid)) => Ok(RepoState::Marked(parse_marker(&repo.cat_object(&oid)?)?)),
        None => Ok(RepoState::Unmarked),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_round_trips() {
        assert_eq!(parse_marker(marker_content().as_bytes()).unwrap(), FORMAT);
        assert!(RepoState::Marked(FORMAT).check().is_ok());
        let err = RepoState::Marked(99).check().unwrap_err().to_string();
        assert!(err.contains("format 99"), "{err}");
        assert!(err.contains("upgrade mise"), "{err}");
        assert!(RepoState::Unmarked.check().is_ok());
    }
}
