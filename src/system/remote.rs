use std::fmt;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use eyre::{Context, Result, bail, eyre};
use indexmap::{IndexMap, IndexSet};
use serde::Deserialize;

use crate::config::Config;

#[derive(Clone, Debug, Default, Deserialize)]
pub struct RemoteTomlConfig {
    pub source: Option<PathBuf>,
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
            let host = RemoteHost {
                name: name.clone(),
                host: host.host,
                user: host.user,
                port: host.port,
                identity_file,
                source,
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

pub fn run(host: &RemoteHost, options: &RemoteRunOptions) -> Result<()> {
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
    let mut result = run_staged(&session, &tar, &staging, options);
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

fn run_staged(
    session: &SshSession<'_>,
    tar: &Path,
    staging: &str,
    options: &RemoteRunOptions,
) -> Result<()> {
    let project = format!("{staging}/project");
    session.status(&["mkdir", "-p", &project], false)?;
    upload_source(session, tar, &project)?;
    let mise = provision_mise(session, staging, &project, options.dry_run)?;
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
    let mut command = Command::new(tar);
    command.args(["-czf"]);
    command.arg(&archive);
    for exclude in &session.host.exclude {
        command.arg(format!("--exclude={exclude}"));
    }
    let status = command
        .args(["-C"])
        .arg(&session.host.source)
        .arg(".")
        .status()?;
    if !status.success() {
        bail!("failed to archive remote source with {status}");
    }
    session.status_with_stdin(&["tar", "-xzf", "-", "-C", project], File::open(archive)?)
}

fn provision_mise(
    session: &SshSession<'_>,
    staging: &str,
    project: &str,
    dry_run: bool,
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
    let platform = session.output(&["sh", "-c", "uname -s; uname -m"])?;
    let mut lines = platform.lines();
    let remote_os = normalize_os(lines.next().unwrap_or_default());
    let remote_arch = normalize_arch(lines.next().unwrap_or_default());
    let binary = if let Some(binary) = &session.host.mise_bin {
        binary.clone()
    } else {
        let local_os = normalize_os(std::env::consts::OS);
        let local_arch = normalize_arch(std::env::consts::ARCH);
        if remote_os != local_os || remote_arch != local_arch {
            bail!(
                "remote host '{}' is {remote_os}/{remote_arch}, but local mise is {local_os}/{local_arch}; set mise_bin, remote_mise, or bootstrap_command",
                session.host.name
            );
        }
        let binary = std::env::current_exe()?;
        validate_default_binary_compatibility(session, &binary, &remote_os)?;
        binary
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

#[cfg(test)]
mod tests {
    use super::*;

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
