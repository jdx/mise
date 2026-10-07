mod packslip;

use crate::github::GithubAsset as ReleaseAsset;
use color_eyre::Result;
use color_eyre::eyre::bail;
use console::style;
#[cfg(windows)]
use indoc::formatdoc;

use crate::cli::version::SelfUpdateSource;
use crate::config::{Settings, SettingsExt};
use crate::env;
#[cfg(windows)]
use crate::file::MAX_PATH;
use crate::platform::{ARCH, OS};
use std::ffi::OsStr;
#[cfg(windows)]
use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

const AUTO_UPDATE_REEXEC_ENV: &str = "__MISE_AUTO_UPDATE_REEXEC";

fn release_archive_name(version: &str, os: &str, arch: &str, build_target: &str) -> String {
    // Rust reports every 32-bit ARM target as `arm`, but our releases use `armv7`.
    // Preserve the build's ARM variant rather than upgrading an unsupported ARM CPU to v7.
    let arch = if arch == "arm" {
        build_target.split('-').next().unwrap_or(arch)
    } else {
        arch
    };
    let libc = if build_target.contains("-musl") {
        "-musl"
    } else {
        ""
    };
    let extension = if os == "windows" { "zip" } else { "tar.gz" };
    format!("mise-{version}-{os}-{arch}{libc}.{extension}")
}

fn release_archive_asset(assets: &[ReleaseAsset], archive_name: &str) -> Option<ReleaseAsset> {
    // The default matcher falls back to architecture/OS substrings when the requested
    // archive is missing, which can select a raw binary or another architecture.
    assets
        .iter()
        .find(|asset| asset.name == archive_name)
        .cloned()
}

pub(crate) use crate::upgrade_hint::{upgrade_instructions_or_hint, upgrade_instructions_text};

/// Checks for and installs an update before an eligible interactive command.
/// Failures are deliberately non-fatal so the requested command still runs.
pub(crate) async fn maybe_auto_update(
    args: &[String],
    original_cwd: Option<&std::path::Path>,
    command_eligible: bool,
) -> Result<()> {
    let Ok(settings) = Settings::try_get() else {
        return Ok(());
    };
    if !auto_update_eligible(AutoUpdateContext {
        enabled: settings.self_update.auto,
        offline: settings.offline(),
        prefer_offline: settings.prefer_offline(),
        ci: settings.ci || ci_info::is_ci(),
        attended: console::user_attended_stderr(),
        already_reexecuted: env::var_os(AUTO_UPDATE_REEXEC_ENV).is_some(),
        self_update_available: SelfUpdate::is_available(),
        command_eligible,
    }) {
        return Ok(());
    }

    let lock_path = crate::dirs::CACHE.join("auto-update");
    let update_lock = match crate::lock_file::LockFile::new(&lock_path).try_lock() {
        Ok(Some(lock)) => lock,
        Ok(None) => {
            debug!("skipping auto-update because another mise process is updating");
            return Ok(());
        }
        Err(err) => {
            debug!("automatic mise update could not acquire its lock: {err:#}");
            return Ok(());
        }
    };
    let last_check_path = crate::dirs::CACHE.join("auto-update-last-check");
    let check_duration = match settings.self_update_check_duration() {
        Ok(duration) => duration,
        Err(err) => {
            debug!("automatic mise update has an invalid check duration: {err:#}");
            return Ok(());
        }
    };
    if !auto_update_check_due(&last_check_path, check_duration) {
        return Ok(());
    }
    if let Err(err) = crate::file::write(&last_check_path, "") {
        debug!("automatic mise update could not record its check: {err:#}");
        return Ok(());
    }
    // The marker above is the auto-update throttle. Bypass the separate shared
    // version cache so a shorter-lived `mise version` lookup cannot make this
    // due check accept stale data and advance the marker for another interval.
    let Some(version) = crate::cli::version::check_for_new_version(Duration::ZERO).await else {
        return Ok(());
    };

    let update = SelfUpdate {
        version: Some(version),
        force: false,
        yes: true,
        no_plugins: true,
        minimum_release_age: None,
    };
    if let Err(err) = update.run_with_age_policy(true).await {
        debug!("automatic mise update failed: {err:#}");
        return Ok(());
    }
    drop(update_lock);
    reexec(args, original_cwd)
}

/// Returns whether the automatic-update attempt marker has expired.
fn auto_update_check_due(path: &std::path::Path, duration: Duration) -> bool {
    crate::file::modified_duration(path).map_or(true, |age| age >= duration)
}

/// Runtime conditions that gate automatic updates.
#[derive(Clone, Copy)]
struct AutoUpdateContext {
    enabled: bool,
    offline: bool,
    prefer_offline: bool,
    ci: bool,
    attended: bool,
    already_reexecuted: bool,
    self_update_available: bool,
    command_eligible: bool,
}

/// Applies the automatic-update safety policy without side effects.
fn auto_update_eligible(context: AutoUpdateContext) -> bool {
    context.enabled
        && !context.offline
        && !context.prefer_offline
        && !context.ci
        && context.attended
        && !context.already_reexecuted
        && context.self_update_available
        && context.command_eligible
}

/// Builds the replacement process with the original arguments, directory, and
/// a recursion guard shared by every platform-specific re-exec path.
fn build_reexec_command<I, S>(args: I, original_cwd: Option<&std::path::Path>) -> Command
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut command = Command::new(&*env::MISE_BIN);
    command.args(args).env(AUTO_UPDATE_REEXEC_ENV, "1");
    if let Some(cwd) = original_cwd {
        command.current_dir(cwd);
    }
    command
}

#[cfg(unix)]
fn reexec(_args: &[String], original_cwd: Option<&std::path::Path>) -> Result<()> {
    use std::os::unix::process::CommandExt;

    let mut command = build_reexec_command(std::env::args_os().skip(1), original_cwd);
    let err = command.exec();
    warn!("mise was updated but could not re-execute the command: {err}");
    Ok(())
}

#[cfg(windows)]
fn reexec(_args: &[String], original_cwd: Option<&std::path::Path>) -> Result<()> {
    let mut command = build_reexec_command(std::env::args_os().skip(1), original_cwd);
    let status = command.status()?;
    Err(crate::request_exit(status.code().unwrap_or(1)))
}

#[cfg(not(any(unix, windows)))]
fn reexec(args: &[String], original_cwd: Option<&std::path::Path>) -> Result<()> {
    let mut command = build_reexec_command(&args[1..], original_cwd);
    let status = command.status()?;
    Err(crate::request_exit(status.code().unwrap_or(1)))
}

/// Update mise itself
///
/// Selects the newest stable release satisfying the minimum release age (24h by default).
/// Explicit versions bypass the delay. Downloads binaries from GitHub Releases.
/// By default, this will also update any installed plugins.
/// Uses mise's GitHub token resolution chain for authenticated requests.
///
/// Packagers can disable this command so that mise is updated through the
/// package manager instead. See
/// https://mise.jdx.dev/contributing.html#packaging-and-self-update-instructions
#[derive(Debug, Default, usage_rs::Args)]
#[usage(verbatim_doc_comment)]
pub(crate) struct SelfUpdate {
    /// Update to a specific version
    version: Option<String>,

    /// Override the minimum release age for unpinned updates (default: 24h)
    #[usage(long)]
    minimum_release_age: Option<String>,

    /// Update even if already up to date
    #[usage(long, short)]
    force: bool,

    /// Skip confirmation prompt
    #[usage(long, short)]
    yes: bool,

    /// Disable auto-updating plugins
    #[usage(long)]
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

/// Refresh the installed plugins, best effort.
///
/// This runs after the binary has already been replaced, so a failure here says nothing about
/// whether the update worked. Propagating it made a successful update print `Updated mise to X`
/// and then exit non-zero, which reads as "the update failed" — on Windows that is a bad enough
/// misreading to send someone looking for a mise that is not broken. Warned about instead, the way
/// the two housekeeping steps above it already are.
///
/// The message names the step because the error often cannot. `duct` attaches the command only
/// when the child exits non-zero; a child that never starts comes back as the bare OS error, so an
/// `ACCESS_DENIED` from spawning the freshly written binary arrives as nothing but
/// "Access is denied. (os error 5)".
///
/// Takes the binary to run rather than reading [`env::MISE_BIN`] itself, so a test can drive the
/// failure without a real update.
fn update_plugins(bin: &std::path::Path) {
    if let Err(err) = cmd!(bin, "plugins", "update").run() {
        warn!("Failed to update plugins: {err}");
    }
}

/// Whether a failed write probe of the install directory means the update cannot proceed.
///
/// Only the errors that say "this user cannot write here" stop the update. Anything else -- a
/// directory that has gone missing, an exotic filesystem error -- is left to the update itself,
/// which is where the real operation and the error that describes it are.
fn write_probe_is_fatal(err: &std::io::Error) -> bool {
    matches!(
        err.kind(),
        std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::ReadOnlyFilesystem
    )
}

/// Create and remove a file in `dir`, the way replacing the binary is about to.
fn probe_install_dir(dir: &Path) -> std::io::Result<()> {
    tempfile::Builder::new()
        .prefix(".mise-self-update-probe")
        .tempfile_in(dir)
        .map(|_| ())
}

/// Whether a sticky directory stops this user from renaming a file in it.
///
/// On a sticky directory (`S_ISVTX`, the bit `/tmp` carries) creating a file is allowed but
/// renaming or removing one is restricted to the file's owner, the directory's owner, and root.
/// Replacing mise renames the *existing* binary out of the way, so a bindir that is group-writable
/// and sticky with a root-owned mise in it passes the write probe -- the probe's own file belongs
/// to the prober -- and then fails at the rename. Taking the four facts as arguments keeps the
/// rule testable without a second user to own the file.
#[cfg(unix)]
fn sticky_blocks_rename(dir_mode: u32, dir_uid: u32, file_uid: u32, euid: u32) -> bool {
    const S_ISVTX: u32 = 0o1000;

    dir_mode & S_ISVTX != 0 && euid != 0 && dir_uid != euid && file_uid != euid
}

/// [`sticky_blocks_rename`] against the real directory and binary. A stat that fails says nothing,
/// so it reports no obstruction and leaves the outcome to the update.
#[cfg(unix)]
fn sticky_dir_blocks_replacing(dir: &Path, exe: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;

    let (Ok(dir_meta), Ok(exe_meta)) = (std::fs::metadata(dir), std::fs::metadata(exe)) else {
        return false;
    };
    sticky_blocks_rename(
        dir_meta.mode(),
        dir_meta.uid(),
        exe_meta.uid(),
        nix::unistd::geteuid().as_raw(),
    )
}

/// Why the running binary cannot be replaced.
#[derive(Clone, Copy)]
enum Unreplaceable {
    /// The install directory refuses writes from this user.
    DirectoryNotWritable,
    /// The install directory is sticky and the binary belongs to someone else, so only its owner
    /// can rename it.
    #[cfg(unix)]
    StickyForeignBinary,
}

/// What to print when the install directory cannot be written to.
///
/// Names the binary as well as the directory: the install that is stuck is frequently not the one
/// the user thinks they are running -- a root-owned `/usr/local/bin/mise` shadowing a packaged
/// `/usr/bin/mise` produces exactly this failure, and the path is what gives that away.
fn install_dir_not_writable_message(exe: &Path, dir: &Path, why: Unreplaceable) -> String {
    let cause = match why {
        Unreplaceable::DirectoryNotWritable => {
            format!("{} is not writable by the current user", dir.display())
        }
        #[cfg(unix)]
        Unreplaceable::StickyForeignBinary => format!(
            "{} is sticky and {} belongs to another user, so only its owner can replace it",
            dir.display(),
            exe.display()
        ),
    };
    let elevate = if cfg!(windows) {
        "run mise self-update again from an elevated (Administrator) prompt"
    } else {
        "run `sudo mise self-update` to update this install"
    };
    let mut msg = format!(
        "cannot replace {exe}: {cause}\n\nEither {elevate}, or update mise the same way you installed it.",
        exe = exe.display(),
    );
    if let Some(instructions) = upgrade_instructions_text() {
        msg.push_str("\n\n");
        msg.push_str(&instructions);
    }
    msg
}

/// Stage a verified executable without touching the installed binary.
fn stage_update_binary(archive: &Path, dest: &Path, keys: &[[u8; 32]]) -> Result<()> {
    verify_update_archive(archive, keys)?;
    extract_update_binary(archive, dest)
}

/// Verify the embedded signature and archive filename before extracting anything.
fn verify_update_archive(path: &Path, keys: &[[u8; 32]]) -> Result<()> {
    eyre::ensure!(!keys.is_empty(), "self-update requires a verification key");
    let context = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| eyre::eyre!("non-UTF8 archive path"))?;
    let keys = zipsign_api::verify::collect_keys(keys.iter().copied().map(Ok))
        .map_err(|err| eyre::eyre!("invalid verification keys: {err}"))?;
    let mut archive = std::fs::File::open(path)?;
    if context.ends_with(".tar.gz") {
        zipsign_api::verify::verify_tar(&mut archive, &keys, Some(context.as_bytes()))
            .map_err(|err| eyre::eyre!("release signature verification failed: {err}"))?;
    } else if context.ends_with(".zip") {
        zipsign_api::verify::verify_zip(&mut archive, &keys, Some(context.as_bytes()))
            .map_err(|err| eyre::eyre!("release signature verification failed: {err}"))?;
    } else {
        bail!("unsupported self-update archive: {context}");
    }
    Ok(())
}

/// Extract only the expected regular executable to a fixed temporary path.
fn extract_update_binary(archive: &Path, dest: &Path) -> Result<()> {
    let file = std::fs::File::open(archive)?;
    if archive.extension().is_some_and(|ext| ext == "zip") {
        let mut archive = zip::ZipArchive::new(file)?;
        let mut entry = archive.by_name("mise/bin/mise.exe")?;
        eyre::ensure!(
            entry.is_file() && !entry.is_symlink(),
            "release executable is not a regular file"
        );
        std::io::copy(&mut entry, &mut std::fs::File::create(dest)?)?;
    } else {
        let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(file));
        for entry in archive.entries()? {
            let mut entry = entry?;
            if entry.path()? == Path::new("mise/bin/mise") {
                eyre::ensure!(
                    entry.header().entry_type().is_file(),
                    "release executable is not a regular file"
                );
                std::io::copy(&mut entry, &mut std::fs::File::create(dest)?)?;
                return Ok(());
            }
        }
        bail!("release archive has no mise/bin/mise executable");
    }
    Ok(())
}

// A fresh explicitly installed release must survive an unpinned update, even
// with --force. Explicit targets still allow intentional downgrades.
fn skip_selected_update(target: &str, current: &str, explicit: bool, force: bool) -> Result<bool> {
    Ok((!explicit
        && crate::cli::version::mise_release_key(target)?
            < crate::cli::version::mise_release_key(current)?)
        || (!force && target == current))
}

impl SelfUpdate {
    pub(crate) async fn run(self) -> Result<()> {
        let enforce_age = self.version.is_none();
        self.run_with_age_policy(enforce_age).await
    }

    async fn run_with_age_policy(self, enforce_age: bool) -> Result<()> {
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
        let version = self.do_update(enforce_age).await?;

        if let Some(version) = version {
            let styled_version = style(&version).bright().yellow();
            miseprintln!("Updated mise to {styled_version}");
            // On Windows, "exe"/"hardlink" shims are copies of mise-shim.exe and
            // go stale after an update. Refresh mise-shim.exe, and ONLY if that
            // succeeds rebuild the shim copies from it. Reshimming on failure
            // would re-copy the OLD mise-shim.exe yet still stamp the new version
            // in the `.version` marker, masking the staleness from future
            // (non-forced) reshims. Best-effort. See discussion #10022.
            #[cfg(windows)]
            match Self::update_mise_shim(&SelfUpdateSource::current(), &version).await {
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
        crate::cli::version::show_auto_update_hint();
        if !self.no_plugins {
            update_plugins(&env::MISE_BIN);
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

    /// Stop before anything is downloaded when the running binary cannot be replaced.
    ///
    /// `self-replace` renames the running mise out of its directory and writes the new binary in
    /// its place, so an update needs write permission on the *directory*, not on the file. A
    /// root-owned install being updated by a normal user -- `/usr/local/bin/mise`, an install
    /// under `/opt` -- therefore fails, but only after the release has been downloaded, and with
    /// a message that names a temp file nobody asked for and no directory at all:
    /// `Permission denied (os error 13) at path "/usr/local/bin/.mise.__temp__XKV5Oz"`. Probing
    /// first turns that into the two things the user needs: which install is stuck, and what to
    /// do about it.
    ///
    /// Deliberately after the up-to-date comparison, so a mise that has nothing to update still
    /// reports that rather than a permission problem it was never going to hit.
    fn ensure_install_dir_writable() -> Result<()> {
        let exe = std::env::current_exe().unwrap_or_else(|_| env::MISE_BIN.clone());
        let Some(dir) = exe.parent() else {
            return Ok(());
        };
        match probe_install_dir(dir) {
            // Writing to the directory is necessary but not sufficient: the update also renames
            // the binary that is already there, which a sticky directory reserves to its owner.
            Ok(()) => {
                #[cfg(unix)]
                if sticky_dir_blocks_replacing(dir, &exe) {
                    bail!(
                        "{}",
                        install_dir_not_writable_message(
                            &exe,
                            dir,
                            Unreplaceable::StickyForeignBinary
                        )
                    );
                }
                Ok(())
            }
            Err(err) if write_probe_is_fatal(&err) => {
                bail!(
                    "{}",
                    install_dir_not_writable_message(
                        &exe,
                        dir,
                        Unreplaceable::DirectoryNotWritable
                    )
                )
            }
            Err(err) => {
                debug!("could not probe {} for writability: {err}", dir.display());
                Ok(())
            }
        }
    }

    async fn do_update(&self, enforce_age: bool) -> Result<Option<String>> {
        let settings = Settings::get();
        let source = SelfUpdateSource::from_settings(&settings);
        source.validate()?;
        source.repository_parts()?;
        let explicit = self.version.is_some();
        let version = match &self.version {
            Some(version) => version.trim_start_matches('v').to_string(),
            None => {
                crate::cli::version::eligible_self_update_version(
                    &source,
                    self.minimum_release_age.as_deref(),
                )
                .await?
            }
        };
        if !explicit {
            miseprintln!(
                "Selected mise {version} (minimum release age: {})",
                crate::cli::version::self_update_release_age(self.minimum_release_age.as_deref())
            );
        }
        if skip_selected_update(&version, env!("CARGO_PKG_VERSION"), explicit, self.force)? {
            return Ok(None);
        }
        Self::ensure_install_dir_writable()?;
        let client = Self::http_client()?;
        let mut url = url::Url::parse(&format!(
            "{}/repos/{}/releases/tags/",
            source.api_url, source.repository
        ))?;
        url.path_segments_mut()
            .map_err(|()| eyre::eyre!("invalid release API URL"))?
            .pop_if_empty()
            .push(&format!("v{version}"));
        let release: crate::github::GithubRelease = client
            .get(url.clone())
            .headers(Self::request_headers(&source, url.as_str())?)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let manifest = Self::release_packslip(&client, &source, &release, &version).await?;
        let before = enforce_age
            .then(|| {
                crate::duration::parse_into_timestamp(
                    &crate::cli::version::self_update_release_age(
                        self.minimum_release_age.as_deref(),
                    ),
                )
            })
            .transpose()?;
        let archive_name = release_archive_name(
            &format!("v{version}"),
            &OS,
            &ARCH,
            crate::build_time::TARGET,
        );
        let asset = release_archive_asset(&release.assets, &archive_name)
            .ok_or_else(|| eyre::eyre!("release v{version} has no asset named {archive_name}"))?;
        if !(self.yes
            || settings.yes
            || crate::ui::prompt::confirm(format!("Update mise to {version}?"))?.is_yes())
        {
            bail!("self-update cancelled; use --yes to update non-interactively");
        }
        let dir = tempfile::tempdir()?;
        let archive_path = dir.path().join(&archive_name);
        Self::download_archive(&client, &source, &asset.url, &archive_path).await?;
        let binary = dir
            .path()
            .join(if cfg!(windows) { "mise.exe" } else { "mise" });
        let new_binary = binary.clone();
        let selected_version = version.clone();
        // Verification and archive I/O are blocking; no unverified contents are
        // extracted, and the running executable is untouched on any failure.
        tokio::task::spawn_blocking(move || -> Result<()> {
            if let Some(manifest) = manifest {
                packslip::verify(&manifest, &selected_version, &archive_path, before)?;
            }
            stage_update_binary(
                &archive_path,
                &new_binary,
                &[*include_bytes!("../../zipsign.pub")],
            )?;
            #[cfg(target_os = "macos")]
            Self::verify_macos_signature(&new_binary)?;
            self_replace::self_replace(&new_binary)?;
            Ok(())
        })
        .await??;
        Ok(Some(version))
    }

    fn request_headers(source: &SelfUpdateSource, url: &str) -> Result<reqwest::header::HeaderMap> {
        let mut headers = crate::github::get_headers(url)?;
        // A configured API may use any HTTPS host/path. Resolve its token
        // explicitly, but never send it to an asset on another origin.
        if url::Url::parse(url)?.origin() == url::Url::parse(&source.api_url)?.origin()
            && let Some(token) = crate::github::resolve_token_for_api_url(&source.api_url)
        {
            headers.insert(
                reqwest::header::AUTHORIZATION,
                crate::tokens::bearer_header("GitHub", &token)?,
            );
        }
        Ok(headers)
    }

    async fn release_packslip(
        client: &reqwest::Client,
        source: &SelfUpdateSource,
        release: &crate::github::GithubRelease,
        version: &str,
    ) -> Result<Option<String>> {
        match release
            .assets
            .iter()
            .find(|asset| asset.name == "packslip.sigstore.json")
        {
            Some(asset) => Ok(Some(
                Self::download_packslip(client, source, &asset.url).await?,
            )),
            None if packslip::required(version)? => {
                bail!("release v{version} is missing its required packslip.sigstore.json")
            }
            None => Ok(None),
        }
    }

    async fn download_packslip(
        client: &reqwest::Client,
        source: &SelfUpdateSource,
        url: &str,
    ) -> Result<String> {
        let mut headers = Self::request_headers(source, url)?;
        headers.insert(reqwest::header::ACCEPT, "application/octet-stream".parse()?);
        let mut response = client
            .get(url)
            .headers(headers)
            .send()
            .await?
            .error_for_status()?;
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            eyre::ensure!(
                bytes.len() + chunk.len() <= 4 * 1024 * 1024,
                "self-update packslip exceeds 4 MiB"
            );
            bytes.extend_from_slice(&chunk);
        }
        Ok(String::from_utf8(bytes)?)
    }

    fn http_client() -> Result<reqwest::Client> {
        let settings = Settings::get();
        Ok(reqwest::Client::builder()
            .user_agent(format!("mise/{}", env!("CARGO_PKG_VERSION")))
            .https_only(true)
            .redirect(Self::redirect_policy())
            .connect_timeout(settings.http_timeout())
            .read_timeout(settings.http_timeout())
            .timeout(settings.http_download_timeout())
            .no_gzip()
            .no_zstd()
            .build()?)
    }

    async fn download_archive(
        client: &reqwest::Client,
        source: &SelfUpdateSource,
        url: &str,
        path: &Path,
    ) -> Result<()> {
        use tokio::io::AsyncWriteExt;
        let mut headers = Self::request_headers(source, url)?;
        headers.insert(reqwest::header::ACCEPT, "application/octet-stream".parse()?);
        let mut response = client
            .get(url)
            .headers(headers)
            .send()
            .await?
            .error_for_status()?;
        let progress =
            crate::ui::multi_progress_report::MultiProgressReport::get().add("self-update");
        progress.set_message("downloading release".into());
        if let Some(length) = response.content_length() {
            progress.set_length(length);
        }
        let mut file = tokio::fs::File::create(path).await?;
        while let Some(chunk) = response.chunk().await? {
            file.write_all(&chunk).await?;
            progress.inc(chunk.len() as u64);
        }
        file.flush().await?;
        progress.finish();
        Ok(())
    }

    fn redirect_policy() -> reqwest::redirect::Policy {
        use reqwest::redirect::Policy;

        Policy::custom(|attempt| {
            if crate::http::is_https_downgrade(attempt.previous(), attempt.url()) {
                attempt.error(std::io::Error::other(
                    "refusing to redirect a self-update request from HTTPS to an insecure URL",
                ))
            } else {
                Policy::default().redirect(attempt)
            }
        })
    }

    // Rebuild the Windows shim copies in-process instead of shelling out to
    // `mise reshim --force`. Mirrors `cli::reshim::Reshim::run`.
    #[cfg(windows)]
    async fn reshim_after_update() -> Result<()> {
        use crate::config::Config;
        use crate::toolset::ToolsetBuilder;

        let config = Config::get().await?;
        let ts = ToolsetBuilder::new().build(&config).await?;
        crate::shims::reshim_for(&config, &ts, true, crate::shims::ShimScope::User).await?;
        let user_shims = crate::dirs::shims();
        let system_shims = crate::dirs::system_shims();
        if system_shims.is_dir() && !crate::file::storage_paths_eq(&user_shims, &system_shims) {
            crate::shims::reshim_for(&config, &ts, true, crate::shims::ShimScope::System).await?;
        }
        Ok(())
    }

    #[cfg(windows)]
    async fn update_mise_shim(source: &SelfUpdateSource, version: &str) -> Result<()> {
        use std::io::Read;

        source.validate()?;
        let version = version.strip_prefix('v').unwrap_or(version);
        let archive_name = format!("mise-v{version}-{}-{}.zip", *OS, *ARCH);
        let release = crate::github::get_release_for_url_with_versions_host(
            &source.api_url,
            &source.repository,
            &format!("v{version}"),
            false,
        )
        .await?;
        let asset = release
            .assets
            .iter()
            .find(|asset| asset.name == archive_name)
            .ok_or_else(|| {
                color_eyre::eyre::eyre!(
                    "release v{version} for {} has no asset named {archive_name}",
                    source.repository
                )
            })?;
        // Use the API asset endpoint directly so every redirect is governed by
        // the downgrade-rejecting client below. This also supports private releases.
        let url = asset.url.clone();
        debug!("Downloading mise-shim.exe from {url}");

        let temp_dir = tempfile::tempdir()?;
        // Use the real archive name so zipsign context matches the release signature
        let zip_path = temp_dir.path().join(&archive_name);
        let headers = crate::github::get_headers(&url)?;
        let settings = Settings::get();
        let request_timeout = settings.http_timeout();
        let archive = reqwest::Client::builder()
            .user_agent(format!("mise/{}", env!("CARGO_PKG_VERSION")))
            .https_only(true)
            .redirect(Self::redirect_policy())
            .connect_timeout(request_timeout)
            .read_timeout(request_timeout)
            .timeout(settings.http_download_timeout())
            .build()?
            .get(&url)
            .headers(headers)
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?;
        fs::write(&zip_path, archive)?;

        let manifest =
            Self::release_packslip(&Self::http_client()?, source, &release, version).await?;
        if let Some(manifest) = manifest {
            // The main executable already enforced age for this exact release.
            crate::file::run_blocking(|| packslip::verify(&manifest, version, &zip_path, None))?;
        }

        // Verify the archive signature using the same key as the main update
        verify_update_archive(&zip_path, &[*include_bytes!("../../zipsign.pub")])?;

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

    pub(crate) fn is_available() -> bool {
        crate::upgrade_hint::self_update_available()
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

#[cfg(test)]
mod release_asset_tests {
    use super::*;

    #[test]
    fn age_filter_never_implicitly_downgrades() {
        assert!(skip_selected_update("2026.9.15", "2026.9.16", false, false).unwrap());
        assert!(skip_selected_update("2026.9.15", "2026.9.16", false, true).unwrap());
        assert!(!skip_selected_update("2026.9.15", "2026.9.16", true, false).unwrap());
        assert!(skip_selected_update("2026.9.16", "2026.9.16", false, false).unwrap());
        assert!(!skip_selected_update("2026.9.16", "2026.9.16", false, true).unwrap());
        assert!(!skip_selected_update("2026.10.0", "2026.9.30", false, false).unwrap());
    }

    #[test]
    fn archive_names_match_release_platforms() {
        for (os, arch, build_target, platform) in [
            (
                "linux",
                "arm",
                "armv7-unknown-linux-gnueabihf",
                "linux-armv7.tar.gz",
            ),
            (
                "linux",
                "arm",
                "armv7-unknown-linux-musleabi",
                "linux-armv7-musl.tar.gz",
            ),
            (
                "linux",
                "arm",
                "arm-unknown-linux-gnueabi",
                "linux-arm.tar.gz",
            ),
            (
                "linux",
                "arm64",
                "aarch64-unknown-linux-gnu",
                "linux-arm64.tar.gz",
            ),
            (
                "linux",
                "arm64",
                "aarch64-unknown-linux-musl",
                "linux-arm64-musl.tar.gz",
            ),
            (
                "linux",
                "x64",
                "x86_64-unknown-linux-gnu",
                "linux-x64.tar.gz",
            ),
            (
                "linux",
                "x64",
                "x86_64-unknown-linux-musl",
                "linux-x64-musl.tar.gz",
            ),
            (
                "macos",
                "arm64",
                "aarch64-apple-darwin",
                "macos-arm64.tar.gz",
            ),
            ("macos", "x64", "x86_64-apple-darwin", "macos-x64.tar.gz"),
            (
                "windows",
                "arm64",
                "aarch64-pc-windows-msvc",
                "windows-arm64.zip",
            ),
            (
                "windows",
                "x64",
                "x86_64-pc-windows-msvc",
                "windows-x64.zip",
            ),
        ] {
            assert_eq!(
                release_archive_name("v2026.9.3", os, arch, build_target),
                format!("mise-v2026.9.3-{platform}"),
                "{build_target}",
            );
        }
    }

    fn release_with_assets(names: &[&str]) -> Vec<ReleaseAsset> {
        names
            .iter()
            .map(|name| ReleaseAsset {
                name: (*name).into(),
                url: String::new(),
                browser_download_url: String::new(),
                digest: None,
                updated_at: None,
                from_versions_host: false,
            })
            .collect()
    }

    #[test]
    fn armv7_selects_its_archive_with_arm64_and_raw_binaries_present() {
        let release = release_with_assets(&[
            "mise-v2026.9.3-linux-arm64",
            "mise-v2026.9.3-linux-arm64.tar.gz",
            "mise-v2026.9.3-linux-armv7",
            "mise-v2026.9.3-linux-armv7-musl.tar.gz",
            "mise-v2026.9.3-linux-armv7.tar.gz",
        ]);
        for build_target in [
            "armv7-unknown-linux-gnueabihf",
            "armv7-unknown-linux-musleabi",
        ] {
            let name = release_archive_name("v2026.9.3", "linux", "arm", build_target);
            let asset = release_archive_asset(&release, &name).unwrap();
            assert_eq!(asset.name, name);
        }
    }

    #[test]
    fn missing_archive_does_not_fall_back_to_other_assets() {
        let release = release_with_assets(&[
            "mise-v2026.9.3-linux-arm64",
            "mise-v2026.9.3-linux-arm64.tar.gz",
            "mise-v2026.9.3-linux-armv7",
            "mise-v2026.9.3-linux-armv7-musl.tar.gz",
            "mise-v2026.9.3-linux-armv7.tar.xz",
            "mise-v2026.9.3-linux-armv7.tar.gz.sig",
            "mise-v2026.9.2-linux-armv7.tar.gz",
        ]);
        let name =
            release_archive_name("v2026.9.3", "linux", "arm", "armv7-unknown-linux-gnueabihf");
        assert!(release_archive_asset(&release, &name).is_none());
    }
}

#[cfg(test)]
mod archive_tests {
    use super::*;
    use std::io::{Cursor, Write};

    fn signed_archive(dir: &Path, zip: bool) -> (std::path::PathBuf, [u8; 32]) {
        let key = zipsign_api::SigningKey::from_bytes(&[7; 32]);
        let public_key = key.verifying_key().to_bytes();
        let path = dir.join(if zip {
            "mise-test.zip"
        } else {
            "mise-test.tar.gz"
        });
        let mut output = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&path)
            .unwrap();
        if zip {
            let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
            archive
                .start_file(
                    "mise/bin/mise.exe",
                    zip::write::SimpleFileOptions::default(),
                )
                .unwrap();
            archive.write_all(b"new executable").unwrap();
            let mut input = archive.finish().unwrap();
            zipsign_api::sign::copy_and_sign_zip(
                &mut input,
                &mut output,
                &[key],
                Some(b"mise-test.zip"),
            )
            .unwrap();
        } else {
            let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
            let mut archive = tar::Builder::new(encoder);
            let mut header = tar::Header::new_gnu();
            header.set_size(14);
            header.set_mode(0o755);
            header.set_cksum();
            archive
                .append_data(&mut header, "mise/bin/mise", &b"new executable"[..])
                .unwrap();
            let bytes = archive.into_inner().unwrap().finish().unwrap();
            zipsign_api::sign::copy_and_sign_tar(
                &mut Cursor::new(bytes),
                &mut output,
                &[key],
                Some(b"mise-test.tar.gz"),
            )
            .unwrap();
        }
        (path, public_key)
    }

    #[test]
    fn signed_tar_and_zip_extract_only_the_executable() {
        for zip in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let (archive, key) = signed_archive(dir.path(), zip);
            let dest = dir.path().join("staged-binary");
            stage_update_binary(&archive, &dest, &[key]).unwrap();
            assert_eq!(std::fs::read(&dest).unwrap(), b"new executable");
            assert!(!dir.path().join("mise").exists());
        }
    }

    #[test]
    fn invalid_signatures_cannot_overwrite_the_destination() {
        for zip in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let (archive, key) = signed_archive(dir.path(), zip);
            let dest = dir.path().join("existing-binary");
            std::fs::write(&dest, b"existing executable").unwrap();
            let wrong_key = zipsign_api::SigningKey::from_bytes(&[8; 32])
                .verifying_key()
                .to_bytes();
            assert!(stage_update_binary(&archive, &dest, &[wrong_key]).is_err());
            assert!(stage_update_binary(&archive, &dest, &[]).is_err());
            let renamed = dir
                .path()
                .join(if zip { "other.zip" } else { "other.tar.gz" });
            std::fs::copy(&archive, &renamed).unwrap();
            assert!(stage_update_binary(&renamed, &dest, &[key]).is_err());
            let mut bytes = std::fs::read(&archive).unwrap();
            bytes[20] ^= 1;
            std::fs::write(&archive, bytes).unwrap();
            assert!(stage_update_binary(&archive, &dest, &[key]).is_err());
            assert_eq!(std::fs::read(&dest).unwrap(), b"existing executable");
        }
    }

    #[test]
    fn tar_executable_must_be_a_regular_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("symlink.tar.gz");
        let encoder = flate2::write::GzEncoder::new(
            std::fs::File::create(&path).unwrap(),
            flate2::Compression::default(),
        );
        let mut archive = tar::Builder::new(encoder);
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Symlink);
        header.set_size(0);
        header.set_mode(0o755);
        archive
            .append_link(&mut header, "mise/bin/mise", "../../outside")
            .unwrap();
        archive.into_inner().unwrap().finish().unwrap();
        let dest = dir.path().join("staged-binary");
        assert!(extract_update_binary(&path, &dest).is_err());
        assert!(!dest.exists());
    }

    #[tokio::test]
    async fn self_update_transport_requires_https() {
        assert!(
            SelfUpdate::http_client()
                .unwrap()
                .get("http://127.0.0.1:1/release")
                .send()
                .await
                .is_err()
        );
    }
}

#[cfg(test)]
mod auto_update_tests {
    use super::*;

    fn eligible_context() -> AutoUpdateContext {
        AutoUpdateContext {
            enabled: true,
            offline: false,
            prefer_offline: false,
            ci: false,
            attended: true,
            already_reexecuted: false,
            self_update_available: true,
            command_eligible: true,
        }
    }

    #[test]
    fn eligible_interactive_command_updates() {
        assert!(auto_update_eligible(eligible_context()));
    }

    #[test]
    fn safety_conditions_disable_auto_update() {
        let context = eligible_context();
        for ineligible in [
            AutoUpdateContext {
                enabled: false,
                ..context
            },
            AutoUpdateContext {
                offline: true,
                ..context
            },
            AutoUpdateContext {
                prefer_offline: true,
                ..context
            },
            AutoUpdateContext {
                ci: true,
                ..context
            },
            AutoUpdateContext {
                attended: false,
                ..context
            },
            AutoUpdateContext {
                already_reexecuted: true,
                ..context
            },
            AutoUpdateContext {
                self_update_available: false,
                ..context
            },
        ] {
            assert!(!auto_update_eligible(ineligible));
        }
    }

    #[test]
    fn ineligible_commands_do_not_update() {
        assert!(!auto_update_eligible(AutoUpdateContext {
            command_eligible: false,
            ..eligible_context()
        }));
    }

    #[test]
    fn reexec_preserves_arguments_directory_and_guard() {
        use std::ffi::OsString;

        let cwd = std::path::Path::new("a directory");
        let args = [OsString::from("install"), OsString::from("node@22 beta")];
        let command = build_reexec_command(&args, Some(cwd));

        assert_eq!(command.get_args().collect::<Vec<_>>(), args);
        assert_eq!(command.get_current_dir(), Some(cwd));
        assert!(command.get_envs().any(|(key, value)| {
            key == AUTO_UPDATE_REEXEC_ENV && value == Some(OsStr::new("1"))
        }));
    }

    #[test]
    fn automatic_update_attempts_are_throttled() {
        let temp = tempfile::tempdir().unwrap();
        let marker = temp.path().join("last-check");
        assert!(auto_update_check_due(&marker, Duration::from_secs(60)));
        std::fs::write(&marker, "").unwrap();
        assert!(!auto_update_check_due(&marker, Duration::from_secs(60)));
        assert!(auto_update_check_due(&marker, Duration::ZERO));
    }
}

#[cfg(test)]
mod post_update_tests {
    use super::*;

    /// By the time plugins are refreshed the new binary is already in place, so a failure there
    /// must not turn a successful update into a failed command. Driving a real spawn failure
    /// rather than a stub: a binary that cannot be started is what Windows produces while an AV
    /// scanner still holds the file mise just wrote, and what discussion #8827 produces over SSH.
    #[test]
    fn a_plugins_update_that_cannot_run_is_not_fatal() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("mise-that-is-not-there");

        // Control: without this the test would pass just as well on a spawn that quietly
        // succeeded, and prove nothing. `run()` has to actually fail for the line below to mean
        // anything.
        assert!(cmd!(&missing, "plugins", "update").run().is_err());

        // And the step swallows it. There is no error here to propagate — which is exactly what
        // the `?` this replaces used to do.
        update_plugins(&missing);
    }
}

#[cfg(test)]
mod install_dir_tests {
    use super::*;

    #[test]
    fn a_writable_directory_probes_clean() {
        let dir = tempfile::tempdir().unwrap();
        probe_install_dir(dir.path()).unwrap();
        // The probe cleans up after itself; an update that then fails must not leave a 40MB
        // artifact of its own behind either, so the directory has to come back empty.
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn only_a_permission_failure_stops_the_update() {
        use std::io::{Error, ErrorKind};

        assert!(write_probe_is_fatal(&Error::from(
            ErrorKind::PermissionDenied
        )));
        assert!(write_probe_is_fatal(&Error::from(
            ErrorKind::ReadOnlyFilesystem
        )));
        // Everything else belongs to the update itself, which reports the operation that really
        // failed rather than a probe standing in for it.
        assert!(!write_probe_is_fatal(&Error::from(ErrorKind::NotFound)));
        assert!(!write_probe_is_fatal(&Error::other("something else")));
    }

    #[cfg(unix)]
    #[test]
    fn an_unwritable_directory_is_a_permission_failure() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("bin");
        std::fs::create_dir(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();

        match probe_install_dir(&dir) {
            // Root ignores the mode bits, so under a root test runner there is nothing to assert.
            Ok(()) => assert_eq!(nix::unistd::geteuid().as_raw(), 0),
            Err(err) => assert!(write_probe_is_fatal(&err), "{err:?}"),
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_sticky_directory_reserves_renaming_to_the_owner() {
        const STICKY: u32 = 0o41777;
        const PLAIN: u32 = 0o40755;

        // The shape the write probe cannot see: a group-writable, sticky bindir holding a
        // root-owned mise. Creating a file there works; renaming root's does not.
        assert!(sticky_blocks_rename(STICKY, 0, 0, 1000));

        // Not sticky: ordinary directory permissions decide, and the probe already covered them.
        assert!(!sticky_blocks_rename(PLAIN, 0, 0, 1000));
        // The sticky rule exempts the file's owner, the directory's owner, and root.
        assert!(!sticky_blocks_rename(STICKY, 0, 1000, 1000));
        assert!(!sticky_blocks_rename(STICKY, 1000, 0, 1000));
        assert!(!sticky_blocks_rename(STICKY, 0, 0, 0));
    }

    #[cfg(unix)]
    #[test]
    fn a_users_own_binary_in_a_sticky_directory_is_replaceable() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o1777)).unwrap();
        let exe = dir.path().join("mise");
        std::fs::write(&exe, b"#!/bin/sh\n").unwrap();

        // Both the file and the temporary directory belong to the test runner, so the sticky bit
        // takes nothing away here -- the check must not fire merely because the bit is set.
        assert!(!sticky_dir_blocks_replacing(dir.path(), &exe));
    }

    #[test]
    fn the_message_names_the_binary_and_its_directory() {
        let msg = install_dir_not_writable_message(
            Path::new("/usr/local/bin/mise"),
            Path::new("/usr/local/bin"),
            Unreplaceable::DirectoryNotWritable,
        );
        // The binary is the part that identifies which of several mise installs is stuck.
        assert!(msg.contains("/usr/local/bin/mise"), "{msg}");
        assert!(msg.contains("/usr/local/bin is not writable"), "{msg}");
        if cfg!(windows) {
            assert!(msg.contains("Administrator"), "{msg}");
        } else {
            assert!(msg.contains("sudo mise self-update"), "{msg}");
        }
        assert!(msg.contains("the same way you installed it"), "{msg}");
    }

    #[cfg(unix)]
    #[test]
    fn the_sticky_message_says_why_the_binary_cannot_be_replaced() {
        let msg = install_dir_not_writable_message(
            Path::new("/usr/local/bin/mise"),
            Path::new("/usr/local/bin"),
            Unreplaceable::StickyForeignBinary,
        );
        assert!(msg.contains("/usr/local/bin is sticky"), "{msg}");
        assert!(msg.contains("belongs to another user"), "{msg}");
        // Still writable, so the directory wording from the other case would be wrong here.
        assert!(!msg.contains("not writable"), "{msg}");
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
