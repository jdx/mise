use std::collections::{BTreeMap, HashMap};
use std::process::Stdio;

use async_trait::async_trait;
use eyre::bail;

use super::{InstallOpts, PackageRequest, PackageState, PackageStatus, SystemPackageManager};
use crate::options::ManagerPackageOptions;
use eyre::Result;
use mise_ui::sudo;

/// Snap risk levels. A channel's first segment is a risk rather than a track
/// when it is one of these, as in `beta` or `edge/fix-123`.
const RISKS: [&str; 4] = ["stable", "candidate", "beta", "edge"];

/// Per-snap options from a table-form `[bootstrap.packages]` entry.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SnapOptions {
    /// Channel to install from and track, as written in the config.
    pub channel: Option<String>,
    /// Install with classic confinement.
    pub classic: bool,
}

impl SnapOptions {
    /// Single-snap flags: snap refuses `--channel` and `--classic` when a
    /// command names more than one snap.
    fn flags(&self) -> Vec<String> {
        let mut flags = vec![];
        if let Some(channel) = &self.channel {
            flags.push(format!("--channel={channel}"));
        }
        if self.classic {
            flags.push("--classic".to_string());
        }
        flags
    }
}

/// Snaps from the Snap Store, installed system-wide through snapd.
pub(crate) struct SnapManager;

/// Check a snap name, with an optional `_<instance-key>` suffix, against the
/// store's naming rules. This also keeps a name from reaching `snap` as a flag.
pub fn validate_name(name: &str) -> eyre::Result<()> {
    let (snap, instance) = match name.split_once('_') {
        Some((snap, instance)) => (snap, Some(instance)),
        None => (name, None),
    };
    let valid_snap = (1..=40).contains(&snap.len())
        && snap
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && snap.bytes().any(|b| b.is_ascii_lowercase())
        && !snap.starts_with('-')
        && !snap.ends_with('-')
        && !snap.contains("--");
    let valid_instance = instance.is_none_or(|key| {
        (1..=10).contains(&key.len())
            && key
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
    });
    if !valid_snap || !valid_instance {
        bail!(
            "invalid snap name '{name}': use lowercase letters, digits, and hyphens, such as \"snap:node\""
        );
    }
    Ok(())
}

/// Check a channel's shape: `[<track>/]<risk>[/<branch>]`, or a bare track.
/// A misspelled risk is caught here, so the entry is skipped with a warning
/// rather than failing inside snap. A bare track cannot be checked: track
/// names are the publisher's.
pub fn validate_channel(name: &str, channel: &str) -> eyre::Result<()> {
    let segments = channel.split('/').collect::<Vec<_>>();
    let well_formed = segments.iter().all(|segment| {
        !segment.is_empty()
            && !segment.starts_with('-')
            && segment
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "-._".contains(c))
    });
    let has_risk = match segments.as_slice() {
        [_] => true,
        [first, second] => RISKS.contains(first) || RISKS.contains(second),
        [_, risk, _] => RISKS.contains(risk),
        _ => false,
    };
    let valid = well_formed && has_risk;
    if !valid {
        bail!(
            "snap:{name}: invalid channel '{channel}'; use <track>/<risk>, such as \"latest/stable\" or \"22/edge\", where the risk is stable, candidate, beta, or edge"
        );
    }
    Ok(())
}

/// Expand a channel to the form that `snap list` prints under Tracking, so
/// that `stable`, `latest`, and `latest/stable` all compare equal. snap list
/// prints a branch as `…` (`22/edge/fix-1` is `22/edge/…`), so a snap on
/// another branch of the same track and risk is not reported as drift.
fn listed_channel(channel: &str) -> String {
    let segments = channel.split('/').collect::<Vec<_>>();
    match segments.as_slice() {
        [risk] if RISKS.contains(risk) => format!("latest/{risk}"),
        [track] => format!("{track}/stable"),
        [risk, _branch] if RISKS.contains(risk) => format!("latest/{risk}/…"),
        [track, risk, _branch] => format!("{track}/{risk}/…"),
        _ => channel.to_string(),
    }
}

/// An installed snap's version and tracked channel, from `snap list`.
struct InstalledSnap<'a> {
    version: &'a str,
    tracking: &'a str,
}

/// Parse `snap list`: `Name Version Rev Tracking Publisher Notes`. A snap
/// installed from a local file tracks no channel and shows `-`.
fn parse_snap_list(output: &str) -> HashMap<&str, InstalledSnap<'_>> {
    output
        .lines()
        .skip_while(|line| !line.starts_with("Name "))
        .skip(1)
        .filter_map(|line| {
            let mut columns = line.split_whitespace();
            let name = columns.next()?;
            let version = columns.next()?;
            let _revision = columns.next()?;
            let tracking = columns.next()?;
            Some((name, InstalledSnap { version, tracking }))
        })
        .collect()
}

fn snap_options<'a>(options: &'a ManagerPackageOptions, name: &str) -> Option<&'a SnapOptions> {
    match options {
        ManagerPackageOptions::Snap { packages } => packages.get(name),
        _ => None,
    }
}

fn statuses(
    installed: &HashMap<&str, InstalledSnap<'_>>,
    requests: &[PackageRequest],
    options: &ManagerPackageOptions,
) -> Vec<PackageStatus> {
    requests
        .iter()
        .map(|request| {
            let channel = snap_options(options, &request.name)
                .and_then(|options| options.channel.as_deref())
                .map(listed_channel);
            let state = match installed.get(request.name.as_str()) {
                None => PackageState::Missing,
                Some(snap) => match &channel {
                    Some(channel) if channel != snap.tracking => PackageState::VersionMismatch {
                        installed: format!("{} from {}", snap.version, snap.tracking),
                    },
                    _ => match &request.version {
                        Some(version) if version != snap.version => PackageState::VersionMismatch {
                            installed: snap.version.to_string(),
                        },
                        _ => PackageState::Installed {
                            version: snap.version.to_string(),
                        },
                    },
                },
            };
            PackageStatus {
                request: request.clone(),
                state,
                display_name: None,
            }
        })
        .collect()
}

/// Read installed snaps. `snap list` talks to snapd without privileges.
async fn snap_list() -> Result<String> {
    debug!("$ snap list");
    let output = tokio::process::Command::new("snap")
        .arg("list")
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("snap list failed: {}", stderr.trim());
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// The `snap` commands that bring `pkgs` to their declared state. `installed`
/// separates the snaps to install from the ones to move to another channel.
/// Snaps without options share one command; snap takes flags for one snap at
/// a time.
fn commands(
    command: &str,
    pkgs: &[PackageRequest],
    installed: &HashMap<&str, InstalledSnap<'_>>,
    options: &ManagerPackageOptions,
) -> Vec<Vec<String>> {
    let mut batch: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    let mut single = vec![];
    for pkg in pkgs {
        let action = match command {
            "install" if installed.contains_key(pkg.name.as_str()) => "refresh",
            command => command,
        };
        match snap_options(options, &pkg.name).map(SnapOptions::flags) {
            Some(flags) if !flags.is_empty() => {
                let mut args = vec![action.to_string()];
                args.extend(flags);
                args.push(pkg.name.clone());
                single.push(args);
            }
            _ => batch.entry(action).or_default().push(pkg.name.clone()),
        }
    }
    let mut commands = batch
        .into_iter()
        .map(|(action, names)| {
            let mut args = vec![action.to_string()];
            args.extend(names);
            args
        })
        .collect::<Vec<_>>();
    commands.extend(single);
    commands
}

fn run(commands: &[Vec<String>], opts: &InstallOpts) -> Result<()> {
    for args in commands {
        if opts.dry_run {
            miseprintln!("{}", sudo::argv("snap", args).join(" "));
        } else {
            sudo::run("snap", args, &[])?;
        }
    }
    Ok(())
}

fn reject_pins(pkgs: &[PackageRequest], action: &str) -> Result<()> {
    if let Some(pkg) = pkgs.iter().find(|pkg| pkg.version.is_some()) {
        bail!("snap cannot {action} a pinned version ('{pkg}'); declare a channel instead");
    }
    Ok(())
}

#[async_trait(?Send)]
impl SystemPackageManager for SnapManager {
    fn name(&self) -> &str {
        "snap"
    }

    fn is_available(&self) -> bool {
        cfg!(target_os = "linux") && mise_util::file::which("snap").is_some()
    }

    fn unavailable_reason(&self) -> String {
        if cfg!(target_os = "linux") {
            "snap not found".to_string()
        } else {
            "only available on linux".to_string()
        }
    }

    fn supports_version_pins(&self) -> bool {
        false
    }

    async fn installed(&self, pkgs: &[PackageRequest]) -> Result<Vec<PackageStatus>> {
        self.installed_with_options(pkgs, &ManagerPackageOptions::None)
            .await
    }

    async fn installed_with_options(
        &self,
        pkgs: &[PackageRequest],
        manager_options: &ManagerPackageOptions,
    ) -> Result<Vec<PackageStatus>> {
        if pkgs.is_empty() {
            return Ok(vec![]);
        }
        let output = snap_list().await?;
        Ok(statuses(&parse_snap_list(&output), pkgs, manager_options))
    }

    async fn install(&self, pkgs: &[PackageRequest], opts: &InstallOpts) -> Result<()> {
        self.install_with_options(pkgs, opts, &ManagerPackageOptions::None)
            .await
    }

    /// Install missing snaps, and move installed ones whose tracked channel
    /// differs from the declared one with `snap refresh --channel`.
    async fn install_with_options(
        &self,
        pkgs: &[PackageRequest],
        opts: &InstallOpts,
        manager_options: &ManagerPackageOptions,
    ) -> Result<()> {
        if pkgs.is_empty() {
            return Ok(());
        }
        reject_pins(pkgs, "install")?;
        let output = snap_list().await?;
        let installed = parse_snap_list(&output);
        run(
            &commands("install", pkgs, &installed, manager_options),
            opts,
        )
    }

    async fn upgrade(&self, pkgs: &[PackageRequest], opts: &InstallOpts) -> Result<()> {
        self.upgrade_with_options(pkgs, opts, &ManagerPackageOptions::None)
            .await
    }

    /// Refresh installed snaps now instead of waiting for snapd's schedule.
    /// A declared channel goes with the refresh, so it also corrects drift.
    async fn upgrade_with_options(
        &self,
        pkgs: &[PackageRequest],
        opts: &InstallOpts,
        manager_options: &ManagerPackageOptions,
    ) -> Result<()> {
        if pkgs.is_empty() {
            return Ok(());
        }
        reject_pins(pkgs, "upgrade")?;
        run(
            &commands("refresh", pkgs, &HashMap::new(), manager_options),
            opts,
        )
    }

    fn supports_remove(&self) -> bool {
        true
    }

    async fn remove(&self, pkgs: &[PackageRequest], opts: &InstallOpts) -> Result<()> {
        if pkgs.is_empty() {
            return Ok(());
        }
        run(
            &commands(
                "remove",
                pkgs,
                &HashMap::new(),
                &ManagerPackageOptions::None,
            ),
            opts,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SNAP_LIST: &str = "\
Name      Version          Rev    Tracking         Publisher   Notes
code      1.95.3           178    latest/stable    vscode**    classic
core22    20241001         1663   latest/stable    canonical** base
node      22.11.0          9257   22/stable        iojs**      classic
hello     2.10             42     -                -           -
";

    fn req(name: &str) -> PackageRequest {
        PackageRequest {
            name: name.to_string(),
            version: None,
            tap_url: None,
            desired: crate::PackageDesiredState::Present,
        }
    }

    fn options(entries: &[(&str, Option<&str>, bool)]) -> ManagerPackageOptions {
        ManagerPackageOptions::Snap {
            packages: entries
                .iter()
                .map(|(name, channel, classic)| {
                    (
                        name.to_string(),
                        SnapOptions {
                            channel: channel.map(str::to_string),
                            classic: *classic,
                        },
                    )
                })
                .collect(),
        }
    }

    #[test]
    fn test_parse_snap_list() {
        let installed = parse_snap_list(SNAP_LIST);
        assert_eq!(installed.len(), 4);
        assert_eq!(installed["code"].version, "1.95.3");
        assert_eq!(installed["node"].tracking, "22/stable");
        assert_eq!(installed["hello"].tracking, "-");
        assert!(parse_snap_list("").is_empty());
    }

    #[test]
    fn test_listed_channel() {
        assert_eq!(listed_channel("stable"), "latest/stable");
        assert_eq!(listed_channel("edge"), "latest/edge");
        assert_eq!(listed_channel("22"), "22/stable");
        assert_eq!(listed_channel("22/beta"), "22/beta");
        assert_eq!(listed_channel("beta/fix-1"), "latest/beta/…");
        assert_eq!(listed_channel("latest/stable"), "latest/stable");
        assert_eq!(listed_channel("22/edge/fix-1"), "22/edge/…");
    }

    #[test]
    fn test_branch_channels_match_snap_list() {
        let installed = parse_snap_list(
            "Name  Version  Rev  Tracking   Publisher  Notes\nnode  23.0.0   9300 22/edge/…  iojs**     classic\n",
        );
        let pkgs = [req("node")];
        let branch = options(&[("node", Some("22/edge/fix-1"), true)]);
        assert!(statuses(&installed, &pkgs, &branch)[0].state.is_installed());
        let other_risk = options(&[("node", Some("22/beta/fix-1"), true)]);
        assert!(
            !statuses(&installed, &pkgs, &other_risk)[0]
                .state
                .is_installed()
        );
    }

    #[test]
    fn test_statuses_report_channel_drift() {
        let installed = parse_snap_list(SNAP_LIST);
        let pkgs = [req("code"), req("node"), req("hello"), req("missing")];
        let options = options(&[
            ("code", Some("stable"), true),
            ("node", Some("20"), true),
            ("hello", Some("latest/stable"), false),
        ]);
        let states = statuses(&installed, &pkgs, &options)
            .into_iter()
            .map(|status| status.state)
            .collect::<Vec<_>>();
        assert_eq!(
            states,
            [
                PackageState::Installed {
                    version: "1.95.3".to_string()
                },
                PackageState::VersionMismatch {
                    installed: "22.11.0 from 22/stable".to_string()
                },
                PackageState::VersionMismatch {
                    installed: "2.10 from -".to_string()
                },
                PackageState::Missing,
            ]
        );

        // without a declared channel, any tracked channel satisfies the entry
        let states = statuses(&installed, &pkgs[1..2], &ManagerPackageOptions::None);
        assert!(states[0].state.is_installed());
    }

    #[test]
    fn test_install_commands_batch_plain_snaps_and_refresh_drifted_ones() {
        let installed = parse_snap_list(SNAP_LIST);
        let pkgs = [req("jq"), req("node"), req("yq"), req("code-insiders")];
        let options = options(&[
            ("node", Some("20/stable"), true),
            ("code-insiders", None, true),
        ]);
        assert_eq!(
            commands("install", &pkgs, &installed, &options),
            [
                vec!["install", "jq", "yq"],
                vec!["refresh", "--channel=20/stable", "--classic", "node"],
                vec!["install", "--classic", "code-insiders"],
            ]
        );
    }

    #[test]
    fn test_refresh_and_remove_commands() {
        let pkgs = [req("jq"), req("node")];
        let options = options(&[("node", Some("22"), false)]);
        assert_eq!(
            commands("refresh", &pkgs, &HashMap::new(), &options),
            [
                vec!["refresh", "jq"],
                vec!["refresh", "--channel=22", "node"]
            ]
        );
        assert_eq!(
            commands(
                "remove",
                &pkgs,
                &HashMap::new(),
                &ManagerPackageOptions::None
            ),
            [vec!["remove", "jq", "node"]]
        );
    }

    #[test]
    fn test_validate_name() {
        for name in ["node", "code-insiders", "go", "1password", "lxd_test1"] {
            assert!(validate_name(name).is_ok(), "{name}");
        }
        for name in [
            "",
            "--classic",
            "Node",
            "a--b",
            "trailing-",
            "123",
            "x_",
            "x_A",
        ] {
            assert!(validate_name(name).is_err(), "{name}");
        }
    }

    #[test]
    fn test_validate_channel() {
        for channel in [
            "stable",
            "22",
            "latest/stable",
            "22/edge/fix-1",
            "1.x/beta",
            "edge/fix-1",
        ] {
            assert!(validate_channel("node", channel).is_ok(), "{channel}");
        }
        for channel in [
            "",
            "a//b",
            "a/b/c/d",
            "--edge",
            "latest stable",
            "latest/stabel",
            "22/unknown/branch",
            "latest/22",
        ] {
            assert!(validate_channel("node", channel).is_err(), "{channel}");
        }
    }

    #[tokio::test]
    async fn test_pins_are_rejected_before_running_snap() {
        let mut pinned = req("node");
        pinned.version = Some("22.11.0".to_string());
        let err = SnapManager
            .install(&[pinned], &InstallOpts::default())
            .await
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            "snap cannot install a pinned version ('node@22.11.0'); declare a channel instead"
        );
    }
}
