use eyre::{Result, bail, eyre};
use std::ffi::OsString;
use std::path::Path;

/// Open `file` in the user's editor and wait for it to exit.
///
/// The wait is part of the contract, not an accident of blocking IO: `mise dotfiles edit --apply`
/// converges the target as soon as this returns, so an editor that detached would apply a file the
/// user had not saved yet.
pub(super) fn open_in_editor(file: &Path) -> Result<()> {
    open_in_editor_with(&crate::env::EDITOR, file)
}

/// Takes the editor rather than reading it, so the failure path can be driven from a test.
fn open_in_editor_with(editor: &str, file: &Path) -> Result<()> {
    let (program, mut args) = split_editor_command(editor)?;
    args.push(file.as_os_str().into());
    if let Err(err) = crate::cmd::cmd(&program, args).run() {
        // Name the program and the variables that choose it. A child that never starts comes back
        // as std's bare "program not found" (see the note in `backend::which_spawnable`), which
        // says neither what mise tried to run nor where the name came from — and the default is
        // the name most likely to be missing.
        bail!(
            "failed to open the editor `{program}`: {err}\n\
             Set $EDITOR or $VISUAL to an editor mise can run."
        );
    }
    Ok(())
}

fn split_editor_command(editor: &str) -> Result<(String, Vec<OsString>)> {
    let mut parts = shell_words::split(editor)
        .map_err(|e| eyre!("failed to parse EDITOR/VISUAL value {:?}: {}", editor, e))?
        .into_iter();
    let program = parts
        .next()
        .ok_or_else(|| eyre!("EDITOR/VISUAL is empty"))?;

    Ok((program, parts.map(Into::into).collect()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_editor_with_arguments() {
        let (program, args) = split_editor_command("cat -n").unwrap();

        assert_eq!(program, "cat");
        assert_eq!(args.len(), 1);
        assert_eq!(args[0].as_os_str(), std::ffi::OsStr::new("-n"));
    }

    #[test]
    fn parses_editor_with_quoted_path() {
        let (program, args) =
            split_editor_command(r#""/Applications/My Editor.app/editor" --wait"#).unwrap();

        assert_eq!(program, "/Applications/My Editor.app/editor");
        assert_eq!(args.len(), 1);
        assert_eq!(args[0], "--wait");
    }

    #[test]
    fn errors_on_empty_editor() {
        assert!(split_editor_command("").is_err());
    }

    /// The failure this exists for: an editor that cannot be started. The bare error is std's
    /// `program not found`, which names nothing — on Windows with neither variable set that is
    /// the whole of what `mise tasks edit` used to print.
    #[test]
    fn an_editor_that_cannot_start_is_named_along_with_the_variables_that_choose_it() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("task");
        std::fs::write(&file, "").unwrap();
        let missing = dir.path().join("editor-that-is-not-there");

        // Control: the spawn really does fail. Without it this test would pass just as well on an
        // editor that started, and prove nothing about the message.
        assert!(crate::cmd::cmd(&missing, [file.as_os_str()]).run().is_err());

        let err = open_in_editor_with(&missing.to_string_lossy(), &file)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("editor-that-is-not-there"),
            "the message has to name the program: {err}"
        );
        assert!(
            err.contains("EDITOR") && err.contains("VISUAL"),
            "and the variables that would fix it: {err}"
        );
    }
}
