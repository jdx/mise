use color_eyre::Result;
use color_eyre::eyre::bail;
use console::style;
#[cfg(windows)]
use indoc::formatdoc;
use self_update::backends::github::Update;
use self_update::{Status, cargo_crate_version};

use crate::cli::version::{ARCH, OS};
use crate::config::Settings;
use crate::env;
#[cfg(windows)]
use crate::file::MAX_PATH;
use std::collections::BTreeMap;
use std::fs;
#[cfg(target_os = "macos")]
use std::path::Path;
use std::path::PathBuf;

#[derive(Debug, Default, serde::Deserialize)]
struct InstructionsToml {
    message: Option<String>,
    #[serde(flatten)]
    commands: BTreeMap<String, String>,
}

fn read_instructions_file(path: &PathBuf) -> Option<String> {
    let body = fs::read_to_string(path).ok()?;
    let parsed: InstructionsToml = toml::from_str(&body).ok()?;
    if let Some(msg) = parsed.message {
        return Some(msg);
    }
    if let Some((_k, v)) = parsed.commands.into_iter().next() {
        return Some(v);
    }
    None
}

pub fn upgrade_instructions_text() -> Option<String> {
    if let Some(path) = &*env::MISE_SELF_UPDATE_INSTRUCTIONS
        && let Some(msg) = read_instructions_file(path)
    {
        return Some(msg);
    }
    None
}

/// Shown when mise cannot update itself and the packager shipped no instructions
/// file. Without it, telling the user their mise is out of date is a dead end on
/// every install that disables self-update: a marker file (Homebrew, the AUR
/// `mise-bin` package), a build without the `self_update` feature (Arch), or
/// `MISE_SELF_UPDATE_AVAILABLE=false`. The wording stays neutral about which of
/// those applies — being unable to self-update is not by itself proof that a
/// package manager owns the install.
pub const SELF_UPDATE_DISABLED_HINT: &str =
    "self-update is disabled for this install, update mise the same way you installed it";

/// How to update mise when `mise self-update` is not available: the packager's
/// instructions when they shipped some, otherwise the generic hint.
pub fn upgrade_instructions_or_hint() -> String {
    upgrade_instructions_text().unwrap_or_else(|| SELF_UPDATE_DISABLED_HINT.to_string())
}

/// Appends self-update guidance and packaging instructions (if any) to a message.
pub fn append_self_update_instructions(mut message: String) -> String {
    if SelfUpdate::is_available() {
        message.push_str("\nRun `mise self-update` to update mise");
    }
    if let Some(instructions) = upgrade_instructions_text() {
        message.push('\n');
        message.push_str(&instructions);
    } else if !SelfUpdate::is_available() {
        message.push('\n');
        message.push_str(SELF_UPDATE_DISABLED_HINT);
    }
    message
}

/// Updates mise itself.
///
/// Uses the GitHub Releases API to find the latest release and binary.
/// By default, this will also update any installed plugins.
/// Uses mise's GitHub token resolution chain for authenticated requests.
///
/// Packagers can disable this command so that mise is updated through the
/// package manager instead. See
/// https://mise.jdx.dev/contributing.html#packaging-and-self-update-instructions
#[derive(Debug, Default, clap::Args)]
#[clap(verbatim_doc_comment)]
pub struct SelfUpdate {
    /// Update to a specific version
    version: Option<String>,

    /// Update even if already up to date
    #[clap(long, short)]
    force: bool,

    /// Skip confirmation prompt
    #[clap(long, short)]
    yes: bool,

    /// Disable auto-updating plugins
    #[clap(long)]
    no_plugins: bool,
}

/// Whether replacing the running binary would destroy the install with `TEMP` set to `tmp`.
///
/// `self-replace` renames the running mise.exe out of its install directory *first*, then
/// launches a copy of it from `TEMP` to finish the swap. When that copy's path exceeds
/// `MAX_PATH` the launch fails — `CreateProcess` has no `\\?\` escape hatch the way the file
/// APIs do — and nothing puts mise back: the install directory is left empty and the binary is
/// stranded in `TEMP` under a generated name. The crate declares executable paths that long out
/// of scope (self-replace-1.5.0/src/windows.rs, in `self_delete_on_init`), so the only place to
/// stop this is before it starts.
#[cfg(windows)]
fn temp_dir_breaks_self_replace(tmp: &std::path::Path, exe_stem: Option<&str>) -> bool {
    helper_path_len(tmp, exe_stem) >= MAX_PATH
}

/// Length in UTF-16 code units of the helper's full path. MAX_PATH counts UTF-16 code units
/// and includes the terminating NUL, so a total of exactly MAX_PATH is already one too many.
/// `OsStr::len()` would be the wrong unit: it counts WTF-8 bytes.
#[cfg(windows)]
fn helper_path_len(tmp: &std::path::Path, exe_stem: Option<&str>) -> usize {
    use std::os::windows::ffi::OsStrExt;

    // `Path::join` only inserts a separator when there is not one already, and Windows'
    // `temp_dir()` always comes back with a trailing backslash.
    let separator = usize::from(!ends_with_separator(tmp));
    tmp.as_os_str().encode_wide().count() + separator + helper_name_len(exe_stem)
}

/// Length in UTF-16 code units of the name `self-replace` generates for the helper:
/// `.` + the running executable's file stem + `.` + 32 random characters +
/// `.__selfdelete__.exe`, with the stem included only when it is valid UTF-8. Mirrors
/// `get_temp_executable_name` in self-replace-1.5.0/src/windows.rs. This is 57 for
/// `mise.exe` and longer whenever the binary has been renamed, so it cannot be a constant.
#[cfg(windows)]
fn helper_name_len(exe_stem: Option<&str>) -> usize {
    let suffix_len = env::SELF_REPLACE_SUFFIXES[0].len();

    // The stem is followed by a second `.`, and dropped entirely when it is not UTF-8.
    let stem = exe_stem.map_or(0, |s| s.encode_utf16().count() + 1);
    1 + stem + env::SELF_REPLACE_RANDOM_LEN + suffix_len
}

/// Delete the copies of mise that earlier updates left in `TEMP`.
///
/// `self-replace` moves the running binary aside and spawns a copy of it to delete the leftovers.
/// When that copy does not delete itself the deletion never happens and a **full copy of mise.exe**
/// stays in `TEMP` for good. Nothing else collects them: they are not under the cache, so
/// `mise cache clear` does not reach them, and their names mean nothing to anyone else.
///
/// A long `TEMP` is not the only trigger, though it was the one this was first written for
/// (measured at 199 and 201 characters, just under the length #12062 refuses outright). Measured
/// again on a `TEMP` of 31: a successful update leaves **both** copies — the `__relocated__`
/// original and the `__selfdelete__` helper — and neither is locked afterwards, so any later mise
/// can remove them. That is what this exists to do.
///
/// Best effort by design. A copy another mise is still using cannot be deleted on Windows, which is
/// the outcome we want, so failures are ignored rather than warned about.
#[cfg(windows)]
fn sweep_helper_orphans() {
    for (path, _) in helper_orphans() {
        match std::fs::remove_file(&path) {
            Ok(()) => debug!("removed stale self-update copy: {}", path.display()),
            Err(e) => trace!("could not remove {}: {e}", path.display()),
        }
    }
}

/// The copies an earlier update left in `TEMP`, with their sizes.
///
/// Shared with `mise doctor` so that "what counts as a leftover" has one definition rather than two
/// that can drift: the predicate stays [`env::is_self_replace_helper`], and this is only the walk.
/// A file whose size cannot be read is still reported, at 0 — it exists, which is the part that
/// matters, and the size is decoration.
#[cfg(windows)]
pub(crate) fn helper_orphans() -> Vec<(std::path::PathBuf, u64)> {
    let Some(stem) = current_exe_stem() else {
        return Vec::new();
    };
    helper_orphans_in(&std::env::temp_dir(), &stem)
}

#[cfg(windows)]
fn helper_orphans_in(dir: &std::path::Path, stem: &str) -> Vec<(std::path::PathBuf, u64)> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| env::is_self_replace_helper(name, stem))
        })
        .map(|entry| {
            let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
            (entry.path(), size)
        })
        .collect()
}

#[cfg(windows)]
fn ends_with_separator(path: &std::path::Path) -> bool {
    use std::os::windows::ffi::OsStrExt;

    path.as_os_str()
        .encode_wide()
        .last()
        .is_some_and(|c| c == u16::from(b'\\') || c == u16::from(b'/'))
}

/// The file stem `self-replace` would put in the helper's name: the running executable's.
#[cfg(windows)]
fn current_exe_stem() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    exe.file_stem().and_then(|s| s.to_str()).map(str::to_owned)
}

impl SelfUpdate {
    pub async fn run(self) -> Result<()> {
        if !Self::is_available() && !self.force {
            if let Some(instructions) = upgrade_instructions_text() {
                warn!("{}", instructions);
            }
            bail!("mise is installed via a package manager, cannot update");
        }
        // Before the update, not after: this run is about to create a copy of its own, and that one
        // is in use rather than stale. Before the length check too, and that ordering is the whole
        // point: a `TEMP` long enough to refuse the update is the case the leftovers come from, so
        // running the sweep afterwards means the only machines that accumulate them are the only
        // machines that never reach the code that collects them.
        #[cfg(windows)]
        sweep_helper_orphans();
        #[cfg(windows)]
        Self::ensure_temp_dir_can_replace_binary()?;
        let status = self.do_update()?;

        if status.updated() {
            let version = status.version().to_string();
            let styled_version = style(&version).bright().yellow();
            miseprintln!("Updated mise to {styled_version}");
            // On Windows, "exe"/"hardlink" shims are copies of mise-shim.exe and
            // go stale after an update. Refresh mise-shim.exe, and ONLY if that
            // succeeds rebuild the shim copies from it. Reshimming on failure
            // would re-copy the OLD mise-shim.exe yet still stamp the new version
            // in the `.version` marker, masking the staleness from future
            // (non-forced) reshims. Best-effort. See discussion #10022.
            #[cfg(windows)]
            match Self::update_mise_shim(&version).await {
                Ok(()) => {
                    if let Err(e) = Self::reshim_after_update().await {
                        warn!("Failed to reshim after self-update: {e}");
                    }
                }
                Err(e) => warn!("Failed to update mise-shim.exe: {e}"),
            }
        } else {
            miseprintln!("mise is already up to date");
        }
        if !self.no_plugins {
            cmd!(&*env::MISE_BIN, "plugins", "update").run()?;
        }

        Ok(())
    }

    /// Stop before anything is downloaded or moved when `TEMP` is long enough that
    /// replacing the binary would leave no mise installed at all.
    #[cfg(windows)]
    fn ensure_temp_dir_can_replace_binary() -> Result<()> {
        use std::os::windows::ffi::OsStrExt;

        let tmp = std::env::temp_dir();
        let stem = current_exe_stem();
        if !temp_dir_breaks_self_replace(&tmp, stem.as_deref()) {
            return Ok(());
        }
        let msg = formatdoc! {r#"
            TEMP is too long to replace mise.exe safely ({len} UTF-16 code units)

              TEMP = {tmp}

            Updating moves the running mise.exe aside and then launches a helper from TEMP to
            put the new one in place. That helper's path would be {helper} UTF-16 code units,
            and Windows cannot launch an executable whose path reaches {max}. The move happens
            first, so going ahead would leave no mise installed at all.

            Point TEMP and TMP at a shorter directory and run mise self-update again:

              $env:TEMP = 'C:\Temp'; $env:TMP = 'C:\Temp'"#,
            len = tmp.as_os_str().encode_wide().count(),
            tmp = tmp.display(),
            helper = helper_path_len(&tmp, stem.as_deref()),
            max = MAX_PATH,
        };
        bail!("{msg}");
    }

    fn do_update(&self) -> Result<Status> {
        // Use block_in_place to allow self_update's blocking HTTP calls
        // to work within mise's async runtime
        tokio::task::block_in_place(|| self.do_update_blocking())
    }

    fn do_update_blocking(&self) -> Result<Status> {
        let mut update = Update::configure();
        if let Some((token, _)) = crate::github::resolve_token("github.com") {
            update.auth_token(&token);
        }
        #[cfg(windows)]
        let bin_path_in_archive = "mise/bin/mise.exe";
        #[cfg(not(windows))]
        let bin_path_in_archive = "mise/bin/mise";
        update
            .repo_owner("jdx")
            .repo_name("mise")
            .bin_name("mise")
            .current_version(cargo_crate_version!())
            .bin_path_in_archive(bin_path_in_archive);

        let settings = Settings::try_get();
        let v = self
            .version
            .clone()
            .map_or_else(
                || -> Result<String> { Ok(update.build()?.get_latest_release()?.version) },
                Ok,
            )
            .map(|v| format!("v{v}"))?;

        // Check if already up to date (unless --force is specified)
        let current_version = format!("v{}", cargo_crate_version!());
        if !self.force && v == current_version {
            return Ok(Status::UpToDate(current_version));
        }

        let target = format!("{}-{}", *OS, *ARCH);
        #[cfg(target_env = "musl")]
        let target = format!("{target}-musl");
        // Always set target_version_tag to ensure we download the correct release
        // (fixes semver mismatch across year boundaries, e.g. 2025.x -> 2026.x)
        update.target_version_tag(&v);
        #[cfg(windows)]
        let target = format!("mise-{v}-{target}.zip");
        #[cfg(not(windows))]
        let target = format!("mise-{v}-{target}.tar.gz");
        let status = update
            .verifying_keys([*include_bytes!("../../zipsign.pub")])
            .show_download_progress(true)
            .target(&target)
            .no_confirm(settings.is_ok_and(|s| s.yes) || self.yes)
            .build()?
            .update()?;

        // Verify macOS binary signature after update
        #[cfg(target_os = "macos")]
        if status.updated() {
            Self::verify_macos_signature(&env::MISE_BIN)?;
        }

        Ok(status)
    }

    // Rebuild the Windows shim copies in-process instead of shelling out to
    // `mise reshim --force`. Mirrors `cli::reshim::Reshim::run`.
    #[cfg(windows)]
    async fn reshim_after_update() -> Result<()> {
        use crate::config::Config;
        use crate::toolset::ToolsetBuilder;

        let config = Config::get().await?;
        let ts = ToolsetBuilder::new().build(&config).await?;
        crate::shims::reshim(&config, &ts, true).await
    }

    #[cfg(windows)]
    async fn update_mise_shim(version: &str) -> Result<()> {
        use crate::http::HTTP;
        use std::io::Read;

        let version = version.strip_prefix('v').unwrap_or(version);
        let archive_name = format!("mise-v{version}-{}-{}.zip", *OS, *ARCH);
        let url =
            format!("https://github.com/jdx/mise/releases/download/v{version}/{archive_name}",);
        debug!("Downloading mise-shim.exe from {url}");

        let temp_dir = tempfile::tempdir()?;
        // Use the real archive name so zipsign context matches the release signature
        let zip_path = temp_dir.path().join(&archive_name);
        HTTP.download_file(&url, &zip_path, None).await?;

        // Verify the archive signature using the same key as the main update
        Self::verify_zip_signature(&zip_path)?;

        let file = fs::File::open(&zip_path)?;
        let mut archive = zip::ZipArchive::new(file)?;

        let mut shim_entry = match archive.by_name("mise/bin/mise-shim.exe") {
            Ok(entry) => entry,
            Err(_) => {
                warn!("mise-shim.exe not found in release archive, skipping");
                return Ok(());
            }
        };

        let dest = env::MISE_BIN
            .parent()
            .expect("MISE_BIN should have a parent directory")
            .join("mise-shim.exe");

        // Write to a temp file first, then rename for atomic replacement
        let mut buf = Vec::new();
        shim_entry.read_to_end(&mut buf)?;
        let temp_shim = temp_dir.path().join("mise-shim.exe");
        fs::write(&temp_shim, &buf)?;
        if fs::rename(&temp_shim, &dest).is_err() {
            // Fallback for cross-filesystem moves
            fs::copy(&temp_shim, &dest)?;
        }

        debug!("Updated mise-shim.exe at {}", dest.display());
        Ok(())
    }

    #[cfg(windows)]
    fn verify_zip_signature(path: &std::path::Path) -> Result<()> {
        let context = path
            .file_name()
            .and_then(|s| s.to_str())
            .map(|s| s.as_bytes())
            .ok_or_else(|| color_eyre::eyre::eyre!("non-UTF8 archive path"))?;

        let keys = zipsign_api::verify::collect_keys(
            [*include_bytes!("../../zipsign.pub")].into_iter().map(Ok),
        )
        .map_err(|e| color_eyre::eyre::eyre!("failed to load verification keys: {e}"))?;

        let mut file = fs::File::open(path)?;
        zipsign_api::verify::verify_zip(&mut file, &keys, Some(context))
            .map_err(|e| color_eyre::eyre::eyre!("zip signature verification failed: {e}"))?;

        debug!("Verified zip signature for {}", path.display());
        Ok(())
    }

    pub fn is_available() -> bool {
        if let Some(b) = *env::MISE_SELF_UPDATE_AVAILABLE {
            return b;
        }
        let has_disable = env::MISE_SELF_UPDATE_DISABLED_PATH.is_some();
        let has_instructions = env::MISE_SELF_UPDATE_INSTRUCTIONS.is_some();
        !(has_disable || has_instructions)
    }

    #[cfg(target_os = "macos")]
    fn verify_macos_signature(binary_path: &Path) -> Result<()> {
        use std::process::Command;

        debug!(
            "Verifying macOS code signature for: {}",
            binary_path.display()
        );

        // Check if codesign is available
        let codesign_check = Command::new("which").arg("codesign").output();

        if codesign_check.is_err() || !codesign_check.unwrap().status.success() {
            warn!("codesign command not found in PATH, skipping binary signature verification");
            warn!("This is unusual on macOS - consider verifying your system installation");
            return Ok(());
        }

        // Verify signature and identifier in one step using --test-requirement
        let output = Command::new("codesign")
            .args([
                "--verify",
                "--deep",
                "--strict",
                "-R=identifier \"dev.jdx.mise\"",
            ])
            .arg(binary_path)
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!(
                "macOS binary signature verification failed (invalid signature or incorrect identifier): {}",
                stderr.trim()
            );
        }

        debug!("macOS binary signature verified successfully");
        Ok(())
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    /// What `env::temp_dir()` hands back on Windows: a directory path of exactly `len`
    /// UTF-16 code units, trailing backslash included.
    fn temp_dir_of_len(len: usize) -> PathBuf {
        let mut s = String::from("C:\\");
        while s.len() < len - 1 {
            s.push('t');
        }
        s.push('\\');
        assert_eq!(s.len(), len, "test helper built the wrong length");
        PathBuf::from(s)
    }

    fn breaks(len: usize) -> bool {
        temp_dir_breaks_self_replace(&temp_dir_of_len(len), Some("mise"))
    }

    #[test]
    fn an_ordinary_temp_dir_is_left_alone() {
        let tmp = Path::new("C:\\Users\\u\\AppData\\Local\\Temp\\");
        assert!(!temp_dir_breaks_self_replace(tmp, Some("mise")));
    }

    #[test]
    fn the_boundary_matches_what_windows_actually_does() {
        // Measured on Windows 11 26200 with LongPathsEnabled=0, running self-update against
        // a copy of mise.exe: with TEMP at 201 it succeeds and the binary survives, at 202 it
        // fails with `os error 3` and the install directory is left empty. `temp_dir()`
        // appends a backslash to both, which is why these are 202 and 203 here.
        assert!(!breaks(202));
        assert!(breaks(203));
    }

    #[test]
    fn temp_dirs_measured_as_destructive_are_rejected() {
        assert!(breaks(206)); // TEMP=205
        assert!(breaks(244)); // TEMP=243
    }

    #[test]
    fn a_long_temp_dir_that_still_works_is_not_rejected() {
        // Control: length alone is not the trigger. TEMP=190 was measured as succeeding, so a
        // guard that fired here would block updates that work.
        assert!(!breaks(191));
    }

    #[test]
    fn a_trailing_separator_is_not_counted_twice() {
        // `env::temp_dir()` always ends in a separator on Windows and `Path::join` does not
        // add a second one, so both spellings of the same directory have to agree.
        assert_eq!(
            helper_path_len(Path::new("C:\\Temp\\"), Some("mise")),
            helper_path_len(Path::new("C:\\Temp"), Some("mise"))
        );
    }

    #[test]
    fn the_helper_name_follows_the_running_executable() {
        // `.` + stem + `.` + 32 random characters + `.__selfdelete__.exe`
        assert_eq!(helper_name_len(Some("mise")), 57);
        assert_eq!(helper_name_len(Some("mise-dev")), 61);
        // self-replace leaves the stem out when it is not valid UTF-8
        assert_eq!(helper_name_len(None), 52);
    }

    #[test]
    fn a_renamed_binary_lowers_the_ceiling() {
        // A TEMP that is safe for `mise.exe` is not safe once the binary has been renamed to
        // something longer, so the guard cannot assume the stem.
        let tmp = temp_dir_of_len(202);
        assert!(!temp_dir_breaks_self_replace(&tmp, Some("mise")));
        assert!(temp_dir_breaks_self_replace(&tmp, Some("mise-dev")));
    }

    /// The walk, not the predicate — `env::is_self_replace_helper` has its own tests. What matters
    /// here is that `doctor` and the sweep see the same set, and that a directory full of unrelated
    /// files does not turn into a warning about mise.
    #[test]
    fn only_the_generated_copies_are_collected() {
        let dir = tempfile::tempdir().unwrap();
        let rand = "a".repeat(env::SELF_REPLACE_RANDOM_LEN);
        let collected = [
            format!(".mise.{rand}.__selfdelete__.exe"),
            format!(".mise.{rand}.__relocated__.exe"),
        ];
        let ignored = [
            "mise.exe".to_string(),
            // a different binary's leftovers are not ours to delete
            format!(".other.{rand}.__selfdelete__.exe"),
            // near-misses on the random segment: too short, and not lowercase
            format!(".mise.{}.__selfdelete__.exe", "a".repeat(31)),
            format!(".mise.{}A.__selfdelete__.exe", "a".repeat(31)),
            "setup-x64.exe".to_string(),
        ];
        for name in collected.iter().chain(ignored.iter()) {
            std::fs::write(dir.path().join(name), b"xyz").unwrap();
        }

        let found = helper_orphans_in(dir.path(), "mise");
        let mut names = found
            .iter()
            .map(|(p, _)| p.file_name().unwrap().to_str().unwrap().to_string())
            .collect::<Vec<_>>();
        names.sort();
        let mut want = collected.to_vec();
        want.sort();
        assert_eq!(names, want);
        // the size is what `doctor` adds up, so it has to come from the files rather than a count
        assert_eq!(found.iter().map(|(_, size)| size).sum::<u64>(), 6);
    }

    #[test]
    fn a_missing_directory_is_not_an_error() {
        // `TEMP` pointing at something unreadable must not take `self-update` or `doctor` down.
        assert!(helper_orphans_in(Path::new("C:\\nope\\nope\\nope"), "mise").is_empty());
    }

    #[test]
    fn the_length_is_counted_in_utf16_code_units() {
        // Control against the `OsStr::len()` trap: this path is 202 UTF-16 code units, the
        // longest that works, but 598 WTF-8 bytes. Counting bytes would reject it.
        let mut s = String::from("C:\\");
        while s.chars().count() < 201 {
            s.push('あ');
        }
        s.push('\\');
        let tmp = PathBuf::from(s);
        assert_eq!(tmp.as_os_str().len(), 598);
        assert!(!temp_dir_breaks_self_replace(&tmp, Some("mise")));
    }
}
