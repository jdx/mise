//! A real application identity is required for a custom notification icon.
//! The small native helper is embedded in mise, not compiled on the user's Mac.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use eyre::{Result, bail};

const HELPER: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/mise-notify"));
const ICON: &[u8] = include_bytes!("../../../../docs/public/android-chrome-512x512.png");
const INFO: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>dev.jdx.mise.notifications</string>
<key>CFBundleName</key><string>mise</string>
<key>CFBundleDisplayName</key><string>mise</string>
<key>CFBundleExecutable</key><string>mise-notify</string>
<key>CFBundleIconFile</key><string>mise.icns</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleVersion</key><string>1</string>
<key>LSUIElement</key><true/>
<key>LSMinimumSystemVersion</key><string>10.14</string>
</dict></plist>"#;

fn app_path(root: &Path) -> PathBuf {
    // A versioned directory permits safe replacement without modifying a
    // running helper. The bundle identifier remains stable across versions.
    let fingerprint = crate::hash::hash_to_str(&(HELPER, ICON, INFO));
    root.join(fingerprint).join("mise.app")
}

fn executable(app: &Path) -> PathBuf {
    app.join("Contents/MacOS/mise-notify")
}

pub(super) fn notification(title: &str, body: &str) -> Result<Command> {
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
    if executable(&app).is_file() {
        return Ok(app);
    }
    crate::file::create_dir_all(root)?;
    let mut lock = fslock::LockFile::open(&root.join("install.lock"))?;
    lock.lock()?;
    if executable(&app).is_file() {
        return Ok(app);
    }
    let staging = tempfile::tempdir_in(root)?;
    let staged = staging.path().join("mise.app");
    let contents = staged.join("Contents");
    std::fs::create_dir_all(contents.join("MacOS"))?;
    std::fs::create_dir_all(contents.join("Resources"))?;
    std::fs::write(executable(&staged), HELPER)?;
    std::fs::set_permissions(executable(&staged), std::fs::Permissions::from_mode(0o755))?;
    std::fs::write(contents.join("Info.plist"), INFO)?;
    // ICNS container with one lossless 512x512 PNG representation (ic09).
    let mut icon = Vec::with_capacity(ICON.len() + 16);
    icon.extend_from_slice(b"icns");
    icon.extend_from_slice(&u32::try_from(ICON.len() + 16)?.to_be_bytes());
    icon.extend_from_slice(b"ic09");
    icon.extend_from_slice(&u32::try_from(ICON.len() + 8)?.to_be_bytes());
    icon.extend_from_slice(ICON);
    std::fs::write(contents.join("Resources/mise.icns"), icon)?;
    // Ad-hoc sign this mise-owned bundle, never another installed app.
    let signed = Command::new("/usr/bin/codesign")
        .args([
            "--force",
            "--sign",
            "-",
            "--identifier",
            "dev.jdx.mise.notifications",
        ])
        .arg(&staged)
        .output()?;
    if !signed.status.success() {
        bail!("could not sign the mise notification helper");
    }
    std::fs::create_dir_all(app.parent().unwrap())?;
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
}
