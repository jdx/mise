use std::fmt;
use std::fs::{self, File};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Output, Stdio};

use eyre::{Context, Result, bail, eyre};
use indexmap::{IndexMap, IndexSet};
use serde::Deserialize;

use crate::config::Config;
use crate::http::HTTP;
use crate::ui::multi_progress_report::MultiProgressReport;

const RELEASE_BASE_URL: &str = "https://github.com/jdx/mise/releases/download";

#[derive(Clone, Debug, Default, Deserialize)]
pub struct RemoteTomlConfig {
    pub source: Option<PathBuf>,
    #[serde(default)]
    pub copy_links: bool,
    #[serde(default)]
    pub copy_link: Vec<PathBuf>,
    #[serde(default)]
    pub exclude: Vec<String>,
    #[serde(default)]
    pub hosts: IndexMap<String, RemoteHostTomlConfig>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct RemoteHostTomlConfig {
    pub host: String,
    pub user: Option<String>,
    pub port: Option<u16>,
    pub identity_file: Option<PathBuf>,
    pub source: Option<PathBuf>,
    pub copy_links: Option<bool>,
    #[serde(default)]
    pub copy_link: Vec<PathBuf>,
    #[serde(default)]
    pub exclude: Vec<String>,
    #[serde(default)]
    pub ssh_options: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub mise_bin: Option<PathBuf>,
    pub remote_mise: Option<String>,
    pub bootstrap_command: Option<String>,
}

#[derive(Clone, Debug)]
pub struct RemoteHost {
    pub name: String,
    pub host: String,
    pub user: Option<String>,
    pub port: Option<u16>,
    pub identity_file: Option<PathBuf>,
    pub source: PathBuf,
    pub copy_links: bool,
    pub copy_link: Vec<PathBuf>,
    pub exclude: Vec<String>,
    pub ssh_options: Vec<String>,
    pub tags: IndexSet<String>,
    pub mise_bin: Option<PathBuf>,
    pub remote_mise: Option<String>,
    pub bootstrap_command: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct RemoteOverrides {
    pub source: Option<PathBuf>,
    pub copy_links: bool,
    pub copy_link: Vec<PathBuf>,
    pub port: Option<u16>,
    pub identity_file: Option<PathBuf>,
    pub exclude: Vec<String>,
    pub ssh_options: Vec<String>,
    pub mise_bin: Option<PathBuf>,
    pub remote_mise: Option<String>,
    pub bootstrap_command: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct RemoteRunOptions {
    pub dry_run: bool,
    pub yes: bool,
    pub update: bool,
    pub prompt_secrets: bool,
    pub force_dotfiles: bool,
    pub skip: Vec<String>,
    pub only: Vec<String>,
    pub keep_staging: bool,
    pub connect_timeout: u16,
}

#[derive(Default)]
pub struct RemoteArtifactResolver {
    directory: Option<tempfile::TempDir>,
    manifest: Option<ReleaseManifest>,
    artifacts: IndexMap<String, PathBuf>,
    official_local_verified: bool,
}

#[derive(Clone, Debug)]
struct ReleaseManifest {
    checksums: std::collections::HashMap<String, String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RemotePlatform {
    os: String,
    arch: String,
    libc: Option<LibcFlavor>,
}

pub fn hosts_from_config(
    config: &Config,
    layered_excludes: &[String],
) -> Result<IndexMap<String, RemoteHost>> {
    let mut hosts = IndexMap::new();
    for cf in config.config_files.values() {
        let Some(bootstrap) = cf.bootstrap_config() else {
            continue;
        };
        let remote = bootstrap.remote;
        let base = cf.get_path().parent().unwrap_or_else(|| Path::new("."));
        let default_source = resolve_local_path(base, remote.source.as_deref())?;
        let default_copy_links = remote.copy_links;
        let default_copy_link = remote.copy_link;
        for (name, host) in remote.hosts {
            if hosts.contains_key(&name) {
                continue;
            }
            let source = match host.source.as_deref() {
                Some(source) => resolve_local_path(base, Some(source))?
                    .expect("a provided source resolves to a path"),
                None => default_source.clone().unwrap_or(std::env::current_dir()?),
            };
            let identity_file = resolve_local_path(base, host.identity_file.as_deref())?;
            let mise_bin = resolve_local_path(base, host.mise_bin.as_deref())?;
            let mut exclude = default_excludes();
            exclude.extend_from_slice(layered_excludes);
            exclude.extend(host.exclude);
            let mut copy_link = default_copy_link.clone();
            copy_link.extend(host.copy_link);
            let host = RemoteHost {
                name: name.clone(),
                host: host.host,
                user: host.user,
                port: host.port,
                identity_file,
                source,
                copy_links: host.copy_links.unwrap_or(default_copy_links),
                copy_link: dedupe_paths(copy_link),
                exclude: dedupe(exclude),
                ssh_options: dedupe(host.ssh_options),
                tags: host.tags.into_iter().collect(),
                mise_bin,
                remote_mise: host.remote_mise,
                bootstrap_command: host.bootstrap_command,
            };
            hosts.insert(name, host);
        }
    }
    Ok(hosts)
}

pub fn excludes_from_config(config: &Config) -> Vec<String> {
    config
        .config_files
        .values()
        .filter_map(|cf| cf.bootstrap_config())
        .flat_map(|bootstrap| bootstrap.remote.exclude)
        .collect()
}

pub fn ad_hoc_host(
    destination: &str,
    source: PathBuf,
    config_excludes: &[String],
) -> Result<RemoteHost> {
    let (user, host) = destination
        .rsplit_once('@')
        .map(|(user, host)| (Some(user.to_string()), host.to_string()))
        .unwrap_or((None, destination.to_string()));
    let target = RemoteHost {
        name: destination.to_string(),
        host,
        user,
        port: None,
        identity_file: None,
        source,
        copy_links: false,
        copy_link: vec![],
        exclude: dedupe(
            default_excludes()
                .into_iter()
                .chain(config_excludes.iter().cloned())
                .collect(),
        ),
        ssh_options: vec![],
        tags: IndexSet::new(),
        mise_bin: None,
        remote_mise: None,
        bootstrap_command: None,
    };
    target.validate()?;
    Ok(target)
}

impl RemoteHost {
    pub fn apply_overrides(&mut self, overrides: &RemoteOverrides) -> Result<()> {
        if let Some(source) = &overrides.source {
            self.source = absolutize(source)?;
        }
        if overrides.copy_links {
            self.copy_links = true;
        }
        self.copy_link.extend(overrides.copy_link.clone());
        self.copy_link = dedupe_paths(std::mem::take(&mut self.copy_link));
        if let Some(port) = overrides.port {
            self.port = Some(port);
        }
        if let Some(identity_file) = &overrides.identity_file {
            self.identity_file = Some(absolutize(&crate::file::replace_path(identity_file))?);
        }
        self.exclude.extend(overrides.exclude.clone());
        self.exclude = dedupe(std::mem::take(&mut self.exclude));
        self.ssh_options.extend(overrides.ssh_options.clone());
        self.ssh_options = dedupe(std::mem::take(&mut self.ssh_options));
        if overrides.mise_bin.is_some()
            || overrides.remote_mise.is_some()
            || overrides.bootstrap_command.is_some()
        {
            self.mise_bin = overrides
                .mise_bin
                .as_ref()
                .map(|binary| absolutize(&crate::file::replace_path(binary)))
                .transpose()?;
            self.remote_mise.clone_from(&overrides.remote_mise);
            self.bootstrap_command
                .clone_from(&overrides.bootstrap_command);
        }
        self.validate()
    }

    pub fn destination(&self) -> String {
        match &self.user {
            Some(user) => format!("{user}@{}", self.host),
            None => self.host.clone(),
        }
    }

    fn validate(&self) -> Result<()> {
        validate_ssh_atom("host", &self.host)?;
        if let Some(user) = &self.user {
            validate_ssh_atom("user", user)?;
        }
        if self.port == Some(0) {
            bail!("remote host '{}' port must be greater than zero", self.name);
        }
        if !self.source.is_dir() {
            bail!(
                "remote host '{}' source is not a directory: {}",
                self.name,
                self.source.display()
            );
        }
        if !self.copy_links {
            for link in &self.copy_link {
                validate_copy_link(&self.source, link).wrap_err_with(|| {
                    format!("remote host '{}' has invalid copy_link", self.name)
                })?;
            }
        }
        if let Some(identity) = &self.identity_file
            && !identity.is_file()
        {
            bail!(
                "remote host '{}' identity file does not exist: {}",
                self.name,
                identity.display()
            );
        }
        if let Some(binary) = &self.mise_bin
            && !binary.is_file()
        {
            bail!(
                "remote host '{}' mise binary does not exist: {}",
                self.name,
                binary.display()
            );
        }
        let provisioning_strategies = [
            self.mise_bin.is_some(),
            self.remote_mise.is_some(),
            self.bootstrap_command.is_some(),
        ]
        .into_iter()
        .filter(|configured| *configured)
        .count();
        if provisioning_strategies > 1 {
            bail!(
                "remote host '{}' must set at most one of mise_bin, remote_mise, or bootstrap_command",
                self.name
            );
        }
        for option in &self.ssh_options {
            validate_value("SSH option", option)?;
        }
        for exclude in &self.exclude {
            validate_value("archive exclude", exclude)?;
        }
        if let Some(remote_mise) = &self.remote_mise {
            validate_remote_executable(remote_mise)?;
        }
        if self
            .bootstrap_command
            .as_ref()
            .is_some_and(|command| command.contains('\0'))
        {
            bail!("remote host '{}' bootstrap command contains NUL", self.name);
        }
        Ok(())
    }
}

pub async fn run(
    host: &RemoteHost,
    options: &RemoteRunOptions,
    artifacts: &mut RemoteArtifactResolver,
) -> Result<()> {
    let ssh = crate::file::which("ssh").ok_or_else(|| eyre!("required command 'ssh' not found"))?;
    let tar = crate::file::which("tar").ok_or_else(|| eyre!("required command 'tar' not found"))?;
    let control_directory = if cfg!(unix) {
        Some(tempfile::tempdir()?)
    } else {
        None
    };
    let session = SshSession {
        ssh,
        host,
        connect_timeout: options.connect_timeout,
        control_path: control_directory
            .as_ref()
            .map(|directory| directory.path().join("control")),
    };
    info!("bootstrap remote {} ({})", host.name, host.destination());
    let staging = session
        .output(&["sh", "-c", staging_creation_script()])?
        .trim()
        .to_string();
    validate_staging_path(&staging)?;
    let mut result = run_staged(&session, &tar, &staging, options, artifacts).await;
    if options.keep_staging {
        warn!("remote staging retained on {}: {staging}", host.name);
    } else if let Err(cleanup_error) = session.status(&["rm", "-rf", "--", &staging], false) {
        if result.is_ok() {
            result = Err(cleanup_error);
        } else {
            warn!(
                "failed to clean remote staging on {}: {cleanup_error:#}",
                host.name
            );
        }
    }
    result
}

fn staging_creation_script() -> &'static str {
    r#"set -eu
staging=$(mktemp -d /tmp/mise-bootstrap.XXXXXXXXXX)
case "$staging" in
  /tmp/mise-bootstrap.?*)
    case "$staging" in
      *[[:space:]]*) ;;
      *) printf '%s\n' "$staging"; exit 0 ;;
    esac
    ;;
esac
case "$staging" in /tmp/mise-bootstrap.?*) rmdir "$staging" 2>/dev/null || true ;; esac
printf 'mktemp returned an unsafe staging path: %s\n' "$staging" >&2
exit 1"#
}

async fn run_staged(
    session: &SshSession<'_>,
    tar: &Path,
    staging: &str,
    options: &RemoteRunOptions,
    artifacts: &mut RemoteArtifactResolver,
) -> Result<()> {
    let project = format!("{staging}/project");
    session.status(&["mkdir", "-p", &project], false)?;
    upload_source(session, tar, &project)?;
    let mise = provision_mise(session, staging, &project, options.dry_run, artifacts).await?;
    let mut argv = vec![
        "env".to_string(),
        format!("MISE_TRUSTED_CONFIG_PATHS={project}"),
        mise,
        "--cd".to_string(),
        project,
        "bootstrap".to_string(),
    ];
    if options.dry_run {
        argv.push("--dry-run".to_string());
    }
    if options.yes {
        argv.push("--yes".to_string());
    }
    if options.update {
        argv.push("--update".to_string());
    }
    if options.prompt_secrets {
        argv.push("--prompt-secrets".to_string());
    }
    if options.force_dotfiles {
        argv.push("--force-dotfiles".to_string());
    }
    for part in &options.skip {
        argv.extend(["--skip".to_string(), part.clone()]);
    }
    for part in &options.only {
        argv.extend(["--only".to_string(), part.clone()]);
    }
    let argv = argv.iter().map(String::as_str).collect::<Vec<_>>();
    session.status(&argv, true)
}

fn upload_source(session: &SshSession<'_>, tar: &Path, project: &str) -> Result<()> {
    let temporary = tempfile::tempdir()?;
    let archive = temporary.path().join("source.tar.gz");
    let archive_source = if session.host.copy_links || session.host.copy_link.is_empty() {
        session.host.source.clone()
    } else {
        let source = temporary.path().join("source");
        fs::create_dir(&source)?;
        crate::file::copy_dir_all_preserve_symlinks(&session.host.source, &source)?;
        for link in &session.host.copy_link {
            materialize_link(&session.host.source, &source, link)?;
        }
        source
    };
    let mut command = Command::new(tar);
    command.args(["-czf"]);
    command.arg(&archive);
    if session.host.copy_links {
        command.arg("-h");
    }
    for exclude in &session.host.exclude {
        command.arg(format!("--exclude={exclude}"));
    }
    let status = command.args(["-C"]).arg(archive_source).arg(".").status()?;
    if !status.success() {
        bail!("failed to archive remote source with {status}");
    }
    session.status_with_stdin(&["tar", "-xzf", "-", "-C", project], File::open(archive)?)
}

fn validate_copy_link(source: &Path, link: &Path) -> Result<()> {
    validate_copy_link_path(source, link)?;
    let path = source.join(link);
    fs::metadata(&path)
        .wrap_err_with(|| format!("copy_link target does not exist: {}", link.display()))?;
    Ok(())
}

fn validate_copy_link_path(source: &Path, link: &Path) -> Result<()> {
    if link.as_os_str().is_empty()
        || link.is_absolute()
        || link.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        bail!(
            "copy_link must be a relative path within the source: {}",
            link.display()
        );
    }
    let mut parent = source.to_path_buf();
    for component in link.parent().unwrap_or_else(|| Path::new("")).components() {
        if let Component::Normal(component) = component {
            parent.push(component);
            let metadata = fs::symlink_metadata(&parent).wrap_err_with(|| {
                format!("copy_link parent does not exist: {}", parent.display())
            })?;
            if metadata.file_type().is_symlink() {
                bail!(
                    "copy_link cannot be nested below a symbolic link: {}",
                    link.display()
                );
            }
        }
    }
    let path = source.join(link);
    let metadata = fs::symlink_metadata(&path)
        .wrap_err_with(|| format!("copy_link does not exist: {}", link.display()))?;
    if !metadata.file_type().is_symlink() {
        bail!("copy_link is not a symbolic link: {}", link.display());
    }
    Ok(())
}

fn materialize_link(source: &Path, staged_source: &Path, link: &Path) -> Result<()> {
    validate_copy_link_path(staged_source, link)
        .wrap_err_with(|| format!("staged copy_link is unsafe: {}", link.display()))?;
    let source_link = source.join(link);
    let staged_link = staged_source.join(link);
    let target = fs::canonicalize(&source_link)
        .wrap_err_with(|| format!("failed to resolve copy_link {}", link.display()))?;
    let parent = staged_link
        .parent()
        .expect("a validated copy_link has a staged parent");
    let original_permissions = make_directory_writable(parent)?;
    let result = (|| {
        crate::file::remove_file(&staged_link)?;
        if target.is_dir() {
            fs::create_dir(&staged_link)?;
            crate::file::copy_dir_all_preserve_symlinks(&target, &staged_link)?;
        } else {
            crate::file::copy(&target, &staged_link)?;
        }
        Ok(())
    })();
    if let Some(permissions) = original_permissions {
        fs::set_permissions(parent, permissions).wrap_err_with(|| {
            format!(
                "failed to restore staged directory permissions: {}",
                parent.display()
            )
        })?;
    }
    result
}

fn make_directory_writable(path: &Path) -> Result<Option<fs::Permissions>> {
    let original = fs::metadata(path)?.permissions();
    let mut writable = original.clone();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if original.mode() & 0o200 != 0 {
            return Ok(None);
        }
        writable.set_mode(original.mode() | 0o200);
    }
    #[cfg(windows)]
    {
        if !original.readonly() {
            return Ok(None);
        }
        writable.set_readonly(false);
    }
    fs::set_permissions(path, writable).wrap_err_with(|| {
        format!(
            "failed to make staged directory writable: {}",
            path.display()
        )
    })?;
    Ok(Some(original))
}

async fn provision_mise(
    session: &SshSession<'_>,
    staging: &str,
    project: &str,
    dry_run: bool,
    artifacts: &mut RemoteArtifactResolver,
) -> Result<String> {
    if let Some(remote_mise) = &session.host.remote_mise {
        let remote_mise = resolve_configured_remote_mise(session, remote_mise, project)
            .wrap_err_with(|| {
                format!(
                    "remote host '{}' has an invalid remote_mise value",
                    session.host.name
                )
            })?;
        session.status(&[&remote_mise, "version"], false)?;
        return Ok(remote_mise);
    }
    if let Some(command) = &session.host.bootstrap_command {
        if dry_run {
            let mise = resolve_remote_mise(session).wrap_err_with(|| {
                format!(
                    "remote host '{}' has no existing mise executable; --dry-run does not execute bootstrap_command, so install mise first or set remote_mise or mise_bin",
                    session.host.name
                )
            })?;
            session.status(&[&mise, "version"], false)?;
            return Ok(mise);
        }
        let before = discover_remote_mise_candidates(session)?;
        let before_identities = remote_mise_candidate_identities(session, &before);
        let candidates_file = format!("{staging}/mise-candidates");
        let lookup = remote_mise_candidate_union_script();
        let script = format!(
            "set -e\n{command}\n{lookup} > {}",
            shell_quote(&candidates_file),
        );
        session.status(&["sh", "-lc", &script], true)?;
        let candidates = session.output(&["cat", &candidates_file])?;
        let candidates = parse_remote_mise_candidates(&candidates)?;
        let after_identities = remote_mise_candidate_identities(session, &candidates);
        let mise =
            select_bootstrapped_mise(&before, &before_identities, &candidates, &after_identities)?;
        session.status(&[&mise, "version"], false)?;
        return Ok(mise);
    }
    let binary = if let Some(binary) = &session.host.mise_bin {
        binary.clone()
    } else {
        let platform = detect_remote_platform(session)?;
        let local_os = normalize_os(std::env::consts::OS);
        let local_arch = normalize_arch(std::env::consts::ARCH);
        let local = std::env::current_exe()?;
        let local_incompatibility = if platform.os != local_os || platform.arch != local_arch {
            Some(format!(
                "local mise is {local_os}/{local_arch}, while the remote target is {}",
                platform.description()
            ))
        } else {
            validate_default_binary_compatibility(session, &local, &platform.os)
                .err()
                .map(|error| format!("{error:#}"))
        };
        if local_incompatibility.is_none() {
            local
        } else {
            artifacts
                .resolve(&platform, &local)
                .await
                .wrap_err_with(|| {
                    format!(
                        "local mise could not run on remote host '{}' ({}) because {}; official mise {} artifact fallback also failed",
                        session.host.name,
                        platform.description(),
                        local_incompatibility.expect("incompatibility was identified"),
                        env!("CARGO_PKG_VERSION"),
                    )
                })?
        }
    };
    let remote = format!("{staging}/mise");
    session.upload_executable(&binary, &remote)?;
    session.status(&[&remote, "version"], false).wrap_err_with(|| {
        format!(
            "uploaded local mise cannot run on remote host '{}'; set mise_bin, remote_mise, or bootstrap_command",
            session.host.name
        )
    })?;
    Ok(remote)
}

fn resolve_remote_mise(session: &SshSession<'_>) -> Result<String> {
    let script = remote_mise_output_script();
    let mise = session.output(&["sh", "-lc", &script])?;
    validated_remote_command_output(&mise)
}

fn resolve_configured_remote_mise(
    session: &SshSession<'_>,
    command: &str,
    project: &str,
) -> Result<String> {
    validate_remote_executable(command)?;
    if !command.contains('/') {
        return resolve_login_path_executable(session, command);
    }
    let remote_home = if command.starts_with("~/") {
        let output = session.output(&["sh", "-lc", "printf '%s\\n' \"$HOME\""])?;
        Some(validated_absolute_remote_path_output(
            &output,
            "remote login home",
        )?)
    } else {
        None
    };
    resolve_remote_mise_path(command, project, remote_home.as_deref())
}

fn resolve_login_path_executable(session: &SshSession<'_>, command: &str) -> Result<String> {
    let script = format!(
        r#"command_path=$(command -v {} 2>/dev/null || true)
case "$command_path" in
  /*) printf '%s\n' "$command_path" ;;
  *) printf 'executable not found on remote login PATH: %s\n' {} >&2; exit 127 ;;
esac"#,
        shell_quote(command),
        shell_quote(command),
    );
    let output = session.output(&["sh", "-lc", &script])?;
    validated_absolute_remote_path_output(&output, "remote login executable")
}

fn resolve_remote_mise_path(
    command: &str,
    project: &str,
    remote_home: Option<&str>,
) -> Result<String> {
    if command.starts_with('/') {
        return Ok(command.to_string());
    }
    if let Some(suffix) = command.strip_prefix("~/") {
        if suffix.is_empty() {
            bail!("remote mise path does not name an executable: {command:?}");
        }
        let home = remote_home.ok_or_else(|| eyre!("remote login home was not resolved"))?;
        return Ok(format!("{}/{suffix}", home.trim_end_matches('/')));
    }

    let mut components = Vec::new();
    for component in command.split('/') {
        match component {
            "" | "." => {}
            ".." => {
                if components.pop().is_none() {
                    bail!("relative remote mise path escapes the staged project: {command:?}");
                }
            }
            component => components.push(component),
        }
    }
    if components.is_empty() {
        bail!("relative remote mise path does not name an executable: {command:?}");
    }
    Ok(format!("{project}/{}", components.join("/")))
}

fn remote_mise_find_script() -> &'static str {
    r#"mise_path=$(command -v mise 2>/dev/null || true)
case "$mise_path" in /*) ;; *) mise_path= ;; esac
if [ -z "$mise_path" ]; then
  for candidate in "$HOME/.local/bin/mise" "$HOME/.local/share/mise/bin/mise" "$HOME/.cargo/bin/mise" /usr/local/bin/mise /opt/homebrew/bin/mise; do
    if [ -x "$candidate" ]; then
      mise_path=$candidate
      break
    fi
  done
fi"#
}

fn remote_mise_output_script() -> String {
    format!(
        "{}\nif [ -z \"$mise_path\" ]; then\n  echo \"mise executable not found after bootstrap_command\" >&2\n  exit 127\nfi\nprintf '%s\\n' \"$mise_path\"",
        remote_mise_find_script()
    )
}

fn discover_remote_mise_candidates(session: &SshSession<'_>) -> Result<Vec<String>> {
    let script = remote_mise_candidate_union_script();
    let output = session.output(&["sh", "-lc", &script])?;
    parse_remote_mise_candidates(&output)
}

fn remote_mise_candidate_union_script() -> String {
    let lookup = remote_mise_candidates_script();
    let fresh_login_lookup = shell_words::join(["sh", "-lc", lookup]);
    format!("{{\n{lookup}\n{fresh_login_lookup}\n}}")
}

fn remote_mise_candidates_script() -> &'static str {
    r#"set -f
emit_mise_candidate() {
  case "$1" in
    /*) if [ -x "$1" ]; then printf '%s\000' "$1"; fi ;;
  esac
}
old_ifs=$IFS
IFS=:
for directory in $PATH; do
  if [ -z "$directory" ]; then directory=.; fi
  emit_mise_candidate "$directory/mise"
done
IFS=$old_ifs
for candidate in "$HOME/.local/bin/mise" "$HOME/.local/share/mise/bin/mise" "$HOME/.cargo/bin/mise" /usr/local/bin/mise /opt/homebrew/bin/mise; do
  emit_mise_candidate "$candidate"
done"#
}

fn parse_remote_mise_candidates(output: &str) -> Result<Vec<String>> {
    if !output.is_empty() && !output.ends_with('\0') {
        bail!("remote mise candidate discovery returned a truncated response");
    }
    output
        .split_terminator('\0')
        .map(validated_remote_command)
        .collect::<Result<IndexSet<_>>>()
        .map(IndexSet::into_iter)
        .map(Iterator::collect)
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RemoteMiseIdentity {
    version: String,
    fingerprint: String,
}

fn remote_mise_candidate_identities(
    session: &SshSession<'_>,
    candidates: &[String],
) -> IndexMap<String, Option<RemoteMiseIdentity>> {
    candidates
        .iter()
        .map(|candidate| {
            let identity = session
                .output(&[candidate, "version"])
                .and_then(|version| {
                    remote_mise_fingerprint(session, candidate).map(|fingerprint| {
                        RemoteMiseIdentity {
                            version: version.trim().to_string(),
                            fingerprint,
                        }
                    })
                })
                .ok();
            (candidate.clone(), identity)
        })
        .collect()
}

fn remote_mise_fingerprint(session: &SshSession<'_>, candidate: &str) -> Result<String> {
    let candidate = shell_quote(candidate);
    let script = format!(
        "if command -v sha256sum >/dev/null 2>&1; then sha256sum {candidate}; elif command -v shasum >/dev/null 2>&1; then shasum -a 256 {candidate}; else cksum {candidate}; fi"
    );
    Ok(session.output(&["sh", "-c", &script])?.trim().to_string())
}

fn select_bootstrapped_mise(
    before: &[String],
    before_identities: &IndexMap<String, Option<RemoteMiseIdentity>>,
    after: &[String],
    after_identities: &IndexMap<String, Option<RemoteMiseIdentity>>,
) -> Result<String> {
    let new_candidates = after
        .iter()
        .filter(|candidate| !before.contains(candidate))
        .collect::<Vec<_>>();
    if let Some(candidate) = select_unique_remote_mise_candidate(new_candidates, "new")? {
        return Ok(candidate);
    }
    let changed_candidates = after
        .iter()
        .filter(|candidate| {
            before_identities.get(*candidate) != after_identities.get(*candidate)
                && after_identities
                    .get(*candidate)
                    .is_some_and(Option::is_some)
        })
        .collect::<Vec<_>>();
    if let Some(candidate) = select_unique_remote_mise_candidate(changed_candidates, "changed")? {
        return Ok(candidate);
    }
    let version_candidates = after
        .iter()
        .filter(|candidate| {
            after_identities
                .get(*candidate)
                .and_then(Option::as_ref)
                .and_then(|identity| identity.version.split_whitespace().next())
                .is_some_and(|version| {
                    version == env!("CARGO_PKG_VERSION")
                        || version == concat!(env!("CARGO_PKG_VERSION"), "-DEBUG")
                })
        })
        .collect::<Vec<_>>();
    if let Some(candidate) =
        select_unique_remote_mise_candidate(version_candidates, "version-matching")?
    {
        return Ok(candidate);
    }
    if let [candidate] = after {
        return Ok(candidate.clone());
    }
    if after.is_empty() {
        bail!("mise executable not found after bootstrap_command");
    }
    bail!(
        "bootstrap_command left multiple unchanged mise executables and the installed path is ambiguous: {}; set remote_mise or mise_bin explicitly",
        after.join(", ")
    )
}

fn select_unique_remote_mise_candidate(
    candidates: Vec<&String>,
    kind: &str,
) -> Result<Option<String>> {
    match candidates.as_slice() {
        [] => Ok(None),
        [candidate] => Ok(Some((*candidate).clone())),
        _ => bail!(
            "bootstrap_command left multiple {kind} mise executables and the installed path is ambiguous: {}; set remote_mise or mise_bin explicitly",
            candidates
                .iter()
                .map(|candidate| candidate.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LibcFlavor {
    Glibc,
    Musl,
}

impl RemotePlatform {
    fn description(&self) -> String {
        match self.libc {
            Some(libc) => format!("{}/{}/{libc}", self.os, self.arch),
            None => format!("{}/{}", self.os, self.arch),
        }
    }

    fn release_asset_name(&self) -> Result<String> {
        release_asset_name(&self.os, &self.arch, self.libc)
    }
}

impl fmt::Display for LibcFlavor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Glibc => f.write_str("glibc"),
            Self::Musl => f.write_str("musl"),
        }
    }
}

impl ReleaseManifest {
    fn verified(contents: &str, signature: &str) -> Result<Self> {
        crate::minisign::verify(
            &crate::minisign::MISE_PUB_KEY,
            contents.as_bytes(),
            signature,
        )
        .wrap_err("mise release checksum signature is invalid")?;
        let checksums = crate::hash::parse_shasums(contents);
        if checksums.is_empty() {
            bail!("signed mise release checksum manifest is empty");
        }
        if checksums.values().any(|checksum| {
            checksum.len() != 64 || !checksum.bytes().all(|byte| byte.is_ascii_hexdigit())
        }) {
            bail!("signed mise release checksum manifest contains an invalid SHA-256 checksum");
        }
        Ok(Self { checksums })
    }

    fn checksum(&self, asset: &str) -> Result<&str> {
        self.checksums
            .get(asset)
            .or_else(|| self.checksums.get(&format!("./{asset}")))
            .map(String::as_str)
            .ok_or_else(|| eyre!("signed mise release manifest does not contain {asset}"))
    }
}

impl RemoteArtifactResolver {
    async fn resolve(&mut self, platform: &RemotePlatform, local: &Path) -> Result<PathBuf> {
        let asset = platform.release_asset_name()?;
        if let Some(path) = self.artifacts.get(&asset) {
            return Ok(path.clone());
        }
        self.ensure_official_local(local).await?;
        let checksum = self.manifest().await?.checksum(&asset)?.to_string();
        if self.directory.is_none() {
            self.directory = Some(tempfile::tempdir()?);
        }
        let path = self
            .directory
            .as_ref()
            .expect("artifact directory was initialized")
            .path()
            .join(&asset);
        let progress = MultiProgressReport::get().add("mise bootstrap");
        progress.set_message(format!("downloading {asset}"));
        if let Err(error) = HTTP
            .download_file(release_url(&asset), &path, Some(progress.as_ref()))
            .await
        {
            progress.abandon();
            return Err(error).wrap_err_with(|| {
                format!("failed to download official mise release artifact {asset}")
            });
        }
        progress.set_message(format!("verifying {asset}"));
        if let Err(error) =
            crate::hash::ensure_checksum(&path, &checksum, Some(progress.as_ref()), "sha256")
        {
            progress.abandon();
            return Err(error).wrap_err_with(|| {
                format!("official mise release artifact {asset} failed verification")
            });
        }
        progress.finish();
        info!(
            "using signed official mise {} artifact {asset}",
            env!("CARGO_PKG_VERSION")
        );
        self.artifacts.insert(asset, path.clone());
        Ok(path)
    }

    async fn ensure_official_local(&mut self, local: &Path) -> Result<()> {
        if self.official_local_verified {
            return Ok(());
        }
        if cfg!(debug_assertions) {
            bail!(
                "automatic cross-platform provisioning is unavailable from a debug mise build; set mise_bin, remote_mise, or bootstrap_command"
            );
        }
        let local_os = normalize_os(std::env::consts::OS);
        let local_arch = normalize_arch(std::env::consts::ARCH);
        let candidates = official_release_assets(&local_os, &local_arch)?;
        let actual = crate::hash::file_hash_sha256(local, None)?;
        let manifest = self.manifest().await?;
        let official = candidates.iter().any(|asset| {
            manifest
                .checksum(asset)
                .is_ok_and(|expected| expected.eq_ignore_ascii_case(&actual))
        });
        if !official {
            bail!(
                "automatic cross-platform provisioning refuses to replace a custom mise build with an official binary because {} does not match the signed mise {} release checksums; set mise_bin, remote_mise, or bootstrap_command",
                local.display(),
                env!("CARGO_PKG_VERSION")
            );
        }
        self.official_local_verified = true;
        Ok(())
    }

    async fn manifest(&mut self) -> Result<&ReleaseManifest> {
        if self.manifest.is_none() {
            let manifest_url = release_url("SHASUMS256.txt");
            let signature_url = release_url("SHASUMS256.txt.minisig");
            let (contents, signature) = tokio::try_join!(
                HTTP.get_text_cached(&manifest_url),
                HTTP.get_text_cached(&signature_url)
            )
            .wrap_err_with(|| {
                format!(
                    "failed to fetch signed mise {} release checksums",
                    env!("CARGO_PKG_VERSION")
                )
            })?;
            self.manifest = Some(ReleaseManifest::verified(&contents, &signature)?);
        }
        Ok(self.manifest.as_ref().expect("release manifest was loaded"))
    }
}

fn release_url(filename: &str) -> String {
    let version = env!("CARGO_PKG_VERSION");
    format!("{RELEASE_BASE_URL}/v{version}/{filename}")
}

fn release_asset_name(os: &str, arch: &str, libc: Option<LibcFlavor>) -> Result<String> {
    let release_arch = match arch {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        "armv7" => "armv7",
        _ => {
            bail!(
                "mise {} has no official precompiled artifact for {os}/{arch}; set mise_bin, remote_mise, or bootstrap_command",
                env!("CARGO_PKG_VERSION")
            )
        }
    };
    let suffix = match os {
        "macos" if matches!(arch, "x86_64" | "aarch64") => {
            if libc.is_some() {
                bail!("macOS release targets cannot declare a libc family");
            }
            format!("macos-{release_arch}")
        }
        "linux" if matches!(arch, "x86_64" | "aarch64" | "armv7") => match libc {
            Some(LibcFlavor::Glibc) => format!("linux-{release_arch}"),
            Some(LibcFlavor::Musl) => format!("linux-{release_arch}-musl"),
            None => bail!("Linux release targets require a detected libc family"),
        },
        _ => {
            bail!(
                "mise {} has no official precompiled artifact for {os}/{arch}; set mise_bin, remote_mise, or bootstrap_command",
                env!("CARGO_PKG_VERSION")
            )
        }
    };
    Ok(format!("mise-v{}-{suffix}", env!("CARGO_PKG_VERSION")))
}

fn official_release_assets(os: &str, arch: &str) -> Result<Vec<String>> {
    match os {
        "linux" => [LibcFlavor::Glibc, LibcFlavor::Musl]
            .into_iter()
            .map(|libc| release_asset_name(os, arch, Some(libc)))
            .collect(),
        "macos" => Ok(vec![release_asset_name(os, arch, None)?]),
        "windows" => {
            let release_arch = match arch {
                "x86_64" => "x64",
                "aarch64" => "arm64",
                _ => {
                    bail!(
                        "mise {} has no official precompiled artifact for {os}/{arch}; set mise_bin, remote_mise, or bootstrap_command",
                        env!("CARGO_PKG_VERSION")
                    )
                }
            };
            Ok(vec![format!(
                "mise-v{}-windows-{release_arch}.exe",
                env!("CARGO_PKG_VERSION")
            )])
        }
        _ => bail!(
            "mise {} has no official precompiled artifact for {os}/{arch}; set mise_bin, remote_mise, or bootstrap_command",
            env!("CARGO_PKG_VERSION")
        ),
    }
}

fn detect_remote_platform(session: &SshSession<'_>) -> Result<RemotePlatform> {
    let output = session.output(&["sh", "-c", remote_platform_script()])?;
    parse_remote_platform(&output).wrap_err_with(|| {
        format!(
            "could not detect the platform for remote host '{}'; set mise_bin, remote_mise, or bootstrap_command",
            session.host.name
        )
    })
}

fn remote_platform_script() -> &'static str {
    r#"os=$(uname -s)
arch=$(uname -m)
printf '%s\n%s\n' "$os" "$arch"
case "$os" in
  Linux|linux)
    libc=
    if command -v getconf >/dev/null 2>&1; then
      libc_output=$(getconf GNU_LIBC_VERSION 2>&1 || true)
      case "$libc_output" in *glibc*|*GLIBC*) libc=glibc ;; esac
    fi
    if [ -z "$libc" ] && command -v ldd >/dev/null 2>&1; then
      libc_output=$(ldd --version 2>&1 || true)
      case "$libc_output" in
        *musl*|*MUSL*) libc=musl ;;
        *glibc*|*GLIBC*|*GNU\ libc*|*GNU\ C\ Library*) libc=glibc ;;
      esac
    fi
    if [ -z "$libc" ]; then
      for loader in /lib/ld-musl-*.so.1 /usr/lib/ld-musl-*.so.1; do
        if [ -e "$loader" ]; then libc=musl; break; fi
      done
    fi
    printf '%s\n' "${libc:-unknown}"
    ;;
  *) printf '%s\n' none ;;
esac"#
}

fn parse_remote_platform(output: &str) -> Result<RemotePlatform> {
    let mut lines = output.lines();
    let os = normalize_os(lines.next().unwrap_or_default());
    let arch = normalize_arch(lines.next().unwrap_or_default());
    let libc = match (os.as_str(), lines.next()) {
        ("linux", Some("glibc")) => Some(LibcFlavor::Glibc),
        ("linux", Some("musl")) => Some(LibcFlavor::Musl),
        ("linux", _) => bail!("remote Linux libc family could not be identified"),
        (_, Some("none")) => None,
        _ => bail!("remote platform response is incomplete"),
    };
    if os.is_empty() || arch.is_empty() || lines.next().is_some() {
        bail!("remote platform response is invalid");
    }
    Ok(RemotePlatform { os, arch, libc })
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct AbiVersion(Vec<u32>);

impl fmt::Display for AbiVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(
            &self
                .0
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join("."),
        )
    }
}

fn validate_default_binary_compatibility(
    session: &SshSession<'_>,
    binary: &Path,
    remote_os: &str,
) -> Result<()> {
    if remote_os != "linux" {
        return Ok(());
    }
    let binary_bytes = std::fs::read(binary)?;
    let Some(interpreter) = elf_interpreter(&binary_bytes)? else {
        // A static ELF has no runtime loader or libc compatibility constraint.
        return Ok(());
    };
    let local_output = Command::new(&interpreter).arg("--version").output()?;
    let local_libc = parse_libc_flavor(&combined_output(&local_output)).ok_or_else(|| {
        eyre!(
            "could not identify the libc used by local mise loader {interpreter}; set mise_bin, remote_mise, or bootstrap_command"
        )
    })?;
    let check = format!(
        "if test -x {loader}; then {loader} --version 2>&1 || true; else printf '%s\\n' MISE_LOADER_MISSING; fi",
        loader = shell_quote(&interpreter),
    );
    let remote_output = session.output(&["sh", "-c", &check])?;
    if remote_output
        .lines()
        .any(|line| line == "MISE_LOADER_MISSING")
    {
        bail!(
            "remote host '{}' does not provide the dynamic loader required by local mise: {interpreter}; set mise_bin, remote_mise, or bootstrap_command",
            session.host.name
        );
    }
    let remote_libc = parse_libc_flavor(&remote_output).ok_or_else(|| {
        eyre!(
            "could not identify the libc provided by remote loader {interpreter} on '{}'; set mise_bin, remote_mise, or bootstrap_command",
            session.host.name
        )
    })?;
    if local_libc != remote_libc {
        bail!(
            "remote host '{}' libc {remote_libc:?} is incompatible with local mise libc {local_libc:?}; set mise_bin, remote_mise, or bootstrap_command",
            session.host.name
        );
    }
    let required = match local_libc {
        LibcFlavor::Glibc => max_required_glibc_version(&binary_bytes).ok_or_else(|| {
            eyre!(
                "could not determine the glibc ABI required by local mise; set mise_bin, remote_mise, or bootstrap_command"
            )
        })?,
        LibcFlavor::Musl => parse_musl_runtime_version(&combined_output(&local_output))
            .ok_or_else(|| {
                eyre!(
                    "could not determine the musl version used by local mise loader {interpreter}; set mise_bin, remote_mise, or bootstrap_command"
                )
            })?,
    };
    let available = match remote_libc {
        LibcFlavor::Glibc => parse_glibc_runtime_version(&remote_output),
        LibcFlavor::Musl => parse_musl_runtime_version(&remote_output),
    }
    .ok_or_else(|| {
        eyre!(
            "could not determine the {remote_libc:?} version provided by remote loader {interpreter} on '{}'; set mise_bin, remote_mise, or bootstrap_command",
            session.host.name
        )
    })?;
    if available < required {
        bail!(
            "remote host '{}' provides {remote_libc:?} {available}, but local mise requires {remote_libc:?} {required}; set mise_bin, remote_mise, or bootstrap_command",
            session.host.name
        );
    }
    Ok(())
}

fn max_required_glibc_version(bytes: &[u8]) -> Option<AbiVersion> {
    const PREFIX: &[u8] = b"GLIBC_";
    bytes
        .windows(PREFIX.len())
        .enumerate()
        .filter_map(|(offset, window)| {
            (window == PREFIX)
                .then(|| parse_abi_version(&bytes[offset + PREFIX.len()..]))
                .flatten()
        })
        .max()
}

fn parse_glibc_runtime_version(output: &str) -> Option<AbiVersion> {
    let lower = output.to_ascii_lowercase();
    ["glibc", "gnu libc"].into_iter().find_map(|marker| {
        lower.match_indices(marker).find_map(|(offset, _)| {
            let suffix = &output.as_bytes()[offset + marker.len()..];
            let digit = suffix.iter().position(u8::is_ascii_digit)?;
            parse_abi_version(&suffix[digit..])
        })
    })
}

fn parse_musl_runtime_version(output: &str) -> Option<AbiVersion> {
    output.lines().find_map(|line| {
        let line = line.trim();
        let suffix = line
            .get(..7)
            .filter(|prefix| prefix.eq_ignore_ascii_case("version"))
            .and_then(|_| line.get(7..))?;
        if !suffix.starts_with([' ', '\t', ':']) {
            return None;
        }
        let digit = suffix.as_bytes().iter().position(u8::is_ascii_digit)?;
        parse_abi_version(&suffix.as_bytes()[digit..])
    })
}

fn parse_abi_version(bytes: &[u8]) -> Option<AbiVersion> {
    let mut components = Vec::new();
    let mut offset = 0;
    loop {
        let start = offset;
        while bytes.get(offset).is_some_and(u8::is_ascii_digit) {
            offset += 1;
        }
        if start == offset {
            break;
        }
        let component = std::str::from_utf8(&bytes[start..offset])
            .ok()?
            .parse()
            .ok()?;
        components.push(component);
        if bytes.get(offset) != Some(&b'.') {
            break;
        }
        offset += 1;
    }
    (!components.is_empty()).then_some(AbiVersion(components))
}

fn elf_interpreter(bytes: &[u8]) -> Result<Option<String>> {
    const ELF_MAGIC: &[u8] = b"\x7fELF";
    const PT_INTERP: u64 = 3;
    if !bytes.starts_with(ELF_MAGIC) {
        bail!("local mise is not an ELF executable");
    }
    let class = *bytes.get(4).ok_or_else(|| eyre!("truncated ELF header"))?;
    let little_endian = match bytes.get(5) {
        Some(1) => true,
        Some(2) => false,
        _ => bail!("unsupported ELF byte order"),
    };
    let (program_offset, entry_size, entry_count, offset_field, size_field) = match class {
        1 => (
            read_elf_int(bytes, 28, 4, little_endian)?,
            read_elf_int(bytes, 42, 2, little_endian)?,
            read_elf_int(bytes, 44, 2, little_endian)?,
            4,
            16,
        ),
        2 => (
            read_elf_int(bytes, 32, 8, little_endian)?,
            read_elf_int(bytes, 54, 2, little_endian)?,
            read_elf_int(bytes, 56, 2, little_endian)?,
            8,
            32,
        ),
        _ => bail!("unsupported ELF class"),
    };
    let program_offset = usize::try_from(program_offset)?;
    let entry_size = usize::try_from(entry_size)?;
    let entry_count = usize::try_from(entry_count)?;
    for index in 0..entry_count {
        let start = program_offset
            .checked_add(
                index
                    .checked_mul(entry_size)
                    .ok_or_else(|| eyre!("invalid ELF program headers"))?,
            )
            .ok_or_else(|| eyre!("invalid ELF program headers"))?;
        if read_elf_int(bytes, start, 4, little_endian)? != PT_INTERP {
            continue;
        }
        let offset = usize::try_from(read_elf_int(
            bytes,
            start + offset_field,
            if class == 1 { 4 } else { 8 },
            little_endian,
        )?)?;
        let size = usize::try_from(read_elf_int(
            bytes,
            start + size_field,
            if class == 1 { 4 } else { 8 },
            little_endian,
        )?)?;
        let value = bytes
            .get(
                offset
                    ..offset
                        .checked_add(size)
                        .ok_or_else(|| eyre!("invalid ELF interpreter"))?,
            )
            .ok_or_else(|| eyre!("truncated ELF interpreter"))?;
        let value = value.strip_suffix(&[0]).unwrap_or(value);
        let interpreter = String::from_utf8(value.to_vec())?;
        if !interpreter.starts_with('/') || interpreter.contains('\0') || interpreter.contains('\n')
        {
            bail!("unsafe ELF interpreter path: {interpreter:?}");
        }
        return Ok(Some(interpreter));
    }
    Ok(None)
}

fn read_elf_int(bytes: &[u8], offset: usize, size: usize, little_endian: bool) -> Result<u64> {
    let value = bytes
        .get(
            offset
                ..offset
                    .checked_add(size)
                    .ok_or_else(|| eyre!("invalid ELF field"))?,
        )
        .ok_or_else(|| eyre!("truncated ELF field"))?;
    let mut padded = [0_u8; 8];
    if little_endian {
        padded[..size].copy_from_slice(value);
        Ok(u64::from_le_bytes(padded))
    } else {
        padded[8 - size..].copy_from_slice(value);
        Ok(u64::from_be_bytes(padded))
    }
}

fn combined_output(output: &Output) -> String {
    format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn parse_libc_flavor(output: &str) -> Option<LibcFlavor> {
    let lower = output.to_ascii_lowercase();
    if lower.contains("musl") {
        Some(LibcFlavor::Musl)
    } else if lower.contains("glibc") || lower.contains("gnu libc") {
        Some(LibcFlavor::Glibc)
    } else {
        None
    }
}

struct SshSession<'a> {
    ssh: PathBuf,
    host: &'a RemoteHost,
    connect_timeout: u16,
    control_path: Option<PathBuf>,
}

impl SshSession<'_> {
    fn args(&self, tty: bool, remote_argv: &[&str]) -> Vec<String> {
        let mut args = vec![
            "-o".to_string(),
            format!("ConnectTimeout={}", self.connect_timeout),
        ];
        if let Some(control_path) = &self.control_path {
            args.extend([
                "-o".to_string(),
                "ControlMaster=auto".to_string(),
                "-o".to_string(),
                "ControlPersist=60".to_string(),
                "-o".to_string(),
                format!("ControlPath={}", control_path.display()),
            ]);
        }
        if !console::user_attended_stderr() {
            args.extend(["-o".to_string(), "BatchMode=yes".to_string()]);
        }
        if tty && console::user_attended_stderr() {
            args.push("-tt".to_string());
        }
        if let Some(port) = self.host.port {
            args.extend(["-p".to_string(), port.to_string()]);
        }
        if let Some(identity) = &self.host.identity_file {
            args.push("-i".to_string());
            args.push(identity.to_string_lossy().to_string());
        }
        for option in &self.host.ssh_options {
            args.extend(["-o".to_string(), option.clone()]);
        }
        args.push(self.host.destination());
        args.push(shell_words::join(remote_argv));
        args
    }

    fn output(&self, remote_argv: &[&str]) -> Result<String> {
        let args = self.args(false, remote_argv);
        info!("$ {} {}", self.ssh.display(), shell_words::join(&args));
        let output = Command::new(&self.ssh).args(&args).output()?;
        checked_output(output, &self.host.name)
    }

    fn status(&self, remote_argv: &[&str], tty: bool) -> Result<()> {
        let args = self.args(tty, remote_argv);
        info!("$ {} {}", self.ssh.display(), shell_words::join(&args));
        let status = Command::new(&self.ssh).args(args).status()?;
        if !status.success() {
            bail!(
                "remote command on '{}' failed with {status}",
                self.host.name
            );
        }
        Ok(())
    }

    fn status_with_stdin(&self, remote_argv: &[&str], input: File) -> Result<()> {
        let args = self.args(false, remote_argv);
        info!("$ {} {}", self.ssh.display(), shell_words::join(&args));
        let status = Command::new(&self.ssh)
            .args(args)
            .stdin(Stdio::from(input))
            .status()?;
        if !status.success() {
            bail!("remote upload to '{}' failed with {status}", self.host.name);
        }
        Ok(())
    }

    fn upload_executable(&self, local: &Path, remote: &str) -> Result<()> {
        let file = File::open(local)
            .wrap_err_with(|| format!("failed to open mise binary {}", local.display()))?;
        self.status_with_stdin(
            &[
                "sh",
                "-c",
                &format!(
                    "cat > {} && chmod 700 {}",
                    shell_quote(remote),
                    shell_quote(remote)
                ),
            ],
            file,
        )
    }

    fn close(&self) {
        let Some(control_path) = &self.control_path else {
            return;
        };
        let status = Command::new(&self.ssh)
            .args(["-S"])
            .arg(control_path)
            .args(["-O", "exit"])
            .arg(self.host.destination())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        if status.is_ok_and(|status| !status.success()) {
            debug!(
                "SSH control connection for {} was already closed",
                self.host.name
            );
        }
    }
}

impl Drop for SshSession<'_> {
    fn drop(&mut self) {
        self.close();
    }
}

fn checked_output(output: Output, name: &str) -> Result<String> {
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "remote command on '{name}' failed with {}: {}",
            output.status,
            stderr.trim()
        );
    }
    Ok(String::from_utf8(output.stdout)?)
}

fn resolve_local_path(base: &Path, path: Option<&Path>) -> Result<Option<PathBuf>> {
    path.map(|path| {
        let path = crate::file::replace_path(path);
        let path = if path.is_absolute() {
            path
        } else {
            base.join(path)
        };
        absolutize(&path)
    })
    .transpose()
}

fn absolutize(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

fn validate_ssh_atom(kind: &str, value: &str) -> Result<()> {
    validate_value(kind, value)?;
    if value.starts_with('-') || value.chars().any(char::is_whitespace) {
        bail!("invalid remote {kind}: {value}");
    }
    Ok(())
}

fn validate_value(kind: &str, value: &str) -> Result<()> {
    if value.is_empty() || value.contains('\0') {
        bail!("remote {kind} cannot be empty or contain NUL");
    }
    Ok(())
}

fn validate_staging_path(path: &str) -> Result<()> {
    if !path.starts_with("/tmp/mise-bootstrap.")
        || path["/tmp/mise-bootstrap.".len()..].is_empty()
        || path.chars().any(char::is_whitespace)
    {
        bail!("remote mktemp returned an unsafe staging path: {path:?}");
    }
    Ok(())
}

fn validated_remote_command(command: &str) -> Result<String> {
    if !command.starts_with('/') || command.contains(['\0', '\n', '\r']) {
        bail!("bootstrap_command returned an unsafe mise path: {command:?}");
    }
    Ok(command.to_string())
}

fn validated_remote_command_output(output: &str) -> Result<String> {
    validated_remote_command(output.strip_suffix('\n').unwrap_or(output))
}

fn validated_absolute_remote_path_output(output: &str, kind: &str) -> Result<String> {
    let path = output.strip_suffix('\n').unwrap_or(output);
    if !path.starts_with('/') || path.contains(['\0', '\n', '\r']) {
        bail!("{kind} returned an unsafe absolute path: {path:?}");
    }
    Ok(path.to_string())
}

fn validate_remote_executable(command: &str) -> Result<()> {
    validate_value("mise command", command)?;
    let is_path = command.contains('/');
    if command.starts_with('-')
        || command.contains(['\n', '\r'])
        || (!is_path && command.chars().any(char::is_whitespace))
    {
        bail!("remote mise command must be an executable name or path: {command:?}");
    }
    Ok(())
}

fn shell_quote(value: &str) -> String {
    shell_words::join([value])
}

fn normalize_os(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "darwin" | "macos" => "macos".to_string(),
        "linux" => "linux".to_string(),
        other => other.to_string(),
    }
}

fn normalize_arch(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "x86_64" | "amd64" => "x86_64".to_string(),
        "aarch64" | "arm64" => "aarch64".to_string(),
        "armv7l" | "armv7" => "armv7".to_string(),
        "armv6l" | "armv6" => "armv6".to_string(),
        other => other.to_string(),
    }
}

fn default_excludes() -> Vec<String> {
    vec![
        ".git".to_string(),
        "target".to_string(),
        "node_modules".to_string(),
    ]
}

fn dedupe(values: Vec<String>) -> Vec<String> {
    values
        .into_iter()
        .collect::<IndexSet<_>>()
        .into_iter()
        .collect()
}

fn dedupe_paths(values: Vec<PathBuf>) -> Vec<PathBuf> {
    values
        .into_iter()
        .map(|path| {
            path.components()
                .filter(|component| !matches!(component, Component::CurDir))
                .collect()
        })
        .collect::<IndexSet<_>>()
        .into_iter()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(unix)]
    fn materializes_only_the_selected_link() -> Result<()> {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir()?;
        let source = temp.path().join("source");
        let staged = temp.path().join("staged");
        let shared = temp.path().join("shared");
        fs::create_dir_all(shared.join("nested"))?;
        fs::write(shared.join("module.toml"), "module")?;
        symlink("../module.toml", shared.join("nested/module-link"))?;
        fs::create_dir_all(&source)?;
        symlink(&shared, source.join("shared"))?;

        fs::create_dir(&staged)?;
        crate::file::copy_dir_all_preserve_symlinks(&source, &staged)?;
        materialize_link(&source, &staged, Path::new("shared"))?;

        assert!(staged.join("shared").is_dir());
        assert!(!staged.join("shared").is_symlink());
        assert_eq!(
            fs::read_to_string(staged.join("shared/module.toml"))?,
            "module"
        );
        assert_eq!(
            fs::read_link(staged.join("shared/nested/module-link"))?,
            Path::new("../module.toml")
        );
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn validates_copy_link_paths() -> Result<()> {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir()?;
        let source = temp.path().join("source");
        fs::create_dir(&source)?;
        fs::write(temp.path().join("target"), "target")?;
        fs::write(source.join("regular"), "regular")?;
        symlink(temp.path().join("target"), source.join("link"))?;
        symlink("missing", source.join("dangling"))?;

        assert!(validate_copy_link(&source, Path::new("link")).is_ok());
        assert!(validate_copy_link(&source, Path::new("regular")).is_err());
        assert!(validate_copy_link(&source, Path::new("dangling")).is_err());
        assert!(validate_copy_link(&source, Path::new("../target")).is_err());
        assert!(validate_copy_link(&source, &temp.path().join("target")).is_err());
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn rejects_copy_link_below_symlinked_parent() -> Result<()> {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir()?;
        let source = temp.path().join("source");
        let outside = temp.path().join("outside");
        fs::create_dir(&source)?;
        fs::create_dir(&outside)?;
        fs::write(temp.path().join("target"), "target")?;
        symlink(temp.path().join("target"), outside.join("child-link"))?;
        symlink(&outside, source.join("linked-parent"))?;

        let error = validate_copy_link(&source, Path::new("linked-parent/child-link"))
            .expect_err("a selected link below a symlinked parent must be rejected");
        assert!(error.to_string().contains("nested below a symbolic link"));
        assert!(outside.join("child-link").is_symlink());
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn rejects_symlinked_parent_introduced_in_staging() -> Result<()> {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir()?;
        let source = temp.path().join("source");
        let staged = temp.path().join("staged");
        let outside = temp.path().join("outside");
        fs::create_dir_all(source.join("parent"))?;
        fs::create_dir(&staged)?;
        fs::create_dir(&outside)?;
        fs::write(temp.path().join("target"), "target")?;
        symlink(temp.path().join("target"), source.join("parent/link"))?;
        symlink(temp.path().join("target"), outside.join("link"))?;
        symlink(&outside, staged.join("parent"))?;

        let error = materialize_link(&source, &staged, Path::new("parent/link"))
            .expect_err("a symlinked parent introduced in staging must be rejected");
        assert!(error.to_string().contains("staged copy_link is unsafe"));
        assert!(outside.join("link").is_symlink());
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn materializes_link_below_read_only_directory() -> Result<()> {
        use std::os::unix::fs::{PermissionsExt, symlink};

        let temp = tempfile::tempdir()?;
        let source = temp.path().join("source");
        let staged = temp.path().join("staged");
        let parent = source.join("read-only");
        fs::create_dir_all(&parent)?;
        fs::write(temp.path().join("target"), "target")?;
        symlink(temp.path().join("target"), parent.join("link"))?;
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o555))?;

        fs::create_dir(&staged)?;
        crate::file::copy_dir_all_preserve_symlinks(&source, &staged)?;
        materialize_link(&source, &staged, Path::new("read-only/link"))?;

        assert_eq!(fs::read_to_string(staged.join("read-only/link"))?, "target");
        assert_eq!(
            fs::metadata(staged.join("read-only"))?.permissions().mode() & 0o777,
            0o555
        );
        Ok(())
    }

    #[test]
    fn dedupes_normalized_copy_link_paths() {
        assert_eq!(
            dedupe_paths(vec![
                PathBuf::from("shared/link"),
                PathBuf::from("./shared/link")
            ]),
            vec![PathBuf::from("shared/link")]
        );
    }

    #[test]
    fn parses_ad_hoc_destinations() {
        let source = std::env::current_dir().unwrap();
        let host = ad_hoc_host("ubuntu@example.com", source, &[]).unwrap();
        assert_eq!(host.user.as_deref(), Some("ubuntu"));
        assert_eq!(host.host, "example.com");
        assert_eq!(host.destination(), "ubuntu@example.com");
        assert!(ad_hoc_host("-oProxyCommand=bad", std::env::current_dir().unwrap(), &[]).is_err());
    }

    #[test]
    fn validates_remote_staging_paths() {
        assert!(validate_staging_path("/tmp/mise-bootstrap.abc123").is_ok());
        assert!(validate_staging_path("/tmp/mise-bootstrap.").is_err());
        assert!(validate_staging_path("/tmp/other").is_err());
        assert!(validate_staging_path("/tmp/mise-bootstrap.a b").is_err());
    }

    #[test]
    fn normalizes_platform_names() {
        assert_eq!(normalize_os("Darwin"), "macos");
        assert_eq!(normalize_arch("amd64"), "x86_64");
        assert_eq!(normalize_arch("arm64"), "aarch64");
    }

    #[test]
    fn provisioning_override_replaces_inventory_strategy() {
        let mut host = ad_hoc_host("example.com", std::env::current_dir().unwrap(), &[]).unwrap();
        host.remote_mise = Some("mise".to_string());
        host.apply_overrides(&RemoteOverrides {
            mise_bin: Some(std::env::current_exe().unwrap()),
            ..Default::default()
        })
        .unwrap();
        assert!(host.mise_bin.is_some());
        assert!(host.remote_mise.is_none());
        assert!(host.bootstrap_command.is_none());

        host.bootstrap_command = Some("install-mise".to_string());
        assert!(host.validate().is_err());
    }

    #[test]
    fn reads_dynamic_loader_from_elf() {
        let interpreter = b"/lib64/ld-linux-x86-64.so.2\0";
        let mut elf = vec![0_u8; 256];
        elf[..6].copy_from_slice(b"\x7fELF\x02\x01");
        elf[32..40].copy_from_slice(&64_u64.to_le_bytes());
        elf[54..56].copy_from_slice(&56_u16.to_le_bytes());
        elf[56..58].copy_from_slice(&1_u16.to_le_bytes());
        elf[64..68].copy_from_slice(&3_u32.to_le_bytes());
        elf[72..80].copy_from_slice(&128_u64.to_le_bytes());
        elf[96..104].copy_from_slice(&(interpreter.len() as u64).to_le_bytes());
        elf[128..128 + interpreter.len()].copy_from_slice(interpreter);
        assert_eq!(
            elf_interpreter(&elf).unwrap().as_deref(),
            Some("/lib64/ld-linux-x86-64.so.2")
        );
        elf[64..68].copy_from_slice(&1_u32.to_le_bytes());
        assert_eq!(elf_interpreter(&elf).unwrap(), None);
    }

    #[test]
    fn identifies_libc_flavors_without_assuming_binary_requirements() {
        assert_eq!(
            parse_libc_flavor("ld.so (Debian GLIBC 2.36) stable release version 2.36."),
            Some(LibcFlavor::Glibc)
        );
        assert_eq!(
            parse_libc_flavor("musl libc\nVersion 1.2.5"),
            Some(LibcFlavor::Musl)
        );
        assert_eq!(parse_libc_flavor("unknown loader"), None);
    }

    #[test]
    fn maps_every_supported_remote_release_target() {
        let version = env!("CARGO_PKG_VERSION");
        assert_eq!(
            release_asset_name("linux", "x86_64", Some(LibcFlavor::Glibc)).unwrap(),
            format!("mise-v{version}-linux-x64")
        );
        assert_eq!(
            release_asset_name("linux", "x86_64", Some(LibcFlavor::Musl)).unwrap(),
            format!("mise-v{version}-linux-x64-musl")
        );
        assert_eq!(
            release_asset_name("linux", "aarch64", Some(LibcFlavor::Glibc)).unwrap(),
            format!("mise-v{version}-linux-arm64")
        );
        assert_eq!(
            release_asset_name("linux", "aarch64", Some(LibcFlavor::Musl)).unwrap(),
            format!("mise-v{version}-linux-arm64-musl")
        );
        assert_eq!(
            release_asset_name("linux", "armv7", Some(LibcFlavor::Glibc)).unwrap(),
            format!("mise-v{version}-linux-armv7")
        );
        assert_eq!(
            release_asset_name("linux", "armv7", Some(LibcFlavor::Musl)).unwrap(),
            format!("mise-v{version}-linux-armv7-musl")
        );
        assert_eq!(
            release_asset_name("macos", "x86_64", None).unwrap(),
            format!("mise-v{version}-macos-x64")
        );
        assert_eq!(
            release_asset_name("macos", "aarch64", None).unwrap(),
            format!("mise-v{version}-macos-arm64")
        );
        assert!(release_asset_name("linux", "riscv64", Some(LibcFlavor::Glibc)).is_err());
        assert!(release_asset_name("freebsd", "x86_64", None).is_err());
        assert!(release_asset_name("linux", "x86_64", None).is_err());
    }

    #[test]
    fn maps_official_local_release_executables_for_provenance() {
        let version = env!("CARGO_PKG_VERSION");
        assert_eq!(
            official_release_assets("windows", "x86_64").unwrap(),
            vec![format!("mise-v{version}-windows-x64.exe")]
        );
        assert_eq!(
            official_release_assets("windows", "aarch64").unwrap(),
            vec![format!("mise-v{version}-windows-arm64.exe")]
        );
        assert!(official_release_assets("windows", "x86").is_err());
    }

    #[test]
    fn parses_remote_release_platforms_and_requires_linux_libc() {
        assert_eq!(
            parse_remote_platform("Linux\nx86_64\nglibc\n").unwrap(),
            RemotePlatform {
                os: "linux".to_string(),
                arch: "x86_64".to_string(),
                libc: Some(LibcFlavor::Glibc),
            }
        );
        assert_eq!(
            parse_remote_platform("linux\naarch64\nmusl\n").unwrap(),
            RemotePlatform {
                os: "linux".to_string(),
                arch: "aarch64".to_string(),
                libc: Some(LibcFlavor::Musl),
            }
        );
        assert_eq!(
            parse_remote_platform("Darwin\narm64\nnone\n").unwrap(),
            RemotePlatform {
                os: "macos".to_string(),
                arch: "aarch64".to_string(),
                libc: None,
            }
        );
        assert!(parse_remote_platform("Linux\nx86_64\nunknown\n").is_err());
        assert!(parse_remote_platform("Linux\nx86_64\n").is_err());
        assert!(parse_remote_platform("Darwin\narm64\nnone\nextra\n").is_err());
    }

    #[test]
    fn verifies_real_signed_release_manifest_before_lookup() {
        let contents = include_str!("remote_testdata/mise-v2026.8.1-SHASUMS256.txt");
        let signature = include_str!("remote_testdata/mise-v2026.8.1-SHASUMS256.txt.minisig");
        let manifest = ReleaseManifest::verified(contents, signature).unwrap();
        assert_eq!(
            manifest.checksum("mise-v2026.8.1-linux-x64-musl").unwrap(),
            "522fd15a3b0748d8a240bdf06cd45f679f759a097e2f49b436363e92c48fdbdc"
        );
        assert!(manifest.checksum("missing-asset").is_err());

        let tampered = contents.replacen("522fd15", "022fd15", 1);
        assert!(ReleaseManifest::verified(&tampered, signature).is_err());
    }

    #[test]
    fn compares_required_glibc_symbol_versions_with_remote_runtime() {
        assert_eq!(
            max_required_glibc_version(b"GLIBC_PRIVATE\0GLIBC_2.3.4\0GLIBC_2.17\0GLIBC_2.34\0"),
            Some(AbiVersion(vec![2, 34]))
        );
        assert_eq!(
            parse_glibc_runtime_version(
                "ld.so (Debian GLIBC 2.36-9+deb12u13) stable release version 2.36"
            ),
            Some(AbiVersion(vec![2, 36]))
        );
        assert_eq!(
            parse_glibc_runtime_version("ldd (GNU libc) 2.17"),
            Some(AbiVersion(vec![2, 17]))
        );
        assert!(AbiVersion(vec![2, 17]) < AbiVersion(vec![2, 34]));
    }

    #[test]
    fn compares_musl_loader_versions() {
        assert_eq!(
            parse_musl_runtime_version("musl libc (x86_64)\nVersion 1.2.5\nDynamic Program Loader"),
            Some(AbiVersion(vec![1, 2, 5]))
        );
        assert_eq!(
            parse_musl_runtime_version("musl libc\nversion: 1.1.24\n"),
            Some(AbiVersion(vec![1, 1, 24]))
        );
        assert!(parse_musl_runtime_version("musl libc (x86_64)").is_none());
        assert!(AbiVersion(vec![1, 2, 4]) < AbiVersion(vec![1, 2, 5]));
    }

    #[test]
    fn accepts_quoted_absolute_remote_paths() {
        assert_eq!(
            validated_remote_command_output("/tmp/mise install/bin/mise\n").unwrap(),
            "/tmp/mise install/bin/mise"
        );
        assert!(validated_remote_command("relative/mise").is_err());
        assert!(validated_remote_command("/tmp/mise\nother").is_err());
        assert!(validate_remote_executable("/tmp/mise install/bin/mise").is_ok());
        assert!(validate_remote_executable("./bin with space/mise").is_ok());
        assert!(validate_remote_executable("mise command").is_err());
        assert!(validate_remote_executable("/tmp/mise\ncommand").is_err());
        assert!(validate_remote_executable("-mise").is_err());
    }

    #[test]
    fn snapshots_the_same_candidate_scopes_before_and_after_installation() {
        let union = remote_mise_candidate_union_script();
        let lookup = remote_mise_candidates_script();
        assert!(union.starts_with("{\n"));
        assert!(union.contains(lookup));
        assert!(union.contains(&shell_words::join(["sh", "-lc", lookup])));
    }

    #[test]
    fn resolves_relative_remote_mise_inside_staged_project() {
        let project = "/tmp/mise-bootstrap.abc/project";
        assert_eq!(
            resolve_remote_mise_path("./bin/mise", project, None).unwrap(),
            "/tmp/mise-bootstrap.abc/project/bin/mise"
        );
        assert_eq!(
            resolve_remote_mise_path("bin/../tools/mise", project, None).unwrap(),
            "/tmp/mise-bootstrap.abc/project/tools/mise"
        );
        assert_eq!(
            resolve_remote_mise_path("~/.local/bin/mise", project, Some("/home/test user"))
                .unwrap(),
            "/home/test user/.local/bin/mise"
        );
        assert_eq!(
            resolve_remote_mise_path("/opt/mise/bin/mise", project, None).unwrap(),
            "/opt/mise/bin/mise"
        );
        assert!(resolve_remote_mise_path("../mise", project, None).is_err());
        assert!(resolve_remote_mise_path("bin/../../mise", project, None).is_err());
        assert!(resolve_remote_mise_path("~/mise", project, None).is_err());
    }

    #[test]
    fn selects_new_changed_or_matching_bootstrapped_mise() {
        let identity = |version: &str, checksum: &str| {
            Some(RemoteMiseIdentity {
                version: version.to_string(),
                fingerprint: checksum.to_string(),
            })
        };
        let old = "/opt/old/mise".to_string();
        let installed = "/opt/new/mise".to_string();
        let before = vec![old.clone()];
        let before_identities = IndexMap::from([(old.clone(), identity("1.0.0", "100 10"))]);
        let after = vec![old.clone(), installed.clone()];
        let after_identities = IndexMap::from([
            (old.clone(), identity("1.0.0", "100 10")),
            (installed.clone(), identity("2.0.0", "200 20")),
        ]);
        assert_eq!(
            select_bootstrapped_mise(&before, &before_identities, &after, &after_identities)
                .unwrap(),
            installed
        );

        let after = vec![old.clone()];
        let changed = IndexMap::from([(old.clone(), identity("1.0.0", "300 30"))]);
        assert_eq!(
            select_bootstrapped_mise(&before, &before_identities, &after, &changed).unwrap(),
            old
        );

        let stale = "/opt/stale/mise".to_string();
        let overwritten = "/opt/overwritten/mise".to_string();
        let same_paths = vec![stale.clone(), overwritten.clone()];
        let before_same_version = IndexMap::from([
            (stale.clone(), identity("1.0.0", "100 10")),
            (overwritten.clone(), identity("2.0.0", "200 20")),
        ]);
        let after_same_version = IndexMap::from([
            (stale, identity("1.0.0", "100 10")),
            (overwritten.clone(), identity("2.0.0", "300 30")),
        ]);
        assert_eq!(
            select_bootstrapped_mise(
                &same_paths,
                &before_same_version,
                &same_paths,
                &after_same_version,
            )
            .unwrap(),
            overwritten
        );

        let current = "/opt/current/mise".to_string();
        let candidates = vec!["/opt/stale/mise".to_string(), current.clone()];
        let identities = IndexMap::from([
            (candidates[0].clone(), identity("0.0.1", "100 10")),
            (
                current.clone(),
                identity(
                    &format!("{} linux-x64", env!("CARGO_PKG_VERSION")),
                    "200 20",
                ),
            ),
        ]);
        assert_eq!(
            select_bootstrapped_mise(&candidates, &identities, &candidates, &identities).unwrap(),
            current
        );
    }

    #[test]
    fn rejects_ambiguous_bootstrapped_mise_candidates() {
        let candidates = vec!["/opt/one/mise".to_string(), "/opt/two/mise".to_string()];
        let identities = IndexMap::from([
            (
                candidates[0].clone(),
                Some(RemoteMiseIdentity {
                    version: "1.0.0".to_string(),
                    fingerprint: "100 10".to_string(),
                }),
            ),
            (
                candidates[1].clone(),
                Some(RemoteMiseIdentity {
                    version: "2.0.0".to_string(),
                    fingerprint: "200 20".to_string(),
                }),
            ),
        ]);
        assert!(
            select_bootstrapped_mise(&candidates, &identities, &candidates, &identities).is_err()
        );
        let current_version = format!("{} linux-x64", env!("CARGO_PKG_VERSION"));
        let matching_identities = IndexMap::from([
            (
                candidates[0].clone(),
                Some(RemoteMiseIdentity {
                    version: current_version.clone(),
                    fingerprint: "100 10".to_string(),
                }),
            ),
            (
                candidates[1].clone(),
                Some(RemoteMiseIdentity {
                    version: current_version,
                    fingerprint: "200 20".to_string(),
                }),
            ),
        ]);
        assert!(
            select_bootstrapped_mise(
                &candidates,
                &matching_identities,
                &candidates,
                &matching_identities,
            )
            .unwrap_err()
            .to_string()
            .contains("multiple version-matching mise executables")
        );
        assert!(parse_remote_mise_candidates("/opt/mise").is_err());
        assert_eq!(
            parse_remote_mise_candidates("/opt/one\0/opt/one\0/opt/two\0").unwrap(),
            vec!["/opt/one".to_string(), "/opt/two".to_string()]
        );
    }
}
