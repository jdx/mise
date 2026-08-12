use super::session;
use eyre::{Context, Result, bail};
use mise_cache_core::{
    ActionPrediction, AgentRequest, AgentResponse, CacheDigest, CacheDirectory, CacheFileNode,
    RemoteActionResult, RustcMetadata, canonical_json,
};
use mise_cache_rustc::{
    ActionContext, CompilerIdentity, DiscoveredInputs, PathMapping, RustcAction, RustcDepInfo,
    RustcInputPrediction, RustcInvocation, RustcOutputs,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, ExitStatus, Output};
use std::time::SystemTime;

struct CachedCompilation {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    outputs: Vec<CachedOutput>,
}

struct CachedOutput {
    path: PathBuf,
    digest: CacheDigest,
    mode: u32,
}

struct StagedOutputs {
    directory: tempfile::TempDir,
    files: Vec<(tempfile::TempPath, PathBuf)>,
}

pub(super) fn compile(rustc: &OsStr, arguments: &[OsString]) -> Result<ExitCode> {
    let invocation = RustcInvocation::parse(arguments)?;
    let working_dir = std::env::current_dir()?;
    let outputs = invocation.outputs(&working_dir)?;

    let verify = std::env::var_os(session::VERIFY_ENV).is_some();
    let mut verification = None;
    let mut action_lookup_attempted = false;
    if outputs.dep_info.is_file()
        && let Ok((action, discovered)) =
            action_from_current_dep_info(rustc, &invocation, &outputs.dep_info, &working_dir)
    {
        action_lookup_attempted = true;
        match restore_result(&action, &outputs, &discovered, !verify) {
            Ok(Some(cached)) => {
                record_prediction(rustc, &invocation, &action, &discovered, &working_dir);
                if verify {
                    verification = Some(cached);
                } else {
                    let _ = replay_bytes(&cached.stdout, &cached.stderr);
                    return Ok(ExitCode::SUCCESS);
                }
            }
            Ok(None) => {}
            Err(error) => {
                eprintln!("mise rustc cache warning: result was not restored: {error:#}");
            }
        }
    }
    if !action_lookup_attempted {
        match restore_predicted_result(rustc, &invocation, &outputs, &working_dir, !verify) {
            Ok(Some(cached)) => {
                if verify {
                    verification = Some(cached);
                } else {
                    let _ = replay_bytes(&cached.stdout, &cached.stderr);
                    return Ok(ExitCode::SUCCESS);
                }
            }
            Ok(None) => {}
            Err(error) => {
                eprintln!("mise rustc cache warning: prediction was not restored: {error:#}");
            }
        }
    }

    let compilation_started = SystemTime::now();
    let output = Command::new(rustc)
        .args(arguments)
        .current_dir(&working_dir)
        .output()
        .wrap_err("failed to execute rustc")?;
    let _ = replay_output(&output);
    if let Some(cached) = verification {
        let matched = cached_matches(&cached, &output);
        record_verification(matched);
        if !matched {
            eprintln!("mise rustc cache warning: shadow verification diverged from cached output");
        }
        return Ok(exit_code(output.status));
    }
    if output.status.success() {
        let publication: Result<()> = (|| {
            let (action, discovered) =
                action_from_dep_info(rustc, &invocation, &outputs.dep_info, &working_dir)?;
            discovered.verify_not_modified_since(compilation_started)?;
            discovered.verify()?;
            let mut cacheable_outputs = outputs.files.clone();
            cacheable_outputs.push(outputs.dep_info.clone());
            publish_result(&action.digest, &action.bytes, &cacheable_outputs, &output)?;
            record_prediction(rustc, &invocation, &action, &discovered, &working_dir);
            Ok(())
        })();
        if let Err(error) = publication {
            eprintln!("mise rustc cache warning: result was not stored: {error:#}");
        }
    }
    Ok(exit_code(output.status))
}

fn restore_predicted_result(
    rustc: &OsStr,
    invocation: &RustcInvocation,
    outputs: &RustcOutputs,
    working_dir: &Path,
    restore_outputs: bool,
) -> Result<Option<CachedCompilation>> {
    let task = std::env::var(session::TASK_ENV)
        .wrap_err_with(|| format!("{} is not set", session::TASK_ENV))?;
    let mut context = base_action_context(rustc, working_dir)?;
    let invocation_digest = invocation.invocation_digest(&context)?;
    let responses = session::request_agent(&[AgentRequest::FindActionPrediction {
        task,
        invocation: invocation_digest.clone(),
    }])?;
    let Some(response) = responses.into_iter().next() else {
        bail!("cache agent did not return an action prediction response");
    };
    let prediction = match response {
        AgentResponse::ActionPrediction {
            prediction: Some(prediction),
        } => prediction,
        AgentResponse::ActionPrediction { prediction: None } => return Ok(None),
        AgentResponse::Error { message } => bail!(message),
        _ => bail!("cache agent returned an unexpected action prediction response"),
    };
    if prediction.adapter != "rustc" || prediction.invocation != invocation_digest {
        bail!("cache agent returned an incompatible rustc action prediction");
    }
    let input_prediction: RustcInputPrediction = serde_json::from_str(&prediction.payload)?;
    if String::from_utf8(canonical_json(&input_prediction)?)? != prediction.payload {
        bail!("cache agent returned a non-canonical rustc action prediction");
    }
    let discovered = input_prediction.discover(working_dir, &context.path_mappings)?;
    discovered.clone().apply_to(&mut context)?;
    let action = invocation.action(context)?;
    let restored = restore_result(&action, outputs, &discovered, restore_outputs)?;
    if restored.is_some() {
        record_prediction_value(invocation_digest, action.digest.clone(), prediction.payload);
    }
    Ok(restored)
}

fn action_from_dep_info(
    rustc: &OsStr,
    invocation: &RustcInvocation,
    dep_info: &Path,
    working_dir: &Path,
) -> Result<(RustcAction, DiscoveredInputs)> {
    let dep_info = RustcDepInfo::read(dep_info)?;
    action_from_parsed_dep_info(rustc, invocation, &dep_info, working_dir)
}

fn action_from_current_dep_info(
    rustc: &OsStr,
    invocation: &RustcInvocation,
    dep_info: &Path,
    working_dir: &Path,
) -> Result<(RustcAction, DiscoveredInputs)> {
    let dep_info = RustcDepInfo::read(dep_info)?;
    verify_environment(&dep_info.environment)?;
    action_from_parsed_dep_info(rustc, invocation, &dep_info, working_dir)
}

fn action_from_parsed_dep_info(
    rustc: &OsStr,
    invocation: &RustcInvocation,
    dep_info: &RustcDepInfo,
    working_dir: &Path,
) -> Result<(RustcAction, DiscoveredInputs)> {
    let discovered = invocation.discover_inputs(dep_info, working_dir)?;
    let mut context = base_action_context(rustc, working_dir)?;
    discovered.clone().apply_to(&mut context)?;
    let action = invocation.action(context)?;
    Ok((action, discovered))
}

fn base_action_context(rustc: &OsStr, working_dir: &Path) -> Result<ActionContext> {
    Ok(ActionContext {
        compiler: compiler_identity(rustc)?,
        working_dir: working_dir.to_path_buf(),
        path_mappings: path_mappings(working_dir),
        environment: BTreeMap::new(),
        inputs: Vec::new(),
    })
}

fn record_prediction(
    rustc: &OsStr,
    invocation: &RustcInvocation,
    action: &RustcAction,
    discovered: &DiscoveredInputs,
    working_dir: &Path,
) {
    let result = (|| {
        let context = base_action_context(rustc, working_dir)?;
        let invocation_digest = invocation.invocation_digest(&context)?;
        let prediction = invocation.prediction(&context, discovered)?;
        let payload = String::from_utf8(canonical_json(&prediction)?)?;
        record_prediction_value(invocation_digest, action.digest.clone(), payload);
        Result::<()>::Ok(())
    })();
    if let Err(error) = result {
        eprintln!("mise rustc cache warning: action prediction was not recorded: {error:#}");
    }
}

fn record_prediction_value(invocation: CacheDigest, action: CacheDigest, payload: String) {
    let result = (|| {
        let task = std::env::var(session::TASK_ENV)
            .wrap_err_with(|| format!("{} is not set", session::TASK_ENV))?;
        let responses = session::request_agent(&[AgentRequest::RecordActionPrediction {
            task,
            prediction: ActionPrediction {
                invocation,
                action,
                adapter: "rustc".into(),
                payload,
            },
        }])?;
        match responses.into_iter().next() {
            Some(AgentResponse::ActionPredictionRecorded) => Ok(()),
            Some(AgentResponse::Error { message }) => bail!(message),
            _ => bail!("cache agent returned an unexpected prediction response"),
        }
    })();
    if let Err(error) = result {
        eprintln!("mise rustc cache warning: action prediction was not recorded: {error:#}");
    }
}

fn verify_environment(environment: &BTreeMap<String, Option<String>>) -> Result<()> {
    for (name, expected) in environment {
        let actual = std::env::var_os(name)
            .map(|value| {
                value.into_string().map_err(|_| {
                    eyre::eyre!("compiler environment input is not valid UTF-8: {name}")
                })
            })
            .transpose()?;
        if &actual != expected {
            bail!("compiler environment input changed: {name}");
        }
    }
    Ok(())
}

fn restore_result(
    action: &RustcAction,
    outputs: &RustcOutputs,
    discovered: &DiscoveredInputs,
    restore_outputs: bool,
) -> Result<Option<CachedCompilation>> {
    let responses = session::request_agent(&[AgentRequest::FindActionResult {
        action: action.digest.clone(),
    }])?;
    let Some(response) = responses.into_iter().next() else {
        bail!("cache agent did not return an action lookup response");
    };
    let result = match response {
        AgentResponse::ActionResult { result } => match result {
            Some(result) => result,
            None => return Ok(None),
        },
        AgentResponse::Error { message } => bail!(message),
        _ => bail!("cache agent returned an unexpected action lookup response"),
    };
    if result.version != 1 || result.action != action.digest {
        bail!("cached rustc action result has an invalid identity");
    }
    let metadata_digest = result
        .metadata
        .ok_or_else(|| eyre::eyre!("cached rustc action result has no metadata"))?;
    let output_root_digest = result
        .output_root
        .ok_or_else(|| eyre::eyre!("cached rustc action result has no output root"))?;
    let roots = find_blobs(&[
        action.digest.clone(),
        metadata_digest.clone(),
        output_root_digest.clone(),
    ])?;
    let cached_action = read_verified_blob(&roots[0], &action.digest, "action descriptor")?;
    if cached_action != action.bytes {
        bail!("cached rustc action descriptor does not match the invocation");
    }
    let metadata: RustcMetadata =
        read_canonical_blob(&roots[1], &metadata_digest, "rustc metadata")?;
    if metadata.version != 1 || metadata.kind != "rustc" {
        bail!("cached rustc metadata is unsupported");
    }
    let directory: CacheDirectory =
        read_canonical_blob(&roots[2], &output_root_digest, "output directory")?;
    let files = validated_outputs(directory, outputs)?;
    let cached_outputs = files
        .iter()
        .map(|(node, destination)| CachedOutput {
            path: destination.clone(),
            digest: node.digest.clone(),
            mode: node.mode,
        })
        .collect();

    let mut digests = vec![metadata.stdout.clone(), metadata.stderr.clone()];
    digests.extend(files.iter().map(|(node, _)| node.digest.clone()));
    let blobs = find_blobs(&digests)?;
    let stdout = read_verified_blob(&blobs[0], &metadata.stdout, "stdout")?;
    let stderr = read_verified_blob(&blobs[1], &metadata.stderr, "stderr")?;

    std::fs::create_dir_all(&outputs.directory)?;
    let staging = tempfile::tempdir_in(&outputs.directory)?;
    let mut staged = Vec::with_capacity(files.len());
    for (index, ((node, destination), source)) in files.into_iter().zip(&blobs[2..]).enumerate() {
        let temporary = stage_cached_output(staging.path(), index, source, &node)?;
        staged.push((temporary, destination));
    }
    let staged = StagedOutputs {
        directory: staging,
        files: staged,
    };

    discovered.verify()?;
    verify_environment(&discovered.environment)?;
    finalize_restored_outputs(staged, restore_outputs)?;
    if restore_outputs {
        record_action_hit(&action.digest);
    }
    Ok(Some(CachedCompilation {
        stdout,
        stderr,
        outputs: cached_outputs,
    }))
}

fn finalize_restored_outputs(staged: StagedOutputs, restore_outputs: bool) -> Result<()> {
    if restore_outputs {
        persist_outputs(staged)?;
    }
    Ok(())
}

fn stage_cached_output(
    directory: &Path,
    index: usize,
    source: &Path,
    node: &CacheFileNode,
) -> Result<tempfile::TempPath> {
    let temporary = directory.join(format!("output-{index}"));
    reflink_copy::reflink_or_copy(source, &temporary)
        .wrap_err_with(|| format!("failed to materialize cached rustc output {}", node.name))?;
    let temporary = tempfile::TempPath::try_from_path(temporary)?;
    make_owner_writable(&temporary)?;
    std::fs::OpenOptions::new()
        .write(true)
        .open(&temporary)?
        .sync_all()?;
    if !node.digest.matches_file(&temporary)? {
        bail!(
            "cached rustc output failed digest verification: {}",
            node.name
        );
    }
    apply_file_mode(&temporary, node.mode)?;
    Ok(temporary)
}

fn cached_matches(cached: &CachedCompilation, output: &Output) -> bool {
    output.status.success()
        && cached.stdout == output.stdout
        && cached.stderr == output.stderr
        && cached.outputs.iter().all(|expected| {
            std::fs::metadata(&expected.path).is_ok_and(|metadata| {
                file_mode(&metadata) == expected.mode
                    && expected
                        .digest
                        .matches_file(&expected.path)
                        .unwrap_or(false)
            })
        })
}

fn record_verification(matched: bool) {
    let responses = session::request_agent(&[AgentRequest::RecordActionVerification { matched }]);
    match responses.map(|responses| responses.into_iter().next()) {
        Ok(Some(AgentResponse::ActionVerificationRecorded)) => {}
        Ok(Some(AgentResponse::Error { message })) => {
            eprintln!("mise rustc cache warning: verification was not recorded: {message}");
        }
        Ok(_) => eprintln!("mise rustc cache warning: verification was not recorded"),
        Err(error) => {
            eprintln!("mise rustc cache warning: verification was not recorded: {error:#}");
        }
    }
}

fn persist_outputs(staged: StagedOutputs) -> Result<()> {
    let StagedOutputs {
        directory: _directory,
        files,
    } = staged;
    let destinations = files
        .iter()
        .map(|(_, destination)| destination.clone())
        .collect::<Vec<_>>();
    for (temporary, destination) in files {
        let persisted = temporary
            .persist(&destination)
            .map_err(|error| error.error)
            .wrap_err_with(|| format!("failed to atomically restore {}", destination.display()));
        if let Err(error) = persisted {
            for destination in &destinations {
                match std::fs::remove_file(destination) {
                    Ok(()) => {}
                    Err(remove_error) if remove_error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(remove_error) => eprintln!(
                        "mise rustc cache warning: failed to roll back {}: {remove_error}",
                        destination.display()
                    ),
                }
            }
            return Err(error);
        }
    }
    Ok(())
}

fn record_action_hit(action: &CacheDigest) {
    let responses = session::request_agent(&[AgentRequest::RecordActionHit {
        action: action.clone(),
    }]);
    match responses.map(|responses| responses.into_iter().next()) {
        Ok(Some(AgentResponse::ActionHitRecorded)) => {}
        Ok(Some(AgentResponse::Error { message })) => {
            eprintln!("mise rustc cache warning: hit was not recorded: {message}");
        }
        Ok(_) => eprintln!("mise rustc cache warning: hit was not recorded"),
        Err(error) => {
            eprintln!("mise rustc cache warning: hit was not recorded: {error:#}");
        }
    }
}

fn find_blobs(digests: &[CacheDigest]) -> Result<Vec<PathBuf>> {
    let requests = digests
        .iter()
        .cloned()
        .map(|digest| AgentRequest::FindBlob { digest })
        .collect::<Vec<_>>();
    let responses = session::request_agent(&requests)?;
    if responses.len() != digests.len() {
        bail!("cache agent returned an incomplete blob lookup response");
    }
    responses
        .into_iter()
        .zip(digests)
        .map(|(response, digest)| match response {
            AgentResponse::Blob { path: Some(path) } => Ok(path),
            AgentResponse::Blob { path: None } => {
                bail!("cached rustc action is missing blob {}", digest.hash)
            }
            AgentResponse::Error { message } => bail!(message),
            _ => bail!("cache agent returned an unexpected blob lookup response"),
        })
        .collect()
}

fn read_canonical_blob<T>(path: &Path, digest: &CacheDigest, description: &str) -> Result<T>
where
    T: DeserializeOwned + Serialize,
{
    let bytes = read_verified_blob(path, digest, description)?;
    let value = serde_json::from_slice(&bytes)
        .wrap_err_with(|| format!("cached {description} is not valid JSON"))?;
    if canonical_json(&value)? != bytes {
        bail!("cached {description} is not canonical JSON");
    }
    Ok(value)
}

fn read_verified_blob(path: &Path, digest: &CacheDigest, description: &str) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?.read_to_end(&mut bytes)?;
    if !digest.matches_bytes(&bytes)? {
        bail!("cached {description} failed digest verification");
    }
    Ok(bytes)
}

fn validated_outputs(
    directory: CacheDirectory,
    outputs: &RustcOutputs,
) -> Result<Vec<(CacheFileNode, PathBuf)>> {
    if directory.version != 1 || !directory.directories.is_empty() || !directory.symlinks.is_empty()
    {
        bail!("cached rustc output directory has unsupported entries");
    }
    let mut expected = outputs
        .files
        .iter()
        .chain(std::iter::once(&outputs.dep_info))
        .map(|path| {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| eyre::eyre!("expected rustc output name is not UTF-8"))?;
            if path.parent() != Some(outputs.directory.as_path()) {
                bail!("expected rustc output escapes its output directory");
            }
            Ok((name.to_string(), path.clone()))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    if directory.files.len() != expected.len() {
        bail!("cached rustc output set does not match the invocation");
    }
    let mut files = Vec::with_capacity(directory.files.len());
    for node in directory.files {
        validate_file_mode(&node)?;
        let destination = expected
            .remove(&node.name)
            .ok_or_else(|| eyre::eyre!("cached rustc output is unexpected: {}", node.name))?;
        files.push((node, destination));
    }
    if !expected.is_empty() {
        bail!("cached rustc output set is incomplete");
    }
    Ok(files)
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
    replay_bytes(&output.stdout, &output.stderr)
}

fn replay_bytes(stdout_bytes: &[u8], stderr_bytes: &[u8]) -> Result<()> {
    let mut stdout = std::io::stdout().lock();
    stdout.write_all(stdout_bytes)?;
    stdout.flush()?;
    let mut stderr = std::io::stderr().lock();
    stderr.write_all(stderr_bytes)?;
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
    metadata.permissions().mode() & 0o644
}

#[cfg(windows)]
fn file_mode(_metadata: &std::fs::Metadata) -> u32 {
    0
}

#[cfg(unix)]
fn validate_file_mode(node: &CacheFileNode) -> Result<()> {
    if node.executable
        || node.mode & !0o777 != 0
        || node.mode & 0o111 != 0
        || node.mode & 0o022 != 0
    {
        bail!("cached rustc output has an unsafe file mode: {}", node.name);
    }
    Ok(())
}

#[cfg(windows)]
fn validate_file_mode(node: &CacheFileNode) -> Result<()> {
    if node.executable || node.mode != 0 {
        bail!("cached rustc output has an unsafe file mode: {}", node.name);
    }
    Ok(())
}

#[cfg(unix)]
fn apply_file_mode(temporary: &Path, mode: u32) -> Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(temporary, std::fs::Permissions::from_mode(mode))?;
    Ok(())
}

#[cfg(windows)]
fn apply_file_mode(_temporary: &Path, _mode: u32) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn make_owner_writable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    let mut permissions = std::fs::metadata(path)?.permissions();
    permissions.set_mode(permissions.mode() | 0o200);
    std::fs::set_permissions(path, permissions)?;
    Ok(())
}

#[cfg(windows)]
fn make_owner_writable(path: &Path) -> Result<()> {
    let mut permissions = std::fs::metadata(path)?.permissions();
    permissions.set_readonly(false);
    std::fs::set_permissions(path, permissions)?;
    Ok(())
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

    fn staged_outputs(root: &Path, entries: Vec<(&[u8], PathBuf)>) -> StagedOutputs {
        let directory = tempfile::tempdir_in(root).unwrap();
        let files = entries
            .into_iter()
            .enumerate()
            .map(|(index, (contents, destination))| {
                let path = directory.path().join(format!("output-{index}"));
                std::fs::write(&path, contents).unwrap();
                (
                    tempfile::TempPath::try_from_path(path).unwrap(),
                    destination,
                )
            })
            .collect();
        StagedOutputs { directory, files }
    }

    fn test_outputs(root: &Path) -> RustcOutputs {
        let directory = root.join("out");
        RustcOutputs {
            files: vec![directory.join("libdemo.rlib")],
            dep_info: directory.join("demo.d"),
            directory,
        }
    }

    fn test_file(name: &str) -> CacheFileNode {
        CacheFileNode {
            digest: CacheDigest::blake3(b"artifact"),
            executable: false,
            mode: if cfg!(unix) { 0o644 } else { 0 },
            name: name.into(),
        }
    }

    fn test_directory(files: Vec<CacheFileNode>) -> CacheDirectory {
        CacheDirectory {
            directories: Vec::new(),
            files,
            symlinks: Vec::new(),
            version: 1,
        }
    }

    fn test_output_directory(file: CacheFileNode) -> CacheDirectory {
        test_directory(vec![file, test_file("demo.d")])
    }

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

    #[test]
    fn validates_exact_rustc_output_set() {
        let root = tempfile::tempdir().unwrap();
        let outputs = test_outputs(root.path());
        let files =
            validated_outputs(test_output_directory(test_file("libdemo.rlib")), &outputs).unwrap();

        assert_eq!(files.len(), 2);
        assert!(files.iter().any(|(_, path)| path == &outputs.files[0]));
        assert!(files.iter().any(|(_, path)| path == &outputs.dep_info));
    }

    #[test]
    fn rejects_cached_output_path_traversal() {
        let root = tempfile::tempdir().unwrap();
        let outputs = test_outputs(root.path());
        assert!(
            validated_outputs(
                test_output_directory(test_file("../libdemo.rlib")),
                &outputs,
            )
            .is_err()
        );
    }

    #[test]
    fn rejects_executable_rustc_outputs() {
        let root = tempfile::tempdir().unwrap();
        let outputs = test_outputs(root.path());
        let mut file = test_file("libdemo.rlib");
        file.executable = true;
        assert!(validated_outputs(test_output_directory(file), &outputs).is_err());
    }

    #[test]
    fn rejects_group_or_world_writable_rustc_outputs() {
        let root = tempfile::tempdir().unwrap();
        let outputs = test_outputs(root.path());
        let mut file = test_file("libdemo.rlib");
        file.mode = 0o666;
        assert!(validated_outputs(test_output_directory(file), &outputs).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn publication_masks_unsafe_rustc_output_permissions() {
        use std::os::unix::fs::PermissionsExt as _;

        let file = tempfile::NamedTempFile::new().unwrap();
        file.as_file()
            .set_permissions(std::fs::Permissions::from_mode(0o666))
            .unwrap();
        assert_eq!(file_mode(&file.as_file().metadata().unwrap()), 0o644);
    }

    #[test]
    fn rolls_back_outputs_after_a_partial_persist() {
        let root = tempfile::tempdir().unwrap();
        let first_destination = root.path().join("first.rlib");
        let blocked_destination = root.path().join("blocked.rmeta");
        std::fs::create_dir(&blocked_destination).unwrap();
        let staged = staged_outputs(
            root.path(),
            vec![
                (b"first", first_destination.clone()),
                (b"second", blocked_destination.clone()),
            ],
        );

        assert!(persist_outputs(staged).is_err());
        assert!(!first_destination.exists());
        assert!(blocked_destination.is_dir());
    }

    #[test]
    fn qualification_does_not_publish_cached_outputs() {
        let root = tempfile::tempdir().unwrap();
        let destination = root.path().join("cached.rlib");
        let staged = staged_outputs(root.path(), vec![(b"cached", destination.clone())]);

        finalize_restored_outputs(staged, false).unwrap();

        assert!(!destination.exists());
    }

    #[test]
    fn materialized_outputs_are_independent_from_the_cas() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("cas-blob");
        std::fs::write(&source, b"artifact").unwrap();
        let staging = tempfile::tempdir_in(root.path()).unwrap();
        let node = test_file("artifact.rlib");

        let output = stage_cached_output(staging.path(), 0, &source, &node).unwrap();
        std::fs::write(&output, b"modified").unwrap();

        assert_eq!(std::fs::read(source).unwrap(), b"artifact");
        assert_eq!(std::fs::read(output).unwrap(), b"modified");
    }

    #[test]
    fn rejects_same_size_corrupt_cached_outputs() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("cas-blob");
        std::fs::write(&source, b"corrupt!").unwrap();
        let staging = tempfile::tempdir_in(root.path()).unwrap();
        let node = test_file("artifact.rlib");

        assert!(stage_cached_output(staging.path(), 0, &source, &node).is_err());
    }

    #[test]
    fn materializes_read_only_cached_outputs() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("cas-blob");
        std::fs::write(&source, b"artifact").unwrap();
        let mut permissions = std::fs::metadata(&source).unwrap().permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(&source, permissions).unwrap();
        let staging = tempfile::tempdir_in(root.path()).unwrap();
        let node = test_file("artifact.rlib");

        let output = stage_cached_output(staging.path(), 0, &source, &node).unwrap();

        assert_eq!(std::fs::read(output).unwrap(), b"artifact");
        assert!(std::fs::metadata(&source).unwrap().permissions().readonly());
        make_owner_writable(&source).unwrap();
    }

    #[test]
    fn rejects_same_size_corrupt_cached_metadata() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("cas-blob");
        std::fs::write(&source, b"corrupt!").unwrap();
        let digest = CacheDigest::blake3(b"artifact");

        assert!(read_verified_blob(&source, &digest, "test blob").is_err());
    }

    #[test]
    #[ignore = "local materialization benchmark"]
    fn benchmark_cached_output_materialization() {
        let size_mib = std::env::var("MISE_CACHE_BENCH_MIB")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(128);
        let iterations = std::env::var("MISE_CACHE_BENCH_ITERATIONS")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(4);
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("cas-blob");
        let mut source_file = std::fs::File::create(&source).unwrap();
        let chunk = vec![0x5a; 1024 * 1024];
        for _ in 0..size_mib {
            source_file.write_all(&chunk).unwrap();
        }
        source_file.sync_all().unwrap();
        drop(source_file);
        let digest = CacheDigest::blake3_file(&source).unwrap();
        let node = CacheFileNode {
            digest: digest.clone(),
            executable: false,
            mode: if cfg!(unix) { 0o644 } else { 0 },
            name: "artifact.rlib".into(),
        };

        let started = std::time::Instant::now();
        for _ in 0..iterations {
            let mut temporary = tempfile::NamedTempFile::new_in(root.path()).unwrap();
            let mut input = std::fs::File::open(&source).unwrap();
            std::io::copy(&mut input, temporary.as_file_mut()).unwrap();
            temporary.flush().unwrap();
            temporary.as_file().sync_all().unwrap();
            assert!(digest.matches_file(temporary.path()).unwrap());
            apply_file_mode(temporary.path(), node.mode).unwrap();
        }
        let copied = started.elapsed();

        let staging = tempfile::tempdir_in(root.path()).unwrap();
        let started = std::time::Instant::now();
        for _ in 0..iterations {
            stage_cached_output(staging.path(), 0, &source, &node).unwrap();
        }
        let materialized = started.elapsed();

        println!(
            "materialized {iterations} x {size_mib} MiB: copied={copied:.2?}, reflink_or_copy={materialized:.2?}, speedup={:.2}x",
            copied.as_secs_f64() / materialized.as_secs_f64()
        );
    }
}
