use std::cmp::Ordering;
use std::io::{Read, Seek};

#[cfg(unix)]
use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum UpgradeDecision {
    Older { live: String, available: String },
    Equal { live: String, available: String },
    Newer { live: String, available: String },
    Unknown { reason: String },
}

fn unknown(reason: impl Into<String>) -> UpgradeDecision {
    UpgradeDecision::Unknown {
        reason: reason.into(),
    }
}

pub(super) fn compare_live_version(live: &str, available: &str) -> UpgradeDecision {
    let components = |version: &str| {
        version
            .split('.')
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
    };
    if !components(live)
        || !components(available)
        || live.split('.').count() != available.split('.').count()
    {
        return unknown(format!(
            "cannot compare live version {live:?} with cask {available:?}"
        ));
    }
    let order = live
        .split('.')
        .zip(available.split('.'))
        .map(|(live, available)| {
            let live = live.trim_start_matches('0');
            let available = available.trim_start_matches('0');
            live.len()
                .cmp(&available.len())
                .then_with(|| live.cmp(available))
        })
        .find(|order| *order != Ordering::Equal)
        .unwrap_or(Ordering::Equal);
    let live = live.to_owned();
    let available = available.to_owned();
    match order {
        Ordering::Less => UpgradeDecision::Older { live, available },
        Ordering::Equal => UpgradeDecision::Equal { live, available },
        Ordering::Greater => UpgradeDecision::Newer { live, available },
    }
}

pub(super) fn read_live_version_from(reader: impl Read + Seek, available: &str) -> UpgradeDecision {
    let value = match plist::Value::from_reader(reader) {
        Ok(value) => value,
        Err(error) if error.is_io() => {
            return unknown(format!("cannot read live version plist: {error}"));
        }
        Err(error) => return unknown(format!("invalid live version plist: {error}")),
    };
    let Some(value) = value
        .as_dictionary()
        .and_then(|dictionary| dictionary.get("CFBundleShortVersionString"))
    else {
        return unknown("cannot determine live version: missing CFBundleShortVersionString");
    };
    let Some(live) = value.as_string() else {
        return unknown(
            "cannot determine live version: CFBundleShortVersionString is not a string",
        );
    };
    compare_live_version(live, available)
}

#[cfg(unix)]
fn open_bundle_component(
    parent: &impl std::os::fd::AsFd,
    name: &std::ffi::OsStr,
    directory: bool,
) -> std::result::Result<std::os::fd::OwnedFd, String> {
    use nix::fcntl::{AtFlags, OFlag, openat};
    use nix::sys::stat::{Mode, SFlag, fstat, fstatat};

    let description = if directory && name != "Contents" {
        "app"
    } else {
        "plist"
    };
    let describe = |error: nix::errno::Errno| {
        if error == nix::errno::Errno::ENOENT {
            format!("missing {description}")
        } else if error == nix::errno::Errno::ELOOP {
            format!("cannot read {description}: symlink")
        } else {
            format!("cannot read {description}: {error}")
        }
    };
    let stat = fstatat(parent, name, AtFlags::AT_SYMLINK_NOFOLLOW).map_err(describe)?;
    if SFlag::from_bits_truncate(stat.st_mode).contains(SFlag::S_IFLNK) {
        return Err(format!("cannot read {description}: symlink"));
    }
    let mut flags = OFlag::O_RDONLY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC | OFlag::O_NONBLOCK;
    if directory {
        flags |= OFlag::O_DIRECTORY;
    }
    let fd = openat(parent, name, flags, Mode::empty()).map_err(describe)?;
    let stat = fstat(&fd).map_err(describe)?;
    let expected = if directory {
        SFlag::S_IFDIR
    } else {
        SFlag::S_IFREG
    };
    if !SFlag::from_bits_truncate(stat.st_mode).contains(expected) {
        return Err(format!("cannot read {description}: unexpected file type"));
    }
    Ok(fd)
}

#[cfg(unix)]
pub(super) fn read_live_version_at(
    parent: &TrustedOperationParent,
    app_name: &std::ffi::OsStr,
    available: &str,
) -> Result<UpgradeDecision> {
    let mut components = Path::new(app_name).components();
    if !matches!(components.next(), Some(Component::Normal(_))) || components.next().is_some() {
        bail!("brew-cask: app bundle name must be one normal path component");
    }
    let file = (|| {
        let app = open_bundle_component(&parent.fd, app_name, true)?;
        let contents = open_bundle_component(&app, std::ffi::OsStr::new("Contents"), true)?;
        open_bundle_component(&contents, std::ffi::OsStr::new("Info.plist"), false)
    })();
    Ok(match file {
        Ok(fd) => read_live_version_from(std::fs::File::from(fd), available),
        Err(reason) => unknown(reason),
    })
}

#[cfg(unix)]
fn supported_layout(cask: &Cask, artifacts: &CaskArtifacts, receipt: &CaskReceipt) -> bool {
    artifacts.apps.len() == 1
        && receipt.apps.len() == 1
        && artifacts.command_wrappers.is_empty()
        && artifacts.pkgs.is_empty()
        && artifacts.installers.is_empty()
        && artifacts.generic.is_empty()
        && artifacts.fonts.is_empty()
        && artifacts.generated_completions.is_empty()
        && artifacts.preflight_steps.is_empty()
        && artifacts.postflight_steps.is_empty()
        && !has_lifecycle_hook(cask, "preflight")
        && !has_lifecycle_hook(cask, "postflight")
        && receipt.pkg_ids.is_empty()
        && receipt.generic.is_empty()
        && receipt.fonts.is_empty()
        && receipt.flight_directories.is_empty()
        && receipt.prune_blocker.as_deref().is_none_or(|reason| {
            reason == "metadata-only app ownership cannot be proven safely during pruning"
        })
        && receipt.targets.iter().all(|record| {
            record.uninstall.is_none()
                && (receipt.apps.contains(&record.path)
                    || ((receipt.binaries.contains(&record.path)
                        || receipt.completions.contains(&record.path))
                        && record.fingerprint.kind == CaskTargetKind::Symlink))
        })
}

#[cfg(unix)]
pub(super) fn assess_auto_update(
    cask: &Cask,
    receipt: Option<&CaskReceipt>,
    installed: bool,
    homebrew_owned: bool,
) -> Result<UpgradeDecision> {
    if homebrew_owned {
        return Ok(unknown("managed by Homebrew"));
    }
    let artifacts = cask_artifacts(cask)?;
    let Some(receipt) = receipt else {
        return Ok(unknown("missing mise receipt"));
    };
    if !cask.auto_updates || !supported_layout(cask, &artifacts, receipt) {
        return Ok(unknown("unsupported auto-updating cask layout"));
    }
    let target = app_target_path(artifacts.apps[0].target_name())?;
    if receipt.apps[0] != target {
        return Ok(unknown(
            "app receipt target does not match current app target",
        ));
    }
    if !installed {
        return Ok(unknown("app is no longer installed"));
    }
    let parent = target
        .parent()
        .ok_or_else(|| eyre!("brew-cask: app target must have a parent"))?;
    let name = target
        .file_name()
        .ok_or_else(|| eyre!("brew-cask: app target must have a filename"))?;
    let parent = open_trusted_appdir_readonly(parent)?;
    read_live_version_at(&parent, name, &cask.version)
}

#[cfg(unix)]
pub(super) fn recheck_auto_update(
    cask: &Cask,
    initial_receipt: &CaskReceipt,
    parent: Option<&TrustedOperationParent>,
) -> Result<UpgradeDecision> {
    if homebrew_metadata_present(&cask.token)? {
        bail!(
            "brew-cask:{}: Homebrew took ownership of this cask while installation was pending",
            cask.token
        );
    }
    if previous_receipt(cask)?.as_ref() != Some(initial_receipt) {
        return Ok(unknown(
            "mise ownership receipt changed while upgrade was pending",
        ));
    }
    let [target] = initial_receipt.apps.as_slice() else {
        bail!("brew-cask: eligible upgrade must own exactly one app");
    };
    let name = target
        .file_name()
        .ok_or_else(|| eyre!("brew-cask: app target has no filename"))?;
    if let Some(parent) = parent {
        return read_live_version_at(parent, name, &cask.version);
    }
    let parent = open_trusted_appdir_readonly(
        target
            .parent()
            .ok_or_else(|| eyre!("brew-cask: app target has no parent"))?,
    )?;
    read_live_version_at(&parent, name, &cask.version)
}

#[cfg(unix)]
pub(super) fn validate_distribution(cask: &Cask, app: &AppArtifact, stage: &Path) -> Result<()> {
    let source = find_app(stage, &app.source)
        .ok_or_else(|| eyre!("brew-cask: app artifact '{}' was not found", app.source))?;
    let parent = open_trusted_appdir_readonly(
        source
            .parent()
            .ok_or_else(|| eyre!("brew-cask: staged app has no parent"))?,
    )?;
    let name = source
        .file_name()
        .ok_or_else(|| eyre!("brew-cask: staged app has no filename"))?;
    match read_live_version_at(&parent, name, &cask.version)? {
        UpgradeDecision::Equal { .. } => Ok(()),
        decision => bail!(
            "brew-cask:{}: distributed app version does not match cask {}: {decision:?}",
            cask.token,
            cask.version
        ),
    }
}
