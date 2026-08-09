use super::session;
use eyre::{Context, Result, bail};
use mise_cache_core::{
    AgentRequest, AgentResponse, CacheDigest, CacheDirectory, CacheFileNode, RemoteActionResult,
    RustcMetadata, canonical_json,
};
use mise_cache_rustc::{
    ActionContext, CompilerIdentity, PathMapping, RustcDepInfo, RustcInvocation,
};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, ExitStatus, Output};

pub(super) fn compile_miss(rustc: &OsStr, arguments: &[OsString]) -> Result<ExitCode> {
    let invocation = RustcInvocation::parse(arguments)?;
    let working_dir = std::env::current_dir()?;
    let outputs = invocation.outputs(&working_dir)?;
    let output = Command::new(rustc)
        .args(arguments)
        .current_dir(&working_dir)
        .output()
        .wrap_err("failed to execute rustc")?;
    let _ = replay_output(&output);
    if output.status.success() {
        let publication = (|| {
            let dep_info = RustcDepInfo::read(&outputs.dep_info)?;
            let discovered = invocation.discover_inputs(&dep_info, &working_dir)?;
            let mut context = ActionContext {
                compiler: compiler_identity(rustc)?,
                working_dir: working_dir.clone(),
                path_mappings: path_mappings(&working_dir),
                environment: BTreeMap::new(),
                inputs: Vec::new(),
            };
            discovered.clone().apply_to(&mut context)?;
            let action = invocation.action(context)?;
            discovered.verify()?;
            publish_result(&action.digest, &action.bytes, &outputs.files, &output)
        })();
        if let Err(error) = publication {
            eprintln!("mise rustc cache warning: result was not stored: {error:#}");
        }
    }
    Ok(exit_code(output.status))
}

fn compiler_identity(rustc: &OsStr) -> Result<CompilerIdentity> {
    let executable = resolve_executable(rustc)?;
    let environment = ["RUSTUP_HOME", "RUSTUP_TOOLCHAIN"]
        .into_iter()
        .map(|name| (name.into(), std::env::var(name).ok()))
        .collect::<BTreeMap<_, _>>();
    let responses = session::request_agent(&[AgentRequest::FindExecutableIdentity {
        executable: executable.clone(),
        environment: environment.clone(),
    }])?;
    let Some(AgentResponse::ExecutableIdentity { stdout }) = responses.into_iter().next() else {
        bail!("cache agent did not return the rustc identity");
    };
    let stdout = if let Some(stdout) = stdout {
        stdout
    } else {
        let mut command = Command::new(&executable);
        command.arg("-vV");
        for (name, value) in &environment {
            if let Some(value) = value {
                command.env(name, value);
            } else {
                command.env_remove(name);
            }
        }
        let output = command
            .output()
            .wrap_err("failed to query the rustc identity")?;
        if !output.status.success() {
            bail!(
                "rustc identity command failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        let responses = session::request_agent(&[AgentRequest::StoreExecutableIdentity {
            executable,
            environment,
            stdout: output.stdout,
        }])?;
        let Some(AgentResponse::ExecutableIdentity {
            stdout: Some(stdout),
        }) = responses.into_iter().next()
        else {
            bail!("cache agent did not store the rustc identity");
        };
        stdout
    };
    let verbose = std::str::from_utf8(&stdout).wrap_err("rustc identity is not UTF-8")?;
    let release = identity_field(verbose, "release")?;
    let host = identity_field(verbose, "host")?;
    let rustc_version = verbose
        .lines()
        .filter(|line| {
            line.starts_with("rustc ")
                || line.starts_with("commit-hash:")
                || line.starts_with("commit-date:")
                || line.starts_with("LLVM version:")
        })
        .collect::<Vec<_>>()
        .join("; ");
    if rustc_version.is_empty() {
        bail!("rustc identity is missing its version");
    }
    let toolchain = std::env::var("RUSTUP_TOOLCHAIN")
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| release.to_string());
    Ok(CompilerIdentity {
        toolchain,
        rustc_version,
        host: host.to_string(),
    })
}

fn identity_field<'a>(verbose: &'a str, field: &str) -> Result<&'a str> {
    verbose
        .lines()
        .find_map(|line| line.strip_prefix(&format!("{field}: ")))
        .filter(|value| !value.is_empty())
        .ok_or_else(|| eyre::eyre!("rustc identity is missing {field}"))
}

fn path_mappings(working_dir: &Path) -> Vec<PathMapping> {
    let mut mappings = Vec::new();
    let mut roots = BTreeSet::new();
    add_mapping(
        &mut mappings,
        &mut roots,
        working_dir.to_path_buf(),
        "workspace",
    );
    for (name, placeholder) in [
        ("CARGO_HOME", "cargo_home"),
        ("RUSTUP_HOME", "rustup_home"),
        ("HOME", "home"),
        ("USERPROFILE", "home"),
    ] {
        if let Some(root) = std::env::var_os(name).map(PathBuf::from)
            && root.is_absolute()
        {
            add_mapping(&mut mappings, &mut roots, root, placeholder);
        }
    }
    mappings
}

fn add_mapping(
    mappings: &mut Vec<PathMapping>,
    roots: &mut BTreeSet<PathBuf>,
    root: PathBuf,
    placeholder: &str,
) {
    if roots.insert(root.clone())
        && !mappings
            .iter()
            .any(|mapping| mapping.placeholder == placeholder)
    {
        mappings.push(PathMapping::new(root, placeholder));
    }
}

fn replay_output(output: &Output) -> Result<()> {
    let mut stdout = std::io::stdout().lock();
    stdout.write_all(&output.stdout)?;
    stdout.flush()?;
    let mut stderr = std::io::stderr().lock();
    stderr.write_all(&output.stderr)?;
    stderr.flush()?;
    Ok(())
}

fn staging_directory() -> Result<tempfile::TempDir> {
    let root = std::env::var_os(session::STAGING_ENV)
        .map(PathBuf::from)
        .ok_or_else(|| eyre::eyre!("{} is not set", session::STAGING_ENV))?;
    Ok(tempfile::tempdir_in(root)?)
}

fn resolve_executable(executable: &OsStr) -> Result<PathBuf> {
    let executable = PathBuf::from(executable);
    if executable.is_absolute() {
        return Ok(executable);
    }
    which::which(&executable).wrap_err_with(|| {
        format!(
            "failed to resolve compiler executable {}",
            executable.display()
        )
    })
}

fn publish_result(
    action: &CacheDigest,
    action_bytes: &[u8],
    outputs: &[PathBuf],
    output: &Output,
) -> Result<()> {
    if outputs.is_empty() {
        bail!("rustc produced no cacheable outputs");
    }
    let staging = staging_directory()?;
    let mut blobs = vec![staged_bytes(staging.path(), "action.json", action_bytes)?];
    let stdout = staged_bytes(staging.path(), "stdout", &output.stdout)?;
    let stderr = staged_bytes(staging.path(), "stderr", &output.stderr)?;
    blobs.extend([stdout.clone(), stderr.clone()]);

    let mut files = Vec::with_capacity(outputs.len());
    for path in outputs {
        let metadata = std::fs::metadata(path)
            .wrap_err_with(|| format!("failed to inspect rustc output {}", path.display()))?;
        if !metadata.is_file() {
            bail!("rustc output is not a regular file: {}", path.display());
        }
        let digest = CacheDigest::blake3_file(path)?;
        blobs.push((digest.clone(), path.clone()));
        files.push(CacheFileNode {
            digest,
            executable: false,
            mode: file_mode(&metadata),
            name: path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| eyre::eyre!("rustc output name is not UTF-8"))?
                .to_string(),
        });
    }
    files.sort_by(|left, right| left.name.cmp(&right.name));

    let metadata = canonical_json(&RustcMetadata {
        version: 1,
        kind: "rustc".into(),
        stdout: stdout.0,
        stderr: stderr.0,
    })?;
    let metadata = staged_bytes(staging.path(), "metadata.json", &metadata)?;
    blobs.push(metadata.clone());
    let directory = canonical_json(&CacheDirectory {
        directories: Vec::new(),
        files,
        symlinks: Vec::new(),
        version: 1,
    })?;
    let directory = staged_bytes(staging.path(), "directory.json", &directory)?;
    blobs.push(directory.clone());

    let mut requests = Vec::new();
    let mut published = BTreeSet::new();
    for (digest, source) in blobs {
        if published.insert(digest.clone()) {
            requests.push(AgentRequest::StoreBlob { digest, source });
        }
    }
    requests.push(AgentRequest::StoreActionResult {
        result: RemoteActionResult {
            action: action.clone(),
            metadata: Some(metadata.0),
            output_root: Some(directory.0),
            version: 1,
        },
    });
    for response in session::request_agent(&requests)? {
        match response {
            AgentResponse::Stored { .. } | AgentResponse::ActionStored { .. } => {}
            AgentResponse::Error { message } => bail!(message),
            _ => bail!("cache agent returned an unexpected publish response"),
        }
    }
    Ok(())
}

fn staged_bytes(directory: &Path, name: &str, bytes: &[u8]) -> Result<(CacheDigest, PathBuf)> {
    let path = directory.join(name);
    std::fs::write(&path, bytes)?;
    Ok((CacheDigest::blake3(bytes), path))
}

#[cfg(unix)]
fn file_mode(metadata: &std::fs::Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt as _;
    metadata.permissions().mode() & 0o777
}

#[cfg(windows)]
fn file_mode(_metadata: &std::fs::Metadata) -> u32 {
    0
}

#[cfg(unix)]
fn exit_code(status: ExitStatus) -> ExitCode {
    use std::os::unix::process::ExitStatusExt as _;
    ExitCode::from(
        status
            .code()
            .unwrap_or_else(|| 128 + status.signal().unwrap_or(1)) as u8,
    )
}

#[cfg(windows)]
fn exit_code(status: ExitStatus) -> ExitCode {
    // SAFETY: this process is only a compiler wrapper and must preserve the
    // compiler's full Windows status code, which stable ExitCode cannot hold.
    unsafe { windows_sys::Win32::System::Threading::ExitProcess(status.code().unwrap_or(1) as u32) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_verbose_rustc_identity() {
        let verbose = "rustc 1.97.0 (abc 2026-08-01)\n\
                       binary: rustc\n\
                       commit-hash: abc\n\
                       commit-date: 2026-08-01\n\
                       host: x86_64-unknown-linux-gnu\n\
                       release: 1.97.0\n\
                       LLVM version: 22.0.0\n";
        assert_eq!(identity_field(verbose, "release").unwrap(), "1.97.0");
        assert_eq!(
            identity_field(verbose, "host").unwrap(),
            "x86_64-unknown-linux-gnu"
        );
    }

    #[test]
    fn mappings_do_not_duplicate_home_placeholders() {
        let directory = tempfile::tempdir().unwrap();
        let mappings = path_mappings(directory.path());
        let placeholders = mappings
            .iter()
            .map(|mapping| &mapping.placeholder)
            .collect::<BTreeSet<_>>();
        assert_eq!(placeholders.len(), mappings.len());
    }
}
