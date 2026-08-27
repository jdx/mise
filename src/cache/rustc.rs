use super::session;
use eyre::{Context, Result, bail};
use mbx_cache_core::{
    ActionPrediction, AgentRequest, AgentResponse, CacheDigest, CacheDirectory, CacheFileNode,
    RemoteActionResult, RestoreStats, RustcMetadata, canonical_json,
};
use mbx_cache_rustc::{
    ActionContext, CompilerIdentity, DiscoveredInputs, PathMapping, RustcAction, RustcDepInfo,
    RustcInputPrediction, RustcInvocation, RustcOutputs, normalize_mapped_path,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, ExitStatus, Output};
use std::time::{Instant, SystemTime};

fn workspace_root(start: &Path) -> PathBuf {
    let mut lockfile = None;
    let mut manifest = None;
    for directory in start.ancestors() {
        if directory.join("Cargo.lock").is_file() {
            lockfile = Some(directory.to_path_buf());
        }
        if directory.join("Cargo.toml").is_file() {
            manifest = Some(directory.to_path_buf());
        }
    }
    lockfile.or(manifest).unwrap_or_else(|| start.to_path_buf())
}

struct CachedCompilation {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    outputs: Vec<CachedOutput>,
    restore: RestoreStats,
}

struct CachedOutput {
    path: PathBuf,
    digest: CacheDigest,
    executable: bool,
    mode: u32,
}

struct StagedOutputs {
    directory: tempfile::TempDir,
    files: Vec<(tempfile::TempPath, PathBuf)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Materialization {
    Reflink,
    Copy,
}

#[derive(Clone, Debug, Default)]
struct CompileTiming {
    crate_name: String,
    duration_ns: u64,
}

pub(crate) fn compile(rustc: &OsStr, arguments: &[OsString]) -> Result<ExitCode> {
    let working_dir = std::env::current_dir()?;
    // The orchestrated session supplies the target root. A persistent wrapper
    // has no parent session, so first parse just enough of the invocation to
    // learn its output directory and use that as the stable target mapping.
    let initial_invocation = RustcInvocation::parse(arguments)?;
    let initial_outputs = initial_invocation.outputs(&working_dir)?;
    let portable = Portable::detect(
        &working_dir,
        Some(&initial_outputs.directory),
        initial_invocation.target(),
    );
    let arguments = portable.applied_to(arguments);
    let invocation = RustcInvocation::parse(&arguments)?;
    let outputs = invocation.outputs(&working_dir)?;

    let verify = session::verify_requested();
    let mut verification = None;
    let mut action_lookup_attempted = false;
    if outputs.dep_info.is_file()
        && let Ok((candidates, discovered)) = action_from_current_dep_info(
            rustc,
            &invocation,
            &outputs.dep_info,
            &working_dir,
            &portable,
        )
    {
        action_lookup_attempted = true;
        match restore_candidates(&candidates, &outputs, &discovered, !verify) {
            Ok(Some((action, mut cached))) => {
                match prediction_timing(rustc, &invocation, &working_dir, &portable) {
                    Ok(timing) => {
                        cached.restore.avoided_compiler_duration_ns = timing.duration_ns;
                        record_prediction(
                            rustc,
                            &invocation,
                            &action,
                            &discovered,
                            &working_dir,
                            &portable,
                            &timing,
                        );
                    }
                    Err(error) => {
                        eprintln!(
                            "mise rustc cache warning: compiler timing was not refreshed: {error:#}"
                        );
                    }
                }
                if verify {
                    verification = Some(cached);
                } else {
                    record_action_hit(&action, cached.restore);
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
        match restore_predicted_result(
            rustc,
            &invocation,
            &outputs,
            &working_dir,
            &portable,
            !verify,
            &mut action_lookup_attempted,
        ) {
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
    let compiler_timer = Instant::now();
    let output = Command::new(rustc)
        .args(&arguments)
        .current_dir(&working_dir)
        .output()
        .wrap_err("failed to execute rustc")?;
    let timing = CompileTiming {
        crate_name: invocation.crate_name().to_string(),
        duration_ns: compiler_timer
            .elapsed()
            .as_nanos()
            .try_into()
            .unwrap_or(u64::MAX),
    };
    session::record_compiler_invocation(
        if verification.is_some() {
            "verification"
        } else if action_lookup_attempted {
            "miss"
        } else {
            "unconsulted"
        },
        Some(&timing.crate_name),
        timing.duration_ns,
    );
    let _ = replay_output(&output);
    if let Some(cached) = verification {
        let matched = cached_matches(&cached, &output);
        record_verification(matched, cached.restore);
        if !matched {
            eprintln!("mise rustc cache warning: shadow verification diverged from cached output");
        }
        return Ok(exit_code(output.status));
    }
    if output.status.success() {
        let publication: Result<()> = (|| {
            let (candidates, discovered) = action_from_dep_info(
                rustc,
                &invocation,
                &outputs.dep_info,
                &working_dir,
                &portable,
            )?;
            discovered.verify_not_modified_since(compilation_started)?;
            discovered.verify()?;
            let action = candidates.publishable(&portable, &outputs.files)?;
            publish_result(&action.digest, &action.bytes, &outputs, &output)?;
            record_prediction(
                rustc,
                &invocation,
                &action.digest,
                &discovered,
                &working_dir,
                &portable,
                &timing,
            );
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
    portable: &Portable,
    restore_outputs: bool,
    action_lookup_attempted: &mut bool,
) -> Result<Option<CachedCompilation>> {
    let mut context = base_action_context(rustc, working_dir, portable)?;
    let invocation_digest = invocation.invocation_digest(&context)?;
    let task = prediction_task(&invocation_digest);
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
        AgentResponse::ActionPrediction { prediction: None } => {
            // No usable action key: either no dep-info from an earlier build or
            // dep-info that did not yield one, and now no prediction either.
            // This compilation runs without an action-result lookup ever being
            // made, which is not a miss and has to be counted as its own thing
            // or the summary reads as though a lookup happened and found
            // nothing.
            session::record_unconsulted();
            return Ok(None);
        }
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
    let candidates = ActionCandidates::build(invocation, context)?;
    // From this point onward, every return follows at least one action-result
    // request, including error responses from a corrupt local record.
    *action_lookup_attempted = true;
    let restored = restore_candidates(&candidates, outputs, &discovered, restore_outputs)?;
    match restored {
        Some((action, mut cached)) => {
            cached.restore.avoided_compiler_duration_ns = input_prediction.compiler_duration_ns;
            if restore_outputs {
                record_action_hit(&action, cached.restore);
            }
            record_prediction_value(invocation_digest, action, prediction.payload);
            Ok(Some(cached))
        }
        None => Ok(None),
    }
}

/// The keys one compilation may be published under, most portable first.
///
/// A compilation whose environment holds nothing portable has exactly one key,
/// the literal one, which is what every action looked like before
/// [`Portable`] existed.
struct ActionCandidates {
    /// Normalizes the portable environment values, so two checkouts agree.
    portable: Option<RustcAction>,
    /// What the compilation falls back to when an output carries one of those
    /// values anyway.
    literal: RustcAction,
}

impl ActionCandidates {
    fn build(invocation: &RustcInvocation, context: ActionContext) -> Result<Self> {
        // Only worth a second key if a portable name is actually an input here.
        // Crates that never read one keep the key they always had.
        let applies = context
            .portable_environment
            .iter()
            .any(|name| context.environment.contains_key(name));
        let literal_context = ActionContext {
            portable_environment: BTreeSet::new(),
            ..context.clone()
        };
        Ok(Self {
            portable: applies.then(|| invocation.action(context)).transpose()?,
            literal: invocation.action(literal_context)?,
        })
    }

    /// The key this compilation is published under.
    ///
    /// The portable key is only honest if no output carries the value it
    /// normalized away. `--remap-path-prefix` covers the paths rustc records
    /// itself, but not one a crate reads through `env!` and keeps as a string,
    /// and nothing in the inputs distinguishes the two shapes -- so the outputs
    /// are read.
    fn publishable(&self, portable: &Portable, outputs: &[PathBuf]) -> Result<&RustcAction> {
        match &self.portable {
            Some(action) if portable.outputs_are_clean(outputs)? => Ok(action),
            _ => Ok(&self.literal),
        }
    }

    /// Every key to look up, most portable first.
    fn ordered(&self) -> impl Iterator<Item = &RustcAction> {
        self.portable.iter().chain(std::iter::once(&self.literal))
    }
}

/// Try each candidate key, returning the digest that hit alongside its result.
///
/// Both keys are tried because either shape may be on the other side of the
/// lookup: a crate that keeps `OUT_DIR` in a string was published literally,
/// and without the second lookup it would never hit, not even in the checkout
/// that compiled it.
fn restore_candidates(
    candidates: &ActionCandidates,
    outputs: &RustcOutputs,
    discovered: &DiscoveredInputs,
    restore_outputs: bool,
) -> Result<Option<(CacheDigest, CachedCompilation)>> {
    for action in candidates.ordered() {
        if let Some(cached) = restore_result(action, outputs, discovered, restore_outputs)? {
            return Ok(Some((action.digest.clone(), cached)));
        }
    }
    Ok(None)
}

fn action_from_dep_info(
    rustc: &OsStr,
    invocation: &RustcInvocation,
    dep_info: &Path,
    working_dir: &Path,
    portable: &Portable,
) -> Result<(ActionCandidates, DiscoveredInputs)> {
    let dep_info = RustcDepInfo::read(dep_info)?;
    action_from_parsed_dep_info(rustc, invocation, &dep_info, working_dir, portable)
}

fn action_from_current_dep_info(
    rustc: &OsStr,
    invocation: &RustcInvocation,
    dep_info: &Path,
    working_dir: &Path,
    portable: &Portable,
) -> Result<(ActionCandidates, DiscoveredInputs)> {
    let dep_info = RustcDepInfo::read(dep_info)?;
    verify_environment(&dep_info.environment)?;
    action_from_parsed_dep_info(rustc, invocation, &dep_info, working_dir, portable)
}

fn action_from_parsed_dep_info(
    rustc: &OsStr,
    invocation: &RustcInvocation,
    dep_info: &RustcDepInfo,
    working_dir: &Path,
    portable: &Portable,
) -> Result<(ActionCandidates, DiscoveredInputs)> {
    let discovered =
        invocation.discover_inputs_with_mappings(dep_info, working_dir, &portable.mappings)?;
    let mut context = base_action_context(rustc, working_dir, portable)?;
    discovered.clone().apply_to(&mut context)?;
    let candidates = ActionCandidates::build(invocation, context)?;
    Ok((candidates, discovered))
}

fn base_action_context(
    rustc: &OsStr,
    working_dir: &Path,
    portable: &Portable,
) -> Result<ActionContext> {
    Ok(ActionContext {
        compiler: compiler_identity(rustc)?,
        working_dir: working_dir.to_path_buf(),
        path_mappings: portable.mappings.clone(),
        environment: BTreeMap::new(),
        portable_environment: portable.names.clone(),
        inputs: Vec::new(),
    })
}

fn record_prediction(
    rustc: &OsStr,
    invocation: &RustcInvocation,
    action: &CacheDigest,
    discovered: &DiscoveredInputs,
    working_dir: &Path,
    portable: &Portable,
    timing: &CompileTiming,
) {
    let result = (|| {
        let context = base_action_context(rustc, working_dir, portable)?;
        let invocation_digest = invocation.invocation_digest(&context)?;
        let mut prediction = invocation.prediction(&context, discovered)?;
        prediction.version = prediction.version.max(2);
        prediction.compiler_duration_ns = timing.duration_ns;
        prediction.crate_name.clone_from(&timing.crate_name);
        let payload = String::from_utf8(canonical_json(&prediction)?)?;
        record_prediction_value(invocation_digest, action.clone(), payload);
        Result::<()>::Ok(())
    })();
    if let Err(error) = result {
        eprintln!("mise rustc cache warning: action prediction was not recorded: {error:#}");
    }
}

fn prediction_timing(
    rustc: &OsStr,
    invocation: &RustcInvocation,
    working_dir: &Path,
    portable: &Portable,
) -> Result<CompileTiming> {
    let context = base_action_context(rustc, working_dir, portable)?;
    let invocation_digest = invocation.invocation_digest(&context)?;
    let task = prediction_task(&invocation_digest);
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
        AgentResponse::ActionPrediction { prediction: None } => {
            return Ok(CompileTiming::default());
        }
        AgentResponse::Error { message } => bail!(message),
        _ => bail!("cache agent returned an unexpected action prediction response"),
    };
    decode_prediction_timing(&prediction, &invocation_digest)
}

fn decode_prediction_timing(
    prediction: &ActionPrediction,
    invocation: &CacheDigest,
) -> Result<CompileTiming> {
    if prediction.adapter != "rustc" || prediction.invocation != *invocation {
        bail!("cache agent returned an incompatible rustc timing prediction");
    }
    let timing: RustcInputPrediction = serde_json::from_str(&prediction.payload)?;
    if !matches!(timing.version, 1..=3)
        || timing.crate_name.len() > 256
        || timing.crate_name.contains(['\0', '\n', '\r'])
        || String::from_utf8(canonical_json(&timing)?)? != prediction.payload
    {
        bail!("cache agent returned an invalid rustc timing prediction");
    }
    Ok(CompileTiming {
        crate_name: timing.crate_name,
        duration_ns: timing.compiler_duration_ns,
    })
}

fn record_prediction_value(invocation: CacheDigest, action: CacheDigest, payload: String) {
    let result = (|| {
        let task = prediction_task(&invocation);
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

/// Select the session run, or a bounded persistent-manifest shard when this
/// shim was installed directly in Cargo configuration.
fn prediction_task(invocation: &CacheDigest) -> String {
    std::env::var(session::BUILD_ENV).unwrap_or_else(|_| {
        // A global manifest would eventually hit the prediction count limit.
        // Sharding by the invocation digest keeps related reads and writes
        // together while bounding each manifest independently.
        let shard = invocation.hash.get(..2).unwrap_or(&invocation.hash);
        CacheDigest::blake3(format!("standalone-predictions-v1\0{shard}").as_bytes()).hash
    })
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
            executable: node.executable,
            mode: node.mode,
        })
        .collect();
    let restored_output_files = files.len().try_into().unwrap_or(u64::MAX);
    let restored_output_bytes = files.iter().fold(0_u64, |total, (node, _)| {
        total.saturating_add(node.digest.size)
    });

    let mut digests = vec![metadata.stdout.clone(), metadata.stderr.clone()];
    digests.extend(files.iter().map(|(node, _)| node.digest.clone()));
    let blobs = find_blobs(&digests)?;
    let stdout = read_verified_blob(&blobs[0], &metadata.stdout, "stdout")?;
    let stderr = read_verified_blob(&blobs[1], &metadata.stderr, "stderr")?;

    let materialization_started = Instant::now();
    std::fs::create_dir_all(&outputs.directory)?;
    let staging = tempfile::tempdir_in(&outputs.directory)?;
    let mut staged = Vec::with_capacity(files.len());
    let mut restore = RestoreStats {
        output_files: restored_output_files,
        output_bytes: restored_output_bytes,
        ..RestoreStats::default()
    };
    for (index, ((node, destination), source)) in files.into_iter().zip(&blobs[2..]).enumerate() {
        let (temporary, materialization) =
            stage_verified_cached_output(staging.path(), index, source, &node)?;
        match materialization {
            Materialization::Reflink => {
                restore.reflinked_output_files = restore.reflinked_output_files.saturating_add(1);
                restore.reflinked_output_bytes = restore
                    .reflinked_output_bytes
                    .saturating_add(node.digest.size);
            }
            Materialization::Copy => {
                restore.copied_output_files = restore.copied_output_files.saturating_add(1);
                restore.copied_output_bytes =
                    restore.copied_output_bytes.saturating_add(node.digest.size);
            }
        }
        staged.push((temporary, destination));
    }
    let staged = StagedOutputs {
        directory: staging,
        files: staged,
    };

    discovered.verify()?;
    verify_environment(&discovered.environment)?;
    finalize_restored_outputs(staged, restore_outputs)?;
    restore.duration_ns = materialization_started
        .elapsed()
        .as_nanos()
        .try_into()
        .unwrap_or(u64::MAX);
    Ok(Some(CachedCompilation {
        stdout,
        stderr,
        outputs: cached_outputs,
        restore,
    }))
}

fn finalize_restored_outputs(staged: StagedOutputs, restore_outputs: bool) -> Result<()> {
    if restore_outputs {
        persist_outputs(staged)?;
    }
    Ok(())
}

fn stage_verified_cached_output(
    directory: &Path,
    index: usize,
    source: &Path,
    node: &CacheFileNode,
) -> Result<(tempfile::TempPath, Materialization)> {
    let temporary = directory.join(format!("output-{index}"));
    let copied_bytes = reflink_copy::reflink_or_copy(source, &temporary)
        .wrap_err_with(|| format!("failed to materialize cached rustc output {}", node.name))?;
    let materialization = match copied_bytes {
        None => Materialization::Reflink,
        Some(written) if written == node.digest.size => Materialization::Copy,
        Some(_) => bail!(
            "materialized cached rustc output has the wrong size: {}",
            node.name
        ),
    };
    let temporary = tempfile::TempPath::try_from_path(temporary)?;
    make_owner_writable(&temporary)?;
    // Deliberately not fsynced. These are build artifacts in a target
    // directory, and cargo does not sync its own outputs either, so syncing
    // here buys no durability the build relies on -- it only costs one fsync
    // per restored file, which on a large workspace is most of the restore.
    // `source` is a session-verified CAS path returned by `FindBlobs`. Hashing
    // the result again would read every output a second time and, for a
    // reflink, eagerly fault the shared data blocks that cloning was intended
    // to leave deferred. A reflink is a CoW snapshot and the copy fallback
    // reports the number of bytes it wrote, so checking the staged length is
    // sufficient after the agent's content verification.
    if std::fs::metadata(&temporary)?.len() != node.digest.size {
        bail!(
            "materialized cached rustc output has the wrong size: {}",
            node.name
        );
    }
    apply_file_mode(&temporary, node.mode, node.executable)?;
    Ok((temporary, materialization))
}

fn cached_matches(cached: &CachedCompilation, output: &Output) -> bool {
    output.status.success()
        && cached.stdout == output.stdout
        && cached.stderr == output.stderr
        && cached.outputs.iter().all(|expected| {
            std::fs::metadata(&expected.path).is_ok_and(|metadata| {
                file_mode(&metadata) == expected.mode
                    && executable_mode_matches(&metadata, expected.executable)
                    && expected
                        .digest
                        .matches_file(&expected.path)
                        .unwrap_or(false)
            })
        })
}

fn record_verification(matched: bool, restore: RestoreStats) {
    let responses =
        session::request_agent(&[AgentRequest::RecordActionVerification { matched, restore }]);
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

fn record_action_hit(action: &CacheDigest, restore: RestoreStats) {
    let responses = session::request_agent(&[AgentRequest::RecordActionHit {
        action: action.clone(),
        restore,
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
    let responses = session::request_agent(&[AgentRequest::FindBlobs {
        digests: digests.to_vec(),
    }])?;
    let Some(response) = responses.into_iter().next() else {
        bail!("cache agent did not return a blob lookup response");
    };
    match response {
        AgentResponse::Blobs { paths } if paths.len() == digests.len() => paths
            .into_iter()
            .zip(digests)
            .map(|(path, digest)| match path {
                Some(path) => Ok(path),
                None => bail!("cached rustc action is missing blob {}", digest.hash),
            })
            .collect(),
        AgentResponse::Blobs { .. } => {
            bail!("cache agent returned an incomplete blob lookup response")
        }
        AgentResponse::Blob { path: Some(path) } if digests.len() == 1 => Ok(vec![path]),
        AgentResponse::Blob { path: None } if digests.len() == 1 => {
            let digest = &digests[0];
            bail!("cached rustc action is missing blob {}", digest.hash)
        }
        AgentResponse::Error { message } => bail!(message),
        _ => bail!("cache agent returned an unexpected blob lookup response"),
    }
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
            Ok((
                name.to_string(),
                (path.clone(), outputs.is_executable(path)),
            ))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    if directory.files.len() != expected.len() {
        bail!("cached rustc output set does not match the invocation");
    }
    let mut files = Vec::with_capacity(directory.files.len());
    for node in directory.files {
        let (destination, executable) = expected
            .remove(&node.name)
            .ok_or_else(|| eyre::eyre!("cached rustc output is unexpected: {}", node.name))?;
        validate_file_mode(&node, executable)?;
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

/// Environment inputs eligible for remapping.
///
/// Deliberately just the one. `OUT_DIR` lives under the target directory, so
/// remapping it confines the change to generated sources, and it is the value
/// the plan identifies as the cross-checkout shortfall. Widening this list
/// widens which paths disappear from debug info, which is its own decision.
const PORTABLE_ENVIRONMENT: &[&str] = &["OUT_DIR"];

/// The environment values whose absolute paths this compilation was made
/// independent of.
///
/// `OUT_DIR` is the one that matters: every crate that includes build-script
/// output reads it, its value differs per checkout, and keeping it in the key
/// verbatim is what stops those compilations sharing between checkouts.
///
/// Two things must hold before a key may normalize such a value, and this type
/// is responsible for both. `--remap-path-prefix` makes rustc record the
/// placeholder instead of the real path, which covers debug info, spans, and
/// diagnostics -- everything rustc writes itself. It does not cover a value the
/// crate reads through `env!` and keeps as a string, so the outputs are read
/// before publishing and the portable key is used only if none carries it.
struct Portable {
    /// Path mappings for this compilation, ordered as keys need them.
    mappings: Vec<PathMapping>,
    /// Flags appended to the real rustc invocation, one per remapped value.
    arguments: Vec<OsString>,
    /// Names whose values an action key may normalize.
    names: BTreeSet<String>,
    /// The literal values, for the check before publishing.
    values: Vec<String>,
}

impl Portable {
    fn detect(working_dir: &Path, target_output: Option<&Path>, target: Option<&str>) -> Self {
        let mut portable = Self {
            mappings: PathMapping::ordered(&path_mappings(working_dir, target_output, target)),
            arguments: Vec::new(),
            names: BTreeSet::new(),
            values: Vec::new(),
        };
        if !session::share_out_dir_requested() {
            return portable;
        }
        for name in PORTABLE_ENVIRONMENT {
            let Some(value) = std::env::var(name)
                .ok()
                .filter(|value| Path::new(value).is_absolute())
            else {
                continue;
            };
            // A value under no known root is one no key could agree on anyway,
            // so there is nothing to remap and nothing to promise.
            let Ok(placeholder) =
                normalize_mapped_path(Path::new(&value), working_dir, &portable.mappings)
            else {
                continue;
            };
            let mut flag = OsString::from("--remap-path-prefix=");
            flag.push(&value);
            flag.push("=");
            flag.push(&placeholder);
            portable.arguments.push(flag);
            portable.names.insert((*name).to_string());
            portable.values.push(value);
        }
        portable
    }

    /// The compiler arguments, with the remapping flags appended.
    fn applied_to(&self, arguments: &[OsString]) -> Vec<OsString> {
        let mut applied = arguments.to_vec();
        applied.extend(self.arguments.iter().cloned());
        applied
    }

    /// Whether the outputs are free of every value a portable key normalized.
    ///
    /// The dep-info file is not one of them: it records absolute input paths by
    /// construction, and is restored as written for every action that already
    /// shares across checkouts today. Judging the artifact by it would reject
    /// every compilation.
    fn outputs_are_clean(&self, outputs: &[PathBuf]) -> Result<bool> {
        if self.values.is_empty() {
            return Ok(false);
        }
        for output in outputs {
            let contents = std::fs::read(output)
                .wrap_err_with(|| format!("failed to read rustc output {}", output.display()))?;
            if self.values.iter().any(|value| carries(&contents, value)) {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

/// Whether `contents` holds `value` anywhere, in either separator spelling.
///
/// rustc writes paths with the platform separator in some places and forward
/// slashes in others, and a value missed here becomes a wrong answer rather
/// than a slow one, so both spellings are searched.
fn carries(contents: &[u8], value: &str) -> bool {
    if contains(contents, value.as_bytes()) {
        return true;
    }
    value.contains('\\') && contains(contents, value.replace('\\', "/").as_bytes())
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    let Some((first, rest)) = needle.split_first() else {
        return false;
    };
    let mut offset = 0;
    while let Some(index) = haystack[offset..].iter().position(|byte| byte == first) {
        let start = offset + index;
        if haystack[start + 1..].starts_with(rest) {
            return true;
        }
        offset = start + 1;
    }
    false
}

fn path_mappings(
    working_dir: &Path,
    target_output: Option<&Path>,
    target: Option<&str>,
) -> Vec<PathMapping> {
    path_mappings_with_env(working_dir, target_output, target, |name| {
        std::env::var_os(name)
    })
}

fn path_mappings_with_env(
    working_dir: &Path,
    target_output: Option<&Path>,
    target: Option<&str>,
    environment: impl Fn(&str) -> Option<OsString>,
) -> Vec<PathMapping> {
    let mut mappings = Vec::new();
    let mut roots = BTreeSet::new();
    let home_roots = ["HOME", "USERPROFILE"]
        .into_iter()
        .filter_map(|name| environment(name).map(PathBuf::from))
        .filter(|root| root.is_absolute())
        .collect::<Vec<_>>();
    // The target directory comes first, and before the workspace that usually
    // contains it: output paths are the ones that differ between checkouts, and
    // mapping them explicitly also keeps keys stable when the target directory
    // is moved out of the workspace.
    //
    // Cargo compiles a dependency with its working directory inside the
    // registry, not in the workspace, so neither root can be inferred from the
    // working directory -- the session passes both in.
    let configured_target = environment(session::TARGET_DIR_ENV)
        .map(PathBuf::from)
        .filter(|root| root.is_absolute());
    if let Some(root) = configured_target.or_else(|| {
        target_output
            .filter(|root| root.is_absolute())
            .map(|output| standalone_target_root(output, target))
    }) {
        add_mapping(&mut mappings, &mut roots, root, "target");
    }
    for (name, placeholder) in [
        (session::WORKSPACE_ROOT_ENV, "workspace"),
        ("CARGO_HOME", "cargo_home"),
        ("RUSTUP_HOME", "rustup_home"),
    ] {
        if let Some(root) = environment(name).map(PathBuf::from)
            && root.is_absolute()
        {
            add_mapping(&mut mappings, &mut roots, root, placeholder);
        }
    }
    if let Some(home) = home_roots.first() {
        for (directory, placeholder) in [(".cargo", "cargo_home"), (".rustup", "rustup_home")] {
            if !mappings
                .iter()
                .any(|mapping| mapping.placeholder == placeholder)
            {
                add_mapping(&mut mappings, &mut roots, home.join(directory), placeholder);
            }
        }
    }
    // Without a session, recover Cargo's workspace root from the outermost
    // lockfile so member crates use the same placeholder as session mode.
    if !mappings
        .iter()
        .any(|mapping| mapping.placeholder == "workspace")
        && !roots.iter().any(|root| working_dir.starts_with(root))
    {
        add_mapping(
            &mut mappings,
            &mut roots,
            workspace_root(working_dir),
            "workspace",
        );
    }
    // Home is deliberately last. Most real checkouts live under it, but a
    // checkout-specific prefix must be `${workspace}` so equivalent worktrees
    // agree on their source paths. Cargo and rustup roots come first because a
    // registry compilation uses one of those as its working directory.
    for root in home_roots {
        add_mapping(&mut mappings, &mut roots, root, "home");
    }
    mappings
}

/// Infer the profile subtree shared by rustc outputs and build-script output.
///
/// Cargo normally writes compilations to `<target>/<profile>/deps` (or the
/// same shape below a target-triple directory). Mapping the profile parent,
/// rather than only `deps`, also covers generated inputs below `build/`.
fn standalone_target_root(output: &Path, target: Option<&str>) -> PathBuf {
    if output.file_name() == Some(OsStr::new("deps"))
        && let Some(profile_root) = output.parent().and_then(Path::parent)
    {
        let target_component = target.and_then(|target| Path::new(target).file_stem());
        if target_component.is_some_and(|target| profile_root.file_name() == Some(target))
            && let Some(root) = profile_root.parent()
        {
            return root.to_path_buf();
        }
        return profile_root.to_path_buf();
    }
    output.to_path_buf()
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
    let root = match std::env::var_os(session::STAGING_ENV).filter(|root| !root.is_empty()) {
        Some(root) => PathBuf::from(root),
        None => mbx_cache_cargo::cache_root()
            .ok_or_else(|| eyre::eyre!("could not determine the mbx cache directory"))?
            .join("actions/standalone-staging"),
    };
    std::fs::create_dir_all(&root)?;
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
    outputs: &RustcOutputs,
    output: &Output,
) -> Result<()> {
    if outputs.files.is_empty() {
        bail!("rustc produced no cacheable outputs");
    }
    let staging = staging_directory()?;
    let mut blobs = vec![staged_bytes(staging.path(), "action.json", action_bytes)?];
    let stdout = staged_bytes(staging.path(), "stdout", &output.stdout)?;
    let stderr = staged_bytes(staging.path(), "stderr", &output.stderr)?;
    blobs.extend([stdout.clone(), stderr.clone()]);

    let output_paths = outputs
        .files
        .iter()
        .chain(std::iter::once(&outputs.dep_info));
    let mut files = Vec::with_capacity(outputs.files.len() + 1);
    for path in output_paths {
        let metadata = std::fs::metadata(path)
            .wrap_err_with(|| format!("failed to inspect rustc output {}", path.display()))?;
        if !metadata.is_file() {
            bail!("rustc output is not a regular file: {}", path.display());
        }
        let digest = CacheDigest::blake3_file(path)?;
        blobs.push((digest.clone(), path.clone()));
        files.push(CacheFileNode {
            digest,
            executable: outputs.is_executable(path),
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
fn validate_file_mode(node: &CacheFileNode, executable: bool) -> Result<()> {
    if node.executable != executable
        || node.mode & !0o777 != 0
        || node.mode & 0o111 != 0
        || node.mode & 0o022 != 0
    {
        bail!("cached rustc output has an unsafe file mode: {}", node.name);
    }
    Ok(())
}

#[cfg(windows)]
fn validate_file_mode(node: &CacheFileNode, executable: bool) -> Result<()> {
    if node.executable != executable || node.mode != 0 {
        bail!("cached rustc output has an unsafe file mode: {}", node.name);
    }
    Ok(())
}

#[cfg(unix)]
fn apply_file_mode(temporary: &Path, mode: u32, executable: bool) -> Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    let executable_mode = if executable { 0o111 } else { 0 };
    std::fs::set_permissions(
        temporary,
        std::fs::Permissions::from_mode(mode | executable_mode),
    )?;
    Ok(())
}

#[cfg(windows)]
fn apply_file_mode(_temporary: &Path, _mode: u32, _executable: bool) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn executable_mode_matches(metadata: &std::fs::Metadata, executable: bool) -> bool {
    use std::os::unix::fs::PermissionsExt as _;
    (metadata.permissions().mode() & 0o111 != 0) == executable
}

#[cfg(windows)]
fn executable_mode_matches(_metadata: &std::fs::Metadata, _executable: bool) -> bool {
    true
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
#[path = "rustc_tests.rs"]
mod tests;
