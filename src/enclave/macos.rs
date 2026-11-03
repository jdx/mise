use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use eyre::{Result, bail};

const HELPER: &[u8] = include_bytes!(concat!(
    env!("OUT_DIR"),
    "/mise-enclave.app/Contents/MacOS/mise-enclave"
));
const INFO: &[u8] = include_bytes!(concat!(
    env!("OUT_DIR"),
    "/mise-enclave.app/Contents/Info.plist"
));
const CODE_RESOURCES: &[u8] = include_bytes!(concat!(
    env!("OUT_DIR"),
    "/mise-enclave.app/Contents/_CodeSignature/CodeResources"
));

fn release_signed() -> bool {
    env!("MISE_ENCLAVE_RELEASE_SIGNED") == "1" && !HELPER.is_empty()
}

fn executable(app: &Path) -> PathBuf {
    app.join("Contents/MacOS/mise-enclave")
}

fn complete(app: &Path) -> bool {
    executable(app).is_file()
        && app.join("Contents/Info.plist").is_file()
        && app.join("Contents/_CodeSignature/CodeResources").is_file()
}

/// The path of the installed helper executable.
pub(super) fn helper() -> Result<PathBuf> {
    if !release_signed() {
        bail!("this build of mise has no Developer ID signed Secure Enclave helper");
    }
    let app = ensure_app(&crate::dirs::DATA.join("enclave"))?;
    Ok(executable(&app))
}

fn ensure_app(root: &Path) -> Result<PathBuf> {
    // A versioned directory permits safe replacement without modifying a
    // running helper.
    let fingerprint = crate::hash::hash_to_str(&(HELPER, INFO, CODE_RESOURCES));
    let app = root.join(fingerprint).join("mise-enclave.app");
    if complete(&app) {
        return Ok(app);
    }
    crate::file::create_dir_all(root)?;
    let mut lock = fslock::LockFile::open(&root.join("install.lock"))?;
    // The helper is tiny; wait for a concurrent installer instead of failing.
    lock.lock()?;
    if complete(&app) {
        return Ok(app);
    }
    let staging = tempfile::tempdir_in(root)?;
    let staged = staging.path().join("mise-enclave.app");
    let contents = staged.join("Contents");
    std::fs::create_dir_all(contents.join("MacOS"))?;
    std::fs::create_dir_all(contents.join("_CodeSignature"))?;
    std::fs::write(executable(&staged), HELPER)?;
    std::fs::set_permissions(executable(&staged), std::fs::Permissions::from_mode(0o755))?;
    std::fs::write(contents.join("Info.plist"), INFO)?;
    std::fs::write(
        contents.join("_CodeSignature/CodeResources"),
        CODE_RESOURCES,
    )?;
    std::fs::create_dir_all(app.parent().unwrap())?;
    if app.symlink_metadata().is_ok() {
        // Keep an incomplete installation for inspection rather than deleting
        // anything; the install lock serializes repair.
        let quarantine = tempfile::Builder::new()
            .prefix("incomplete-")
            .tempdir_in(root)?;
        std::fs::rename(&app, quarantine.path().join("mise-enclave.app"))?;
        let _ = quarantine.keep();
    }
    std::fs::rename(staged, &app)?;
    Ok(app)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::process::{Command, Stdio};

    #[test]
    fn helper_installs_and_rejects_bad_input_without_prompting() {
        if HELPER.is_empty() {
            return;
        }
        let temp = tempfile::tempdir().unwrap();
        let app = ensure_app(temp.path()).unwrap();
        assert_eq!(ensure_app(temp.path()).unwrap(), app);
        let mut child = Command::new(executable(&app))
            .arg("unseal")
            .stdin(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(b"not sealed\n")
            .unwrap();
        assert!(!child.wait().unwrap().success());
    }
}
