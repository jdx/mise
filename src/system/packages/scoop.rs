use std::collections::BTreeSet;
use std::path::PathBuf;
use std::process::Stdio;

use async_trait::async_trait;
use eyre::bail;
use serde::Deserialize;

use super::{InstallOpts, PackageRequest, PackageState, PackageStatus, SystemPackageManager};
use crate::result::Result;

/// `scoop bucket add` exit code for a bucket that is already present.
const BUCKET_ALREADY_EXISTS: i32 = 2;

/// Windows applications and command-line tools via Scoop.
pub(crate) struct ScoopManager {}

impl ScoopManager {
    /// Creates a Scoop package manager.
    pub(crate) fn new() -> Self {
        Self {}
    }

    /// Syncs Scoop itself and every configured bucket.
    async fn refresh(&self, dry_run: bool) -> Result<()> {
        apply(&refresh_args(), "update", dry_run, &[]).await
    }
}

/// `scoop export` as mise reads it. Unlisted fields are ignored.
#[derive(Debug, Deserialize)]
struct ScoopExport {
    #[serde(default)]
    buckets: Vec<ScoopBucket>,
    #[serde(default)]
    apps: Vec<ScoopApp>,
}

#[derive(Debug, Deserialize)]
struct ScoopBucket {
    #[serde(rename = "Name")]
    name: String,
}

#[derive(Debug, Deserialize)]
struct ScoopApp {
    #[serde(rename = "Name")]
    name: String,
    /// `null` for an app directory Scoop cannot resolve a current version for.
    #[serde(rename = "Version")]
    version: Option<String>,
    /// Comma-separated notes Scoop attaches to the row, such as
    /// `Install failed`, `Held package`, or `Global install`.
    #[serde(rename = "Info", default)]
    info: String,
}

impl ScoopApp {
    fn notes(&self) -> impl Iterator<Item = &str> {
        self.info.split(',').map(str::trim)
    }

    /// Scoop left this app's directory behind without a usable current version.
    fn failed(&self) -> bool {
        self.notes().any(|note| note == "Install failed")
    }

    fn is_global(&self) -> bool {
        self.notes().any(|note| note == "Global install")
    }
}

/// Splits a `bucket/app` request name into its bucket and app parts.
///
/// Scoop also accepts a manifest URL or local path in place of an app name.
/// Those are not buckets, so they come back whole and are rejected by
/// [`unsupported_name`].
fn split_bucket(name: &str) -> (Option<&str>, &str) {
    let Some((bucket, app)) = name.split_once('/') else {
        return (None, name);
    };
    let is_bucket = !bucket.is_empty()
        && !app.is_empty()
        && !app.contains('/')
        && bucket
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    if is_bucket {
        (Some(bucket), app)
    } else {
        (None, name)
    }
}

/// The app name Scoop records on disk, without any bucket qualifier.
fn app_name(name: &str) -> &str {
    split_bucket(name).1
}

/// A request Scoop can install but mise cannot reconcile afterwards.
///
/// `scoop install` also takes a manifest URL or local path, but it records the
/// app under the name the manifest declares. mise has no way to learn that
/// name, so status would report the entry missing forever and every apply
/// would reinstall it. Rejecting the declaration is better than that loop.
fn unsupported_name(name: &str) -> bool {
    let app = app_name(name);
    app.contains('/') || app.contains('\\') || app.to_ascii_lowercase().ends_with(".json")
}

/// Rejects declarations mise cannot converge, naming every offending entry.
fn check_supported(pkgs: &[PackageRequest]) -> Result<()> {
    let unsupported = pkgs
        .iter()
        .filter(|pkg| unsupported_name(&pkg.name))
        .map(|pkg| format!("'{}'", pkg.name))
        .collect::<Vec<_>>();
    if unsupported.is_empty() {
        return Ok(());
    }
    bail!(
        "scoop: {} names a manifest URL or path; declare the app name Scoop installs it as, \
         optionally qualified with a bucket",
        unsupported.join(", ")
    );
}

/// Builds the operand that selects a package, keeping any bucket qualifier
/// and appending an opaque version pin in Scoop's `app@version` syntax.
fn install_spec(request: &PackageRequest) -> String {
    match &request.version {
        Some(version) => format!("{}@{version}", request.name),
        None => request.name.clone(),
    }
}

/// `--no-update-scoop` keeps an install from syncing Scoop and every bucket
/// as a side effect; mise refreshes only when asked to with `--update`.
fn install_args(specs: &[String]) -> Vec<String> {
    let mut args = vec!["install".to_string(), "--no-update-scoop".to_string()];
    args.extend(specs.iter().cloned());
    args
}

fn upgrade_args(apps: &[String]) -> Vec<String> {
    let mut args = vec!["update".to_string()];
    args.extend(apps.iter().cloned());
    args
}

fn uninstall_args(apps: &[String]) -> Vec<String> {
    let mut args = vec!["uninstall".to_string()];
    args.extend(apps.iter().cloned());
    args
}

fn bucket_add_args(bucket: &str) -> Vec<String> {
    vec!["bucket".to_string(), "add".to_string(), bucket.to_string()]
}

fn refresh_args() -> Vec<String> {
    vec!["update".to_string()]
}

/// Buckets named by `bucket/app` requests that Scoop does not have yet, in
/// first-requested order.
fn missing_buckets(pkgs: &[PackageRequest], export: &ScoopExport) -> Vec<String> {
    let present = export
        .buckets
        .iter()
        .map(|bucket| bucket.name.to_ascii_lowercase())
        .collect::<BTreeSet<_>>();
    let mut wanted = Vec::new();
    for bucket in pkgs.iter().filter_map(|pkg| split_bucket(&pkg.name).0) {
        let key = bucket.to_ascii_lowercase();
        if !present.contains(&key) && !wanted.iter().any(|b: &String| *b == bucket) {
            wanted.push(bucket.to_string());
        }
    }
    wanted
}

/// Apps that must be uninstalled before their pin can be installed.
///
/// `scoop install <app>@<version>` skips an app that is already installed at
/// a different version, so mise removes it first — the same uninstall the
/// `scoop update` path performs internally when it moves an app's version.
///
/// A global-only install is left out: mise installs into the user scope, so
/// there is nothing local to remove, and `scoop uninstall` without `--global`
/// would exit successfully without touching it. The pinned install that
/// follows creates the user-scope copy the declaration asks for.
fn pinned_reinstalls(pkgs: &[PackageRequest], export: &ScoopExport) -> Vec<String> {
    let mut apps = Vec::new();
    for pkg in pkgs {
        let Some(pin) = &pkg.version else { continue };
        let app = app_name(&pkg.name);
        let Some(entry) = find_app(export, app) else {
            continue;
        };
        if entry.is_global() {
            continue;
        }
        if entry.version.as_deref() != Some(pin.as_str())
            && !apps.iter().any(|existing: &String| existing == app)
        {
            apps.push(app.to_string());
        }
    }
    apps
}

/// App names to act on, dropping any that exist only as a global install.
fn user_scope_apps(pkgs: &[PackageRequest], global: &[String]) -> Vec<String> {
    pkgs.iter()
        .map(|pkg| app_name(&pkg.name).to_string())
        .filter(|app| !global.contains(app))
        .collect()
}

/// Whether any exported entry for `app` is a global install.
fn has_global(export: &ScoopExport, app: &str) -> bool {
    matching(export, app).any(ScoopApp::is_global)
}

/// Whether any exported entry for `app` is a user-scope install.
fn has_local(export: &ScoopExport, app: &str) -> bool {
    matching(export, app).any(|entry| !entry.is_global())
}

/// The requested app names a predicate keeps, deduplicated, in request order.
fn selected_apps(pkgs: &[PackageRequest], keep: impl Fn(&str) -> bool) -> Vec<String> {
    let mut apps = Vec::new();
    for pkg in pkgs {
        let app = app_name(&pkg.name);
        if keep(app) && !apps.iter().any(|existing: &String| existing == app) {
            apps.push(app.to_string());
        }
    }
    apps
}

/// Requested apps whose only install is global, in request order.
///
/// `scoop uninstall` and `scoop update` without `--global` report that the app
/// is not installed locally and still exit zero, so acting on one of these
/// would look like it succeeded while changing nothing. An app that also has a
/// user-scope copy is absent here, because that copy is mise's to act on.
fn global_only(pkgs: &[PackageRequest], export: &ScoopExport) -> Vec<String> {
    selected_apps(pkgs, |app| {
        has_global(export, app) && !has_local(export, app)
    })
}

/// Requested apps with a global install, whether or not a local copy exists.
///
/// Removal needs this wider set: uninstalling the user-scope copy of an app
/// installed in both scopes leaves the global one behind, and the driver would
/// otherwise report the app as removed.
fn global_copies(pkgs: &[PackageRequest], export: &ScoopExport) -> Vec<String> {
    selected_apps(pkgs, |app| has_global(export, app))
}

/// Every exported entry for `app`. Scoop keys apps by name, so a name can
/// appear twice at most: once per scope.
fn matching<'a>(export: &'a ScoopExport, app: &str) -> impl Iterator<Item = &'a ScoopApp> {
    let app = app.to_string();
    export
        .apps
        .iter()
        .filter(move |entry| entry.name.eq_ignore_ascii_case(&app))
}

/// The exported entry for `app`, preferring a local install over a global one.
fn find_app<'a>(export: &'a ScoopExport, app: &str) -> Option<&'a ScoopApp> {
    let mut global = None;
    for entry in matching(export, app) {
        if !entry.is_global() {
            return Some(entry);
        }
        global.get_or_insert(entry);
    }
    global
}

/// Compares the installed version with an optional opaque version pin.
fn package_state(request: &PackageRequest, export: &ScoopExport) -> PackageState {
    let Some(entry) = find_app(export, app_name(&request.name)) else {
        return PackageState::Missing;
    };
    let installed = entry.version.clone();
    if entry.failed() || installed.is_none() {
        return PackageState::NeedsRepair {
            installed: installed.unwrap_or_else(|| "unknown".to_string()),
        };
    }
    let installed = installed.unwrap_or_default();
    match &request.version {
        Some(pin) if pin != &installed => PackageState::VersionMismatch { installed },
        _ => PackageState::Installed { version: installed },
    }
}

/// Strips anything Scoop printed ahead of the exported JSON document.
///
/// `scoop export` pretty-prints the object, so the document opens with a `{`
/// on a line of its own. Scoop's own notices go to the information stream,
/// but a bucket's `git` output or a PowerShell profile can still leave a line
/// on stdout ahead of it.
fn json_body(output: &str) -> &str {
    let trimmed = output.trim_start_matches('\u{feff}').trim_start();
    if trimmed.starts_with('{') {
        return trimmed;
    }
    match trimmed.find("\n{") {
        Some(offset) => &trimmed[offset + 1..],
        None => trimmed,
    }
}

fn parse_export(output: &str) -> Result<ScoopExport> {
    let body = json_body(output);
    serde_json::from_str(body).map_err(|err| {
        eyre::eyre!(
            "failed to parse `scoop export` output: {err}\n{}",
            body.trim()
        )
    })
}

/// Scoop ships as a PowerShell script behind a `scoop.cmd` shim, so the
/// resolved path is spawned rather than the bare name.
fn scoop_bin() -> Result<PathBuf> {
    crate::file::which_spawnable("scoop").ok_or_else(|| eyre::eyre!("scoop not found on PATH"))
}

/// Reads Scoop's installed apps and buckets. Side-effect free.
async fn export() -> Result<ScoopExport> {
    let bin = scoop_bin()?;
    debug!("$ scoop export");
    let output = tokio::process::Command::new(&bin)
        .arg("export")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let detail = [stdout.trim(), stderr.trim()]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        bail!("scoop export failed: {detail}");
    }
    parse_export(&stdout)
}

/// Runs Scoop and accepts only zero plus explicitly allowed no-op codes.
async fn run_scoop(args: &[String], action: &str, accepted_exit_codes: &[i32]) -> Result<()> {
    let bin = scoop_bin()?;
    debug!("$ scoop {}", args.join(" "));
    let status = tokio::process::Command::new(&bin)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .await?;
    let code = status.code();
    if code != Some(0) && !code.is_some_and(|code| accepted_exit_codes.contains(&code)) {
        bail!("scoop {action} failed with {status}");
    }
    Ok(())
}

/// Prints the command in dry-run mode, otherwise runs it.
async fn apply(
    args: &[String],
    action: &str,
    dry_run: bool,
    accepted_exit_codes: &[i32],
) -> Result<()> {
    if dry_run {
        miseprintln!("scoop {}", args.join(" "));
        return Ok(());
    }
    run_scoop(args, action, accepted_exit_codes).await
}

#[async_trait(?Send)]
impl SystemPackageManager for ScoopManager {
    fn name(&self) -> &str {
        "scoop"
    }

    fn is_available(&self) -> bool {
        cfg!(windows) && crate::file::which_spawnable("scoop").is_some()
    }

    fn unavailable_reason(&self) -> String {
        if cfg!(windows) {
            "scoop not found".to_string()
        } else {
            "only available on windows".to_string()
        }
    }

    async fn installed(&self, pkgs: &[PackageRequest]) -> Result<Vec<PackageStatus>> {
        if pkgs.is_empty() {
            return Ok(vec![]);
        }
        check_supported(pkgs)?;
        let export = export().await?;
        Ok(pkgs
            .iter()
            .map(|pkg| PackageStatus {
                request: pkg.clone(),
                state: package_state(pkg, &export),
            })
            .collect())
    }

    async fn install(&self, pkgs: &[PackageRequest], opts: &InstallOpts) -> Result<()> {
        if pkgs.is_empty() {
            return Ok(());
        }
        // `scoop export` is read-only, so the dry run reads the same state the
        // real run would and prints the bucket and uninstall steps it implies.
        let export = export().await?;
        for bucket in missing_buckets(pkgs, &export) {
            apply(
                &bucket_add_args(&bucket),
                &format!("bucket add {bucket}"),
                opts.dry_run,
                &[BUCKET_ALREADY_EXISTS],
            )
            .await?;
        }
        if opts.update {
            self.refresh(opts.dry_run).await?;
        }
        // Apps in `NeedsRepair` need no uninstall step here: `scoop install`
        // runs `ensure_none_failed` first, which resets an app whose current
        // version still resolves and purges one whose does not, before the
        // install proceeds.
        let reinstall = pinned_reinstalls(pkgs, &export);
        if !reinstall.is_empty() {
            apply(&uninstall_args(&reinstall), "uninstall", opts.dry_run, &[]).await?;
        }
        let specs = pkgs.iter().map(install_spec).collect::<Vec<_>>();
        apply(&install_args(&specs), "install", opts.dry_run, &[]).await
    }

    async fn upgrade(&self, pkgs: &[PackageRequest], opts: &InstallOpts) -> Result<()> {
        if pkgs.is_empty() {
            return Ok(());
        }
        // `scoop update <app>` always moves to the bucket's current manifest
        // and has no way to hold a version, so a pinned entry is left alone
        // instead of being silently upgraded off its pin.
        let (pinned, unpinned): (Vec<_>, Vec<_>) =
            pkgs.iter().partition(|pkg| pkg.version.is_some());
        for pkg in pinned {
            warn!("scoop: '{pkg}' pins a version, skipping upgrade");
        }
        if unpinned.is_empty() {
            return Ok(());
        }
        let unpinned = unpinned.into_iter().cloned().collect::<Vec<_>>();
        // A global-only install is outside the user scope mise manages.
        // `scoop update` without `--global` only prints that the app is not
        // installed locally and still exits zero, so filtering these out
        // keeps the run quiet about apps mise deliberately leaves alone.
        let global = global_only(&unpinned, &export().await?);
        for app in &global {
            warn!("scoop: '{app}' is installed globally, skipping upgrade");
        }
        let apps = user_scope_apps(&unpinned, &global);
        if apps.is_empty() {
            return Ok(());
        }
        // `scoop update <app>` syncs buckets only when Scoop already considers
        // itself outdated (`LAST_UPDATE` at least three hours old), so an
        // upgrade inside that window would compare against a stale bucket
        // clone and silently find nothing to do. Refresh explicitly, the way
        // the other managers' upgrades do.
        self.refresh(opts.dry_run).await?;
        apply(&upgrade_args(&apps), "update", opts.dry_run, &[]).await
    }

    fn supports_remove(&self) -> bool {
        true
    }

    async fn remove(&self, pkgs: &[PackageRequest], opts: &InstallOpts) -> Result<()> {
        if pkgs.is_empty() {
            return Ok(());
        }
        let export = export().await?;
        // Remove everything mise can reach before reporting the rest. Failing
        // first would strand every other removable app in the batch, and the
        // driver aborts the whole run on this error, so it would also skip the
        // managers queued behind scoop.
        let apps = user_scope_apps(pkgs, &global_only(pkgs, &export));
        if !apps.is_empty() {
            apply(&uninstall_args(&apps), "uninstall", opts.dry_run, &[]).await?;
        }
        // Wider than the set held back above: an app installed in both scopes
        // just lost its user-scope copy, and the global one is still there.
        let global = global_copies(pkgs, &export);
        if !global.is_empty() {
            // Not a warning: the driver reports the batch it handed over as
            // removed, so anything mise did not remove has to fail the run
            // rather than let that report stand.
            bail!(
                "scoop: {} installed globally; mise manages the user scope only. \
                 Remove it with `scoop uninstall --global {}` from an elevated shell.",
                global
                    .iter()
                    .map(|app| format!("'{app}'"))
                    .collect::<Vec<_>>()
                    .join(", "),
                global.join(" ")
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXPORT: &str = r#"{
    "buckets":  [
                    {
                        "Name":  "main",
                        "Source":  "https://github.com/ScoopInstaller/Main",
                        "Updated":  "2026-09-15T10:00:00",
                        "Manifests":  1317
                    }
                ],
    "apps":  [
                 {
                     "Info":  "",
                     "Source":  "main",
                     "Name":  "7zip",
                     "Updated":  "2026-09-01T12:00:00",
                     "Version":  "24.09"
                 },
                 {
                     "Info":  "Held package",
                     "Source":  "main",
                     "Name":  "ripgrep",
                     "Updated":  "2026-08-02T09:30:00",
                     "Version":  "14.1.1"
                 }
             ]
}"#;

    fn req(name: &str, version: Option<&str>) -> PackageRequest {
        PackageRequest {
            name: name.to_string(),
            version: version.map(str::to_string),
            tap_url: None,
            desired: crate::system::packages::PackageDesiredState::Present,
        }
    }

    fn export_of(apps: &[(&str, Option<&str>, &str)], buckets: &[&str]) -> ScoopExport {
        ScoopExport {
            buckets: buckets
                .iter()
                .map(|name| ScoopBucket {
                    name: name.to_string(),
                })
                .collect(),
            apps: apps
                .iter()
                .map(|(name, version, info)| ScoopApp {
                    name: name.to_string(),
                    version: version.map(str::to_string),
                    info: info.to_string(),
                })
                .collect(),
        }
    }

    #[test]
    fn splits_a_bucket_qualifier_but_leaves_a_manifest_url_alone() {
        assert_eq!(split_bucket("ripgrep"), (None, "ripgrep"));
        assert_eq!(split_bucket("extras/vscode"), (Some("extras"), "vscode"));
        assert_eq!(
            split_bucket("nerd-fonts/FiraCode-NF"),
            (Some("nerd-fonts"), "FiraCode-NF")
        );
        let url = "https://example.com/bucket/runat.json";
        assert_eq!(split_bucket(url), (None, url));
        assert_eq!(split_bucket("extras/"), (None, "extras/"));
        assert_eq!(split_bucket("/vscode"), (None, "/vscode"));
    }

    #[test]
    fn a_manifest_url_or_path_is_rejected_rather_than_reinstalled_forever() {
        for name in [
            "https://example.com/bucket/runat.json",
            r"C:\\manifests\\runat.json",
            "runat.JSON",
        ] {
            assert!(unsupported_name(name), "{name}");
        }
        for name in ["ripgrep", "extras/vscode", "nerd-fonts/FiraCode-NF"] {
            assert!(!unsupported_name(name), "{name}");
        }

        let err = check_supported(&[
            req("ripgrep", None),
            req("https://example.com/bucket/runat.json", None),
        ])
        .unwrap_err();
        assert_eq!(
            err.to_string(),
            "scoop: 'https://example.com/bucket/runat.json' names a manifest URL or path; \
             declare the app name Scoop installs it as, optionally qualified with a bucket"
        );
        check_supported(&[req("extras/vscode", Some("1.99.0"))]).unwrap();
    }

    #[test]
    fn a_global_only_install_is_never_uninstalled_from_the_user_scope() {
        let export = export_of(
            &[
                ("git", Some("2.48.0"), "Global install"),
                ("ripgrep", Some("14.1.1"), ""),
            ],
            &[],
        );
        // `scoop uninstall git` would exit zero without touching the global
        // install, so it must not be part of a pin reconciliation...
        assert_eq!(
            pinned_reinstalls(
                &[req("git", Some("2.51.0")), req("ripgrep", Some("13.0.0"))],
                &export
            ),
            vec!["ripgrep"]
        );
        // ...and a removal targeting it has to say so instead of succeeding.
        assert_eq!(
            global_only(&[req("git", None), req("ripgrep", None)], &export),
            vec!["git"]
        );
        assert!(global_only(&[req("ripgrep", None), req("gh", None)], &export).is_empty());
    }

    #[test]
    fn an_app_in_both_scopes_loses_its_local_copy_and_still_reports_the_global_one() {
        let export = export_of(
            &[
                ("git", Some("2.51.0"), ""),
                ("git", Some("2.48.0"), "Global install"),
                ("ripgrep", Some("14.1.1"), ""),
                ("gh", Some("2.60.0"), "Global install"),
            ],
            &[],
        );
        let pkgs = [req("git", None), req("ripgrep", None), req("gh", None)];

        // Only `gh` has nothing for mise to uninstall, so `git`'s user-scope
        // copy is still removed...
        assert_eq!(global_only(&pkgs, &export), vec!["gh"]);
        assert_eq!(
            user_scope_apps(&pkgs, &global_only(&pkgs, &export)),
            vec!["git", "ripgrep"]
        );
        // ...and both `git` and `gh` are reported, because a global copy of
        // `git` survives that uninstall.
        assert_eq!(global_copies(&pkgs, &export), vec!["git", "gh"]);

        // Upgrade keeps using the narrow set: git's local copy is upgradable.
        assert_eq!(
            user_scope_apps(&pkgs, &global_only(&pkgs, &export)),
            vec!["git", "ripgrep"]
        );
        assert!(has_global(&export, "git") && has_local(&export, "git"));
        assert!(has_global(&export, "gh") && !has_local(&export, "gh"));
        assert!(!has_global(&export, "ripgrep") && has_local(&export, "ripgrep"));
    }

    #[test]
    fn one_global_app_does_not_strand_the_rest_of_a_batch() {
        let pkgs = [
            req("git", None),
            req("extras/vscode", None),
            req("ripgrep", None),
        ];
        let global = vec!["git".to_string()];
        // The removable apps still go out in one `scoop uninstall`; only the
        // global-only one is held back for the error that follows.
        assert_eq!(user_scope_apps(&pkgs, &global), vec!["vscode", "ripgrep"]);
        assert_eq!(
            uninstall_args(&user_scope_apps(&pkgs, &global)),
            vec!["uninstall", "vscode", "ripgrep"]
        );
        assert!(user_scope_apps(&pkgs, &["git", "vscode", "ripgrep"].map(String::from)).is_empty());
        assert_eq!(
            user_scope_apps(&pkgs, &[]),
            vec!["git", "vscode", "ripgrep"]
        );
    }

    #[test]
    fn keeps_the_bucket_on_install_and_drops_it_for_installed_app_names() {
        assert_eq!(install_spec(&req("extras/vscode", None)), "extras/vscode");
        assert_eq!(
            install_spec(&req("extras/vscode", Some("1.100.0"))),
            "extras/vscode@1.100.0"
        );
        assert_eq!(app_name("extras/vscode"), "vscode");
        assert_eq!(app_name("ripgrep"), "ripgrep");
    }

    #[test]
    fn install_does_not_sync_scoop_as_a_side_effect() {
        assert_eq!(
            install_args(&["ripgrep".to_string(), "extras/vscode@1.100.0".to_string()]),
            vec![
                "install",
                "--no-update-scoop",
                "ripgrep",
                "extras/vscode@1.100.0"
            ]
        );
        assert_eq!(refresh_args(), vec!["update"]);
    }

    #[test]
    fn parses_installed_apps_and_buckets_from_scoop_export() {
        let export = parse_export(EXPORT).unwrap();
        assert_eq!(
            export
                .buckets
                .iter()
                .map(|bucket| bucket.name.as_str())
                .collect::<Vec<_>>(),
            vec!["main"]
        );
        assert_eq!(
            package_state(&req("7zip", None), &export),
            PackageState::Installed {
                version: "24.09".to_string()
            }
        );
        assert_eq!(
            package_state(&req("ripgrep", Some("14.1.1")), &export),
            PackageState::Installed {
                version: "14.1.1".to_string()
            }
        );
        assert_eq!(
            package_state(&req("ripgrep", Some("13.0.0")), &export),
            PackageState::VersionMismatch {
                installed: "14.1.1".to_string()
            }
        );
        assert_eq!(
            package_state(&req("extras/vscode", None), &export),
            PackageState::Missing
        );
    }

    #[test]
    fn an_empty_scoop_installation_parses_as_no_apps() {
        let export = parse_export("{\r\n    \"buckets\":  [],\r\n    \"apps\":  []\r\n}").unwrap();
        assert_eq!(
            package_state(&req("ripgrep", None), &export),
            PackageState::Missing
        );
        assert!(missing_buckets(&[req("extras/vscode", None)], &export) == vec!["extras"]);
    }

    #[test]
    fn a_line_printed_before_the_document_does_not_break_parsing() {
        let noisy = format!("\u{feff}Updating Scoop...\n{EXPORT}");
        assert_eq!(
            package_state(&req("7zip", None), &parse_export(&noisy).unwrap()),
            PackageState::Installed {
                version: "24.09".to_string()
            }
        );
    }

    #[test]
    fn unparseable_output_reports_what_scoop_printed() {
        let err = parse_export("scoop is not recognized").unwrap_err();
        assert!(err.to_string().contains("scoop is not recognized"), "{err}");
    }

    #[test]
    fn a_failed_or_versionless_install_needs_repair() {
        let export = export_of(
            &[
                ("neovim", Some("0.11.0"), "Install failed"),
                ("gh", None, ""),
            ],
            &[],
        );
        assert_eq!(
            package_state(&req("neovim", None), &export),
            PackageState::NeedsRepair {
                installed: "0.11.0".to_string()
            }
        );
        assert_eq!(
            package_state(&req("gh", Some("2.60.0")), &export),
            PackageState::NeedsRepair {
                installed: "unknown".to_string()
            }
        );
    }

    #[test]
    fn a_local_install_wins_over_a_global_one_with_the_same_name() {
        let export = export_of(
            &[
                ("git", Some("2.48.0"), "Global install"),
                ("git", Some("2.51.0"), ""),
            ],
            &[],
        );
        assert_eq!(
            package_state(&req("git", None), &export),
            PackageState::Installed {
                version: "2.51.0".to_string()
            }
        );

        let global_only = export_of(&[("git", Some("2.48.0"), "Global install")], &[]);
        assert_eq!(
            package_state(&req("git", None), &global_only),
            PackageState::Installed {
                version: "2.48.0".to_string()
            }
        );
    }

    #[test]
    fn only_buckets_scoop_is_missing_are_added_once_each() {
        let export = export_of(&[], &["main", "Extras"]);
        assert_eq!(
            missing_buckets(
                &[
                    req("ripgrep", None),
                    req("extras/vscode", None),
                    req("versions/go1.21", None),
                    req("versions/python27", None),
                ],
                &export
            ),
            vec!["versions"]
        );
    }

    #[test]
    fn a_pin_that_differs_from_the_installed_version_uninstalls_first() {
        let export = export_of(
            &[
                ("ripgrep", Some("14.1.1"), ""),
                ("vscode", Some("1.100.0"), ""),
                ("gh", None, "Install failed"),
            ],
            &[],
        );
        assert_eq!(
            pinned_reinstalls(
                &[
                    // pinned to what is installed — nothing to do
                    req("ripgrep", Some("14.1.1")),
                    // pinned to a different version — must be removed first
                    req("extras/vscode", Some("1.99.0")),
                    // a broken install cannot satisfy a pin either
                    req("gh", Some("2.60.0")),
                    // not installed at all — `scoop install gh@x` is enough
                    req("neovim", Some("0.11.0")),
                    // unpinned entries are never reinstalled
                    req("7zip", None),
                ],
                &export
            ),
            vec!["vscode", "gh"]
        );
        assert_eq!(
            uninstall_args(&["vscode".to_string()]),
            vec!["uninstall", "vscode"]
        );
    }

    #[test]
    fn upgrade_targets_bare_app_names() {
        assert_eq!(
            upgrade_args(&["ripgrep".to_string(), "vscode".to_string()]),
            vec!["update", "ripgrep", "vscode"]
        );
        assert_eq!(bucket_add_args("extras"), vec!["bucket", "add", "extras"]);
    }

    #[tokio::test]
    async fn upgrade_skips_pinned_entries_without_running_scoop() {
        let manager = ScoopManager::new();
        manager.upgrade(&[], &InstallOpts::default()).await.unwrap();
        manager
            .upgrade(
                &[req("extras/vscode", Some("1.99.0"))],
                &InstallOpts::default(),
            )
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn empty_batches_never_run_scoop() {
        let manager = ScoopManager::new();
        assert!(manager.installed(&[]).await.unwrap().is_empty());
        manager.install(&[], &InstallOpts::default()).await.unwrap();
        manager.remove(&[], &InstallOpts::default()).await.unwrap();
    }

    #[test]
    fn is_a_windows_only_manager_that_supports_removal() {
        let manager = ScoopManager::new();
        assert_eq!(manager.name(), "scoop");
        assert!(manager.supports_remove());
        assert!(manager.supports_version_pins());
        if cfg!(windows) {
            assert_eq!(manager.unavailable_reason(), "scoop not found");
        } else {
            assert!(!manager.is_available());
            assert_eq!(manager.unavailable_reason(), "only available on windows");
        }
    }
}
