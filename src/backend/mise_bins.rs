//! Entries in a tool's `.mise-bins` directory.
//!
//! `.mise-bins` holds only the executables a tool declares, so bundled helpers stay off `PATH`.
//! On Windows an entry is a copy, and a copy runs from `.mise-bins`: a program that finds its
//! DLLs or data through its own path (exiftool's `exiftool_files`, Electron's `resources`) looks
//! there and fails. Each copy therefore records which file it was copied from, and `mise x` runs
//! that file instead (jdx/mise#14369).

use std::path::{Path, PathBuf};

use eyre::{Result, WrapErr};

use crate::file;

/// Put `src` in a `.mise-bins` directory as `dst`, recording where it came from when the entry is
/// a copy.
pub(crate) fn link(src: &Path, dst: &Path) -> Result<()> {
    file::make_symlink_or_copy(src, dst)?;
    if cfg!(windows) {
        record_source(src, dst)?;
    }
    Ok(())
}

/// The file the `.mise-bins` entry `program` was copied from, when it recorded one that exists.
#[cfg(any(windows, test))]
pub fn source_of(program: &Path) -> Option<PathBuf> {
    use std::path::Component;

    let bins_dir = program.parent()?;
    if bins_dir.file_name()? != super::MISE_BINS_DIR {
        return None;
    }
    let root = bins_dir.parent()?;
    let recorded = std::fs::read_to_string(source_file(program)?).ok()?;
    let relative = Path::new(recorded.trim_end());
    // Only a path inside the install: the record sits in a directory on PATH.
    if !relative
        .components()
        .all(|component| matches!(component, Component::Normal(_)))
    {
        return None;
    }
    let source = root.join(relative);
    // The writer records only an original of the same name; anything else is not this entry's.
    let same_name = source
        .file_name()
        .zip(program.file_name())
        .is_some_and(|(source, entry)| source.eq_ignore_ascii_case(entry));
    (same_name && source.is_file()).then_some(source)
}

/// Write the path of `src`, relative to the directory holding `.mise-bins`, beside `dst`. It is
/// relative so it survives the install moving or being reached through a version link.
///
/// A source outside that directory is not recorded, and neither is one with another file name:
/// running it would change the name the program sees itself called by, which a multi-call binary
/// such as uutils coreutils dispatches on. Either entry keeps running as it is, and a record left
/// by an earlier entry of that name is removed.
fn record_source(src: &Path, dst: &Path) -> Result<()> {
    let Some(record) = source_file(dst) else {
        return Ok(());
    };
    let same_name = match (src.file_name(), dst.file_name()) {
        (Some(src), Some(dst)) => src.eq_ignore_ascii_case(dst),
        _ => false,
    };
    let root = dst.parent().and_then(Path::parent);
    let relative = root.and_then(|root| src.strip_prefix(root).ok());
    let (true, Some(relative)) = (same_name, relative) else {
        return match std::fs::remove_file(&record) {
            Err(err) if err.kind() != std::io::ErrorKind::NotFound => {
                Err(err).wrap_err_with(|| format!("failed rm: {}", file::display_path(&record)))
            }
            _ => Ok(()),
        };
    };
    let relative = relative
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");
    file::write(record, relative)
}

/// `.mise-bins/.<name>.source`: a dotfile, so shim generation and `PATH` lookups skip it.
fn source_file(entry: &Path) -> Option<PathBuf> {
    let name = entry.file_name()?.to_str()?;
    Some(entry.with_file_name(format!(".{name}.source")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::MISE_BINS_DIR;

    #[test]
    fn source_of_follows_the_record_to_the_original() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let original = root.join("exiftool-13.59_64").join("exiftool.exe");
        std::fs::create_dir_all(original.parent().unwrap()).unwrap();
        std::fs::write(&original, "").unwrap();
        let bins = root.join(MISE_BINS_DIR);
        std::fs::create_dir_all(&bins).unwrap();
        let entry = bins.join("exiftool.exe");
        std::fs::copy(&original, &entry).unwrap();

        assert_eq!(source_of(&entry), None, "no record: run the entry as it is");
        record_source(&original, &entry).unwrap();
        assert_eq!(
            std::fs::read_to_string(bins.join(".exiftool.exe.source")).unwrap(),
            "exiftool-13.59_64/exiftool.exe"
        );
        assert_eq!(source_of(&entry), Some(original.clone()));

        std::fs::remove_file(&original).unwrap();
        assert_eq!(source_of(&entry), None, "a missing original is not run");
    }

    #[test]
    fn source_of_ignores_paths_outside_mise_bins_and_the_install() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("install");
        let bins = root.join(MISE_BINS_DIR);
        std::fs::create_dir_all(&bins).unwrap();
        std::fs::write(tmp.path().join("outside.exe"), "").unwrap();
        let entry = bins.join("tool.exe");
        std::fs::write(&entry, "").unwrap();
        std::fs::write(bins.join(".tool.exe.source"), "../outside.exe").unwrap();
        assert_eq!(source_of(&entry), None);

        let bin = root.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("tool.exe"), "").unwrap();
        std::fs::write(bin.join(".tool.exe.source"), "tool.exe").unwrap();
        assert_eq!(source_of(&bin.join("tool.exe")), None);

        // a source outside the install is not recorded
        record_source(&tmp.path().join("outside.exe"), &bins.join("outside.exe")).unwrap();
        assert!(!bins.join(".outside.exe.source").exists());
        // nor is one the entry renames, and it replaces an earlier record
        std::fs::write(bins.join(".renamed.exe.source"), "bin/renamed.exe").unwrap();
        record_source(&bin.join("tool.exe"), &bins.join("renamed.exe")).unwrap();
        assert!(!bins.join(".renamed.exe.source").exists());

        // a record naming another file is not this entry's
        std::fs::write(bins.join(".tool.exe.source"), "bin/tool.exe").unwrap();
        assert_eq!(source_of(&entry), Some(bin.join("tool.exe")));
        std::fs::write(bin.join("other.exe"), "").unwrap();
        std::fs::write(bins.join(".tool.exe.source"), "bin/other.exe").unwrap();
        assert_eq!(source_of(&entry), None);
    }
}
