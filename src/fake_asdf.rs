use color_eyre::eyre::Result;
use indoc::formatdoc;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

pub fn get_path(rtx_dir: &Path) -> PathBuf {
    rtx_dir.join(".fake-asdf")
}

pub fn setup(path: &Path) -> Result<()> {
    let asdf_bin = path.join("asdf");
    if !asdf_bin.exists() {
        fs::create_dir_all(path)?;
        fs::write(
            &asdf_bin,
            formatdoc! {r#"
                #!/bin/sh
                rtx="${{RTX_EXE:-rtx}}"
                "$rtx" asdf "$@"
            "#},
        )?;
        let mut perms = asdf_bin.metadata()?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&asdf_bin, perms)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {

    use super::*;
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn test_get_path() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test/fixtures");
        assert_eq!(get_path(&path), path.join(".fake-asdf"));
    }

    #[test]
    fn test_setup() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test/fixtures/fake-asdf");
        setup(&path).unwrap();
        assert!(path.join("asdf").exists());
        fs::remove_dir_all(&path).unwrap();
    }
}
