//! A real application identity is required for a custom notification icon.
//! The small native helper is embedded in mise, not compiled on the user's Mac.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use eyre::{Result, bail};

const HELPER: &[u8] = include_bytes!(concat!(
    env!("OUT_DIR"),
    "/mise-notify.app/Contents/MacOS/mise-notify"
));
const INFO: &[u8] = include_bytes!(concat!(
    env!("OUT_DIR"),
    "/mise-notify.app/Contents/Info.plist"
));
const ICON: &[u8] = include_bytes!(concat!(
    env!("OUT_DIR"),
    "/mise-notify.app/Contents/Resources/mise.icns"
));
#[cfg(mise_notification_has_signature_resources)]
const CODE_RESOURCES: &[u8] = include_bytes!(concat!(
    env!("OUT_DIR"),
    "/mise-notify.app/Contents/_CodeSignature/CodeResources"
));

pub(super) fn release_signed() -> bool {
    env!("MISE_NOTIFICATION_RELEASE_SIGNED") == "1"
}

fn app_path(root: &Path) -> PathBuf {
    // A versioned directory permits safe replacement without modifying a
    // running helper. The bundle identifier remains stable across versions.
    let fingerprint = bundle_fingerprint();
    root.join(fingerprint).join("mise.app")
}

#[cfg(mise_notification_has_signature_resources)]
fn bundle_fingerprint() -> String {
    crate::hash::hash_to_str(&(HELPER, INFO, ICON, CODE_RESOURCES))
}

#[cfg(not(mise_notification_has_signature_resources))]
fn bundle_fingerprint() -> String {
    crate::hash::hash_to_str(&(HELPER, INFO, ICON))
}

fn executable(app: &Path) -> PathBuf {
    app.join("Contents/MacOS/mise-notify")
}

#[cfg(mise_notification_has_signature_resources)]
fn complete(app: &Path) -> bool {
    executable(app).is_file()
        && app.join("Contents/Info.plist").is_file()
        && app.join("Contents/Resources/mise.icns").is_file()
        && app.join("Contents/_CodeSignature/CodeResources").is_file()
}

#[cfg(not(mise_notification_has_signature_resources))]
fn complete(app: &Path) -> bool {
    executable(app).is_file()
        && app.join("Contents/Info.plist").is_file()
        && app.join("Contents/Resources/mise.icns").is_file()
}

pub(super) fn notification(title: &str, body: &str) -> Result<Command> {
    if !release_signed() {
        bail!("the embedded notification helper is not Developer ID signed");
    }
    let app = ensure_app(&crate::dirs::DATA.join("notifications"))?;
    Ok(notification_command(&app, title, body))
}

fn notification_command(app: &Path, title: &str, body: &str) -> Command {
    let mut command = Command::new(executable(app));
    command.args([title, body]);
    command
}

fn ensure_app(root: &Path) -> Result<PathBuf> {
    let app = app_path(root);
    if complete(&app) {
        return Ok(app);
    }
    crate::file::create_dir_all(root)?;
    let mut lock = fslock::LockFile::open(&root.join("install.lock"))?;
    if !lock.try_lock()? {
        bail!("another process is installing the mise notification helper");
    }
    if complete(&app) {
        return Ok(app);
    }
    let staging = tempfile::tempdir_in(root)?;
    let staged = staging.path().join("mise.app");
    let contents = staged.join("Contents");
    std::fs::create_dir_all(contents.join("MacOS"))?;
    std::fs::create_dir_all(contents.join("Resources"))?;
    #[cfg(mise_notification_has_signature_resources)]
    std::fs::create_dir_all(contents.join("_CodeSignature"))?;
    std::fs::write(executable(&staged), HELPER)?;
    std::fs::set_permissions(executable(&staged), std::fs::Permissions::from_mode(0o755))?;
    std::fs::write(contents.join("Info.plist"), INFO)?;
    std::fs::write(contents.join("Resources/mise.icns"), ICON)?;
    #[cfg(mise_notification_has_signature_resources)]
    std::fs::write(
        contents.join("_CodeSignature/CodeResources"),
        CODE_RESOURCES,
    )?;
    std::fs::create_dir_all(app.parent().unwrap())?;
    if app.symlink_metadata().is_ok() {
        // Preserve an incomplete installation for inspection, rather than
        // deleting anything a user may have placed in it. The install lock
        // serializes repair with other mise processes.
        let quarantine = tempfile::Builder::new()
            .prefix("incomplete-")
            .tempdir_in(root)?;
        std::fs::rename(&app, quarantine.path().join("mise.app"))?;
        let preserved = quarantine.keep();
        debug!(
            "preserved incomplete notification helper at {}",
            preserved.display()
        );
    }
    std::fs::rename(staged, &app)?;
    Ok(app)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notification_helper_has_its_own_identity_and_decodable_icon_without_notifying() {
        let temp = tempfile::tempdir().unwrap();
        let app = ensure_app(temp.path()).unwrap();
        assert!(
            Command::new(executable(&app))
                .arg("--check")
                .status()
                .unwrap()
                .success()
        );
        #[cfg(mise_notification_has_signature_resources)]
        assert!(
            Command::new("/usr/bin/codesign")
                .args(["--verify", "--strict"])
                .arg(&app)
                .status()
                .unwrap()
                .success()
        );
        assert_eq!(ensure_app(temp.path()).unwrap(), app);
        assert!(Command::new(executable(&app)).status().unwrap().success());
    }

    #[test]
    fn notification_text_is_literal_and_install_failure_is_reported() {
        let command = notification_command(
            Path::new("/app with spaces/mise.app"),
            "[mise]",
            "\"file\"; $(touch unwanted)",
        );
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            ["[mise]", "\"file\"; $(touch unwanted)"]
        );
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("not-a-directory");
        std::fs::write(&root, b"").unwrap();
        assert!(ensure_app(&root).is_err());
    }

    #[test]
    fn incomplete_installation_is_repaired_without_discarding_its_contents() {
        for missing in ["Contents/MacOS/mise-notify", "Contents/Resources/mise.icns"] {
            let temp = tempfile::tempdir().unwrap();
            let app = ensure_app(temp.path()).unwrap();
            std::fs::remove_file(app.join(missing)).unwrap();
            std::fs::write(app.join("inspection.txt"), "preserve me").unwrap();
            assert_eq!(ensure_app(temp.path()).unwrap(), app);
            assert!(complete(&app));
            let preserved = std::fs::read_dir(temp.path())
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .find(|path| {
                    path.file_name()
                        .unwrap()
                        .to_string_lossy()
                        .starts_with("incomplete-")
                })
                .unwrap();
            assert_eq!(
                std::fs::read_to_string(preserved.join("mise.app/inspection.txt")).unwrap(),
                "preserve me"
            );
        }
    }
}
