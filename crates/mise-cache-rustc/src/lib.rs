use mise_cache_core::{CacheDigest, canonical_json};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};
use thiserror::Error;

mod dep_info;

pub use dep_info::{DepInfoCommand, DiscoveredInputs, RustcDepInfo};

pub const ACTION_SCHEMA_VERSION: u8 = 1;
pub const ADAPTER_VERSION: u8 = 1;

const SUPPORTED_CODEGEN_OPTIONS: &[&str] = &[
    "codegen-units",
    "control-flow-guard",
    "debug-assertions",
    "debuginfo",
    "default-linker-libraries",
    "embed-bitcode",
    "extra-filename",
    "force-frame-pointers",
    "force-unwind-tables",
    "instrument-coverage",
    "link-dead-code",
    "link-self-contained",
    "lto",
    "metadata",
    "no-prepopulate-passes",
    "opt-level",
    "overflow-checks",
    "panic",
    "prefer-dynamic",
    "relocation-model",
    "rpath",
    "save-temps",
    "soft-float",
    "split-debuginfo",
    "split-dwarf-kind",
    "strip",
    "symbol-mangling-version",
    "target-cpu",
    "target-feature",
    "tls-model",
];

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum BypassReason {
    #[error("rustc argument {index} is not valid UTF-8")]
    NonUtf8Argument { index: usize },
    #[error("rustc response files are not supported: {0}")]
    ResponseFile(String),
    #[error("rustc flag is not modeled by the cache adapter: {0}")]
    UnknownFlag(String),
    #[error("rustc codegen option is not modeled by the cache adapter: {0}")]
    UnknownCodegenOption(String),
    #[error("rustc flag requires a value: {0}")]
    MissingValue(String),
    #[error("rustc invocation is a compiler query, not a compilation")]
    CompilerQuery,
    #[error("rustc invocation reads source from standard input")]
    StandardInput,
    #[error("rustc invocation has no source input")]
    MissingInput,
    #[error("rustc invocation has multiple source inputs")]
    MultipleInputs,
    #[error("incremental compilation cannot be combined with action caching")]
    Incremental,
    #[error("rustc crate type is not cacheable yet: {0}")]
    UnsupportedCrateType(String),
    #[error("rustc output type is not cacheable yet: {0}")]
    UnsupportedEmit(String),
    #[error("rustc invocation does not emit an rlib or metadata artifact")]
    NoCacheableOutput,
    #[error("native library lookup is not cacheable yet")]
    NativeLibrary,
    #[error("rustc search path kind is not cacheable yet: {0}")]
    UnsupportedSearchPath(String),
    #[error("rustc extern does not identify an input artifact: {0}")]
    UnresolvedExtern(String),
    #[error("absolute path has no stable cache mapping: {0}")]
    UnmappedAbsolutePath(PathBuf),
    #[error("cache key paths must be valid UTF-8: {0}")]
    NonUtf8Path(PathBuf),
    #[error("cache action working directory must be absolute: {0}")]
    RelativeWorkingDirectory(PathBuf),
    #[error("cache path mapping must use an absolute root: {0}")]
    RelativePathMapping(PathBuf),
    #[error("cache path mapping placeholder is invalid: {0}")]
    InvalidPathPlaceholder(String),
    #[error("required compiler input was not provided: {0}")]
    MissingRequiredInput(String),
    #[error("compiler input has an invalid digest: {0}")]
    InvalidInputDigest(String),
    #[error("compiler input appears more than once with different content: {0}")]
    ConflictingInput(String),
    #[error("rustc dep-info is malformed: {0}")]
    MalformedDepInfo(String),
    #[error("failed to read rustc dep-info {path}: {message}")]
    DepInfoRead { path: PathBuf, message: String },
    #[error("rustc dep-info output path must be absolute: {0}")]
    RelativeDepInfoPath(PathBuf),
    #[error("rustc dep-info output path cannot contain a comma: {0}")]
    UnsafeDepInfoPath(PathBuf),
    #[error("failed to read compiler input {path}: {message}")]
    InputRead { path: PathBuf, message: String },
    #[error("compiler input changed after discovery: {0}")]
    InputChanged(PathBuf),
    #[error("discovered inputs were collected from a different working directory")]
    DiscoveryWorkingDirectory,
    #[error("compiler environment input has conflicting values: {0}")]
    ConflictingEnvironment(String),
    #[error("failed to serialize the rustc action: {0}")]
    Serialization(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Argument {
    Plain(String),
    Path { flag: String, path: PathBuf },
    SearchPath { kind: String, path: PathBuf },
    Extern { name: String, path: Option<PathBuf> },
    Emit(Vec<Emit>),
    RemapPath { from: PathBuf, to: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Emit {
    kind: String,
    path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustcInvocation {
    arguments: Vec<Argument>,
    source: PathBuf,
    required_inputs: Vec<PathBuf>,
}

impl RustcInvocation {
    /// Parse rustc's arguments, excluding the compiler executable supplied as
    /// the first argument to `RUSTC_WRAPPER`.
    ///
    /// Any flag whose cache semantics are not modeled returns a bypass reason
    /// instead of guessing. A successful parse only admits the initial
    /// rlib/rmeta cacheability tier.
    pub fn parse(arguments: &[OsString]) -> Result<Self, BypassReason> {
        Parser::new(arguments).parse()
    }

    pub fn source(&self) -> &Path {
        &self.source
    }

    /// Build canonical action bytes after precise input discovery has run.
    ///
    /// `context.inputs` must contain the source, every explicit extern, and
    /// every additional source or environment-generated input discovered from
    /// dep-info.
    pub fn action(&self, context: ActionContext) -> Result<RustcAction, BypassReason> {
        ActionBuilder::new(self, context).build()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathMapping {
    pub root: PathBuf,
    pub placeholder: String,
}

impl PathMapping {
    pub fn new(root: impl Into<PathBuf>, placeholder: impl Into<String>) -> Self {
        Self {
            root: root.into(),
            placeholder: placeholder.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompilerIdentity {
    pub toolchain: String,
    pub rustc_version: String,
    pub host: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionInput {
    pub path: PathBuf,
    pub digest: CacheDigest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionContext {
    pub compiler: CompilerIdentity,
    pub working_dir: PathBuf,
    pub path_mappings: Vec<PathMapping>,
    pub environment: BTreeMap<String, Option<String>>,
    pub inputs: Vec<ActionInput>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustcAction {
    pub digest: CacheDigest,
    pub bytes: Vec<u8>,
}

#[derive(Serialize)]
struct ActionDescriptor {
    version: u8,
    kind: &'static str,
    adapter_version: u8,
    compiler: CompilerDescriptor,
    arguments: Vec<String>,
    environment: BTreeMap<String, Option<String>>,
    inputs: Vec<InputDescriptor>,
}

#[derive(Serialize)]
struct CompilerDescriptor {
    toolchain: String,
    rustc_version: String,
    host: String,
}

#[derive(Debug, Serialize, PartialEq, Eq, PartialOrd, Ord)]
struct InputDescriptor {
    path: String,
    digest: CacheDigest,
}

struct Parser<'a> {
    arguments: &'a [OsString],
    index: usize,
    parsed: Vec<Argument>,
    source: Option<PathBuf>,
    crate_types: Vec<String>,
    emits: Vec<Emit>,
    required_inputs: Vec<PathBuf>,
    test: bool,
}

impl<'a> Parser<'a> {
    fn new(arguments: &'a [OsString]) -> Self {
        Self {
            arguments,
            index: 0,
            parsed: Vec::new(),
            source: None,
            crate_types: Vec::new(),
            emits: Vec::new(),
            required_inputs: Vec::new(),
            test: false,
        }
    }

    fn parse(mut self) -> Result<RustcInvocation, BypassReason> {
        while self.index < self.arguments.len() {
            let value = self.current()?.to_string();
            self.index += 1;
            if value.starts_with('@') {
                return Err(BypassReason::ResponseFile(value));
            }
            if let Some(long) = value.strip_prefix("--") {
                self.parse_long(long)?;
            } else if value.starts_with('-') && value != "-" {
                self.parse_short(&value)?;
            } else {
                self.parse_input(&value)?;
            }
        }

        let source = self.source.clone().ok_or(BypassReason::MissingInput)?;
        self.classify()?;
        self.required_inputs.push(source.clone());
        Ok(RustcInvocation {
            arguments: self.parsed,
            source,
            required_inputs: self.required_inputs,
        })
    }

    fn current(&self) -> Result<&str, BypassReason> {
        self.arguments[self.index]
            .to_str()
            .ok_or(BypassReason::NonUtf8Argument { index: self.index })
    }

    fn take_value(&mut self, flag: &str, inline: Option<&str>) -> Result<String, BypassReason> {
        if let Some(value) = inline {
            if value.is_empty() {
                return Err(BypassReason::MissingValue(flag.into()));
            }
            return Ok(value.into());
        }
        if self.index >= self.arguments.len() {
            return Err(BypassReason::MissingValue(flag.into()));
        }
        let value = self.current()?.to_string();
        self.index += 1;
        Ok(value)
    }

    fn parse_long(&mut self, value: &str) -> Result<(), BypassReason> {
        let (flag, inline) = value
            .split_once('=')
            .map_or((value, None), |(flag, value)| (flag, Some(value)));
        let rendered_flag = format!("--{flag}");
        match flag {
            "help" | "version" | "explain" | "print" => Err(BypassReason::CompilerQuery),
            "test" => {
                self.test = true;
                self.parsed.push(Argument::Plain(rendered_flag));
                Ok(())
            }
            "verbose" => {
                self.parsed.push(Argument::Plain(rendered_flag));
                Ok(())
            }
            "cfg" | "check-cfg" | "crate-name" | "edition" | "error-format" | "json" | "color"
            | "diagnostic-width" | "remap-path-scope" | "allow" | "warn" | "force-warn"
            | "deny" | "forbid" | "cap-lints" => {
                let value = self.take_value(&rendered_flag, inline)?;
                self.parsed
                    .push(Argument::Plain(format!("{rendered_flag}={value}")));
                Ok(())
            }
            "target" => {
                let value = self.take_value(&rendered_flag, inline)?;
                if value.ends_with(".json") || value.contains(['/', '\\']) {
                    let path = PathBuf::from(value);
                    self.required_inputs.push(path.clone());
                    self.parsed.push(Argument::Path {
                        flag: rendered_flag,
                        path,
                    });
                } else {
                    self.parsed
                        .push(Argument::Plain(format!("{rendered_flag}={value}")));
                }
                Ok(())
            }
            "crate-type" => {
                let value = self.take_value(&rendered_flag, inline)?;
                self.crate_types
                    .extend(value.split(',').map(ToOwned::to_owned));
                self.parsed
                    .push(Argument::Plain(format!("{rendered_flag}={value}")));
                Ok(())
            }
            "emit" => {
                let value = self.take_value(&rendered_flag, inline)?;
                let emits = parse_emits(&value);
                self.emits.extend(emits.clone());
                self.parsed.push(Argument::Emit(emits));
                Ok(())
            }
            "out-dir" | "sysroot" => {
                let path = self.take_value(&rendered_flag, inline)?;
                self.parsed.push(Argument::Path {
                    flag: rendered_flag,
                    path: path.into(),
                });
                Ok(())
            }
            "extern" => {
                let value = self.take_value(&rendered_flag, inline)?;
                let (name, path) = value
                    .split_once('=')
                    .map_or((value.as_str(), None), |(name, path)| {
                        (name, Some(PathBuf::from(path)))
                    });
                if let Some(path) = &path {
                    self.required_inputs.push(path.clone());
                }
                self.parsed.push(Argument::Extern {
                    name: name.into(),
                    path,
                });
                Ok(())
            }
            "remap-path-prefix" => {
                let value = self.take_value(&rendered_flag, inline)?;
                let Some((from, to)) = value.split_once('=') else {
                    return Err(BypassReason::MissingValue(rendered_flag));
                };
                self.parsed.push(Argument::RemapPath {
                    from: from.into(),
                    to: to.into(),
                });
                Ok(())
            }
            "codegen" => {
                let value = self.take_value(&rendered_flag, inline)?;
                self.parse_codegen(&value)
            }
            _ => Err(BypassReason::UnknownFlag(rendered_flag)),
        }
    }

    fn parse_short(&mut self, value: &str) -> Result<(), BypassReason> {
        match value {
            "-h" | "-V" => return Err(BypassReason::CompilerQuery),
            "-g" | "-O" | "-v" => {
                self.parsed.push(Argument::Plain(value.into()));
                return Ok(());
            }
            _ => {}
        }
        for (short, long) in [
            ("-A", "--allow"),
            ("-W", "--warn"),
            ("-D", "--deny"),
            ("-F", "--forbid"),
        ] {
            if let Some(attached) = value.strip_prefix(short) {
                let lint = self.take_value(short, (!attached.is_empty()).then_some(attached))?;
                self.parsed.push(Argument::Plain(format!("{long}={lint}")));
                return Ok(());
            }
        }
        if let Some(attached) = value.strip_prefix("-C") {
            let option = self.take_value("-C", (!attached.is_empty()).then_some(attached))?;
            return self.parse_codegen(&option);
        }
        if let Some(attached) = value.strip_prefix("-L") {
            let search = self.take_value("-L", (!attached.is_empty()).then_some(attached))?;
            let (kind, path) = search
                .split_once('=')
                .map_or(("all", search.as_str()), |(kind, path)| (kind, path));
            if kind != "dependency" {
                return Err(BypassReason::UnsupportedSearchPath(kind.into()));
            }
            self.parsed.push(Argument::SearchPath {
                kind: kind.into(),
                path: path.into(),
            });
            return Ok(());
        }
        if value == "-l" || value.starts_with("-l") {
            return Err(BypassReason::NativeLibrary);
        }
        if let Some(attached) = value.strip_prefix("-o") {
            let path = self.take_value("-o", (!attached.is_empty()).then_some(attached))?;
            self.parsed.push(Argument::Path {
                flag: "-o".into(),
                path: path.into(),
            });
            return Ok(());
        }
        Err(BypassReason::UnknownFlag(value.into()))
    }

    fn parse_codegen(&mut self, value: &str) -> Result<(), BypassReason> {
        let name = value.split_once('=').map_or(value, |(name, _)| name);
        if name == "incremental" {
            return Err(BypassReason::Incremental);
        }
        if SUPPORTED_CODEGEN_OPTIONS.binary_search(&name).is_err() {
            return Err(BypassReason::UnknownCodegenOption(name.into()));
        }
        self.parsed
            .push(Argument::Plain(format!("--codegen={value}")));
        Ok(())
    }

    fn parse_input(&mut self, value: &str) -> Result<(), BypassReason> {
        if value == "-" {
            return Err(BypassReason::StandardInput);
        }
        if self.source.replace(value.into()).is_some() {
            return Err(BypassReason::MultipleInputs);
        }
        Ok(())
    }

    fn classify(&self) -> Result<(), BypassReason> {
        if self.crate_types.is_empty() {
            return Err(BypassReason::UnsupportedCrateType("bin".into()));
        }
        if let Some(crate_type) = self
            .crate_types
            .iter()
            .find(|crate_type| !matches!(crate_type.as_str(), "lib" | "rlib"))
        {
            return Err(BypassReason::UnsupportedCrateType(crate_type.clone()));
        }
        if self.test {
            return Err(BypassReason::UnsupportedCrateType("test".into()));
        }
        if let Some(name) = self.parsed.iter().find_map(|argument| match argument {
            Argument::Extern { name, path: None } if name != "proc_macro" => Some(name),
            _ => None,
        }) {
            return Err(BypassReason::UnresolvedExtern(name.clone()));
        }
        if let Some(emit) = self
            .emits
            .iter()
            .find(|emit| !matches!(emit.kind.as_str(), "dep-info" | "link" | "metadata"))
        {
            return Err(BypassReason::UnsupportedEmit(emit.kind.clone()));
        }
        if !self
            .emits
            .iter()
            .any(|emit| matches!(emit.kind.as_str(), "link" | "metadata"))
        {
            return Err(BypassReason::NoCacheableOutput);
        }
        Ok(())
    }
}

fn parse_emits(value: &str) -> Vec<Emit> {
    value
        .split(',')
        .map(|emit| {
            let (kind, path) = emit
                .split_once('=')
                .map_or((emit, None), |(kind, path)| (kind, Some(path.into())));
            Emit {
                kind: kind.into(),
                path,
            }
        })
        .collect()
}

struct ActionBuilder<'a> {
    invocation: &'a RustcInvocation,
    context: ActionContext,
    mappings: Vec<PathMapping>,
}

impl<'a> ActionBuilder<'a> {
    fn new(invocation: &'a RustcInvocation, mut context: ActionContext) -> Self {
        context
            .path_mappings
            .sort_by_key(|mapping| std::cmp::Reverse(mapping.root.components().count()));
        Self {
            invocation,
            mappings: context.path_mappings.clone(),
            context,
        }
    }

    fn build(self) -> Result<RustcAction, BypassReason> {
        self.validate_mappings()?;
        let arguments = self
            .invocation
            .arguments
            .iter()
            .map(|argument| self.normalize_argument(argument))
            .collect::<Result<Vec<_>, _>>()?;
        // rustc may embed these values verbatim through `env!`; unlike paths
        // used to locate inputs and outputs, changing them changes the artifact.
        let environment = self.context.environment.clone();

        let mut inputs = BTreeMap::<String, CacheDigest>::new();
        for input in &self.context.inputs {
            input
                .digest
                .validate()
                .map_err(|_| BypassReason::InvalidInputDigest(input.path.display().to_string()))?;
            let path = self.normalize_path(&input.path)?;
            if inputs
                .insert(path.clone(), input.digest.clone())
                .is_some_and(|existing| existing != input.digest)
            {
                return Err(BypassReason::ConflictingInput(path));
            }
        }
        let required = self
            .invocation
            .required_inputs
            .iter()
            .map(|path| self.normalize_path(path))
            .collect::<Result<BTreeSet<_>, _>>()?;
        if let Some(missing) = required.iter().find(|path| !inputs.contains_key(*path)) {
            return Err(BypassReason::MissingRequiredInput(missing.clone()));
        }
        let inputs = inputs
            .into_iter()
            .map(|(path, digest)| InputDescriptor { path, digest })
            .collect();
        let descriptor = ActionDescriptor {
            version: ACTION_SCHEMA_VERSION,
            kind: "rustc",
            adapter_version: ADAPTER_VERSION,
            compiler: CompilerDescriptor {
                toolchain: self.context.compiler.toolchain,
                rustc_version: self.context.compiler.rustc_version,
                host: self.context.compiler.host,
            },
            arguments,
            environment,
            inputs,
        };
        let bytes = canonical_json(&descriptor)
            .map_err(|error| BypassReason::Serialization(error.to_string()))?;
        let digest = CacheDigest::blake3(&bytes);
        Ok(RustcAction { digest, bytes })
    }

    fn validate_mappings(&self) -> Result<(), BypassReason> {
        if !self.context.working_dir.is_absolute() {
            return Err(BypassReason::RelativeWorkingDirectory(
                self.context.working_dir.clone(),
            ));
        }
        let mut roots = BTreeSet::new();
        let mut placeholders = BTreeSet::new();
        for mapping in &self.mappings {
            if !mapping.root.is_absolute() {
                return Err(BypassReason::RelativePathMapping(mapping.root.clone()));
            }
            if mapping.placeholder.is_empty()
                || !mapping
                    .placeholder
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
                || !roots.insert(normalize_components(&mapping.root))
                || !placeholders.insert(&mapping.placeholder)
            {
                return Err(BypassReason::InvalidPathPlaceholder(
                    mapping.placeholder.clone(),
                ));
            }
        }
        Ok(())
    }

    fn normalize_argument(&self, argument: &Argument) -> Result<String, BypassReason> {
        match argument {
            Argument::Plain(value) => Ok(value.clone()),
            Argument::Path { flag, path } => Ok(format!("{flag}={}", self.normalize_path(path)?)),
            Argument::SearchPath { kind, path } => {
                Ok(format!("-L{kind}={}", self.normalize_path(path)?))
            }
            Argument::Extern { name, path } => match path {
                Some(path) => Ok(format!("--extern={name}={}", self.normalize_path(path)?)),
                None => Ok(format!("--extern={name}")),
            },
            Argument::Emit(emits) => Ok(format!(
                "--emit={}",
                emits
                    .iter()
                    .map(|emit| match &emit.path {
                        Some(path) => self
                            .normalize_path(path)
                            .map(|path| format!("{}={path}", emit.kind)),
                        None => Ok(emit.kind.clone()),
                    })
                    .collect::<Result<Vec<_>, _>>()?
                    .join(",")
            )),
            Argument::RemapPath { from, to } => Ok(format!(
                "--remap-path-prefix={}={}",
                self.normalize_path(from)?,
                to
            )),
        }
    }

    fn normalize_path(&self, path: &Path) -> Result<String, BypassReason> {
        let absolute = if path.is_absolute() {
            normalize_components(path)
        } else {
            normalize_components(&self.context.working_dir.join(path))
        };
        for mapping in &self.mappings {
            let root = normalize_components(&mapping.root);
            if let Ok(relative) = absolute.strip_prefix(&root) {
                let suffix = slash_path(relative)?;
                return Ok(if suffix.is_empty() {
                    format!("${{{}}}", mapping.placeholder)
                } else {
                    format!("${{{}}}/{suffix}", mapping.placeholder)
                });
            }
        }
        Err(BypassReason::UnmappedAbsolutePath(absolute))
    }
}

fn normalize_components(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            component => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

fn slash_path(path: &Path) -> Result<String, BypassReason> {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(value) => Some(
                value
                    .to_str()
                    .map(ToOwned::to_owned)
                    .ok_or_else(|| BypassReason::NonUtf8Path(path.to_path_buf())),
            ),
            _ => None,
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|components| components.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    fn digest(value: &str) -> CacheDigest {
        CacheDigest::blake3(value.as_bytes())
    }

    fn absolute(segments: &[&str]) -> PathBuf {
        let mut path = if cfg!(windows) {
            PathBuf::from(r"C:\")
        } else {
            PathBuf::from("/")
        };
        path.extend(segments);
        path
    }

    fn workspace() -> PathBuf {
        absolute(&["work", "project"])
    }

    fn sysroot() -> PathBuf {
        absolute(&["toolchains", "1.97.1"])
    }

    fn context(inputs: &[(&str, &str)]) -> ActionContext {
        ActionContext {
            compiler: CompilerIdentity {
                toolchain: "core:rust@1.97.1".into(),
                rustc_version: "1.97.1 (8bab26f4f 2026-07-14)".into(),
                host: "x86_64-unknown-linux-gnu".into(),
            },
            working_dir: workspace(),
            path_mappings: vec![
                PathMapping::new(workspace().join("target"), "target"),
                PathMapping::new(workspace(), "workspace"),
                PathMapping::new(absolute(&["home", "user", ".cargo"]), "cargo_home"),
                PathMapping::new(sysroot(), "sysroot"),
            ],
            environment: BTreeMap::from([("CARGO_PKG_VERSION".into(), Some("1.0.0".into()))]),
            inputs: inputs
                .iter()
                .map(|(path, contents)| ActionInput {
                    path: (*path).into(),
                    digest: digest(contents),
                })
                .collect(),
        }
    }

    fn common_invocation() -> RustcInvocation {
        let output = workspace().join("target/debug/deps");
        RustcInvocation::parse(&[
            "--crate-name".into(),
            "widget".into(),
            "--edition=2024".into(),
            "src/lib.rs".into(),
            "--crate-type".into(),
            "lib".into(),
            "--emit=dep-info,metadata,link".into(),
            "-Cembed-bitcode=no".into(),
            "-C".into(),
            "metadata=abc123".into(),
            "--out-dir".into(),
            output.clone().into_os_string(),
            format!("-Ldependency={}", output.display()).into(),
            "--extern".into(),
            format!("serde={}", output.join("libserde.rlib").display()).into(),
            format!("--sysroot={}", sysroot().display()).into(),
            "--cap-lints".into(),
            "allow".into(),
        ])
        .unwrap()
    }

    #[test]
    fn parses_a_cargo_library_invocation() {
        let invocation = common_invocation();
        assert_eq!(invocation.source(), Path::new("src/lib.rs"));
        let action = invocation
            .action(context(&[
                ("src/lib.rs", "source"),
                ("target/debug/deps/libserde.rlib", "serde"),
            ]))
            .unwrap();
        let json = String::from_utf8(action.bytes).unwrap();
        assert!(json.contains(r#""kind":"rustc""#));
        assert!(json.contains(r#""--out-dir=${target}/debug/deps""#));
        assert!(json.contains(r#""--extern=serde=${target}/debug/deps/libserde.rlib""#));
        assert_eq!(action.digest.algorithm, "blake3");
    }

    #[test]
    fn equivalent_worktrees_produce_the_same_action_key() {
        let first_context = context(&[
            ("src/lib.rs", "source"),
            ("target/debug/deps/libserde.rlib", "serde"),
        ]);
        let first = common_invocation().action(first_context).unwrap();
        let other = absolute(&["other", "checkout"]);
        let output = other.join("target/debug/deps");
        let invocation = RustcInvocation::parse(&[
            "--crate-name=widget".into(),
            "--edition=2024".into(),
            "src/lib.rs".into(),
            "--crate-type=lib".into(),
            "--emit=dep-info,metadata,link".into(),
            "-Cembed-bitcode=no".into(),
            "-Cmetadata=abc123".into(),
            format!("--out-dir={}", output.display()).into(),
            format!("-Ldependency={}", output.display()).into(),
            format!("--extern=serde={}", output.join("libserde.rlib").display()).into(),
            format!("--sysroot={}", sysroot().display()).into(),
            "--cap-lints=allow".into(),
        ])
        .unwrap();
        let mut second_context = context(&[]);
        second_context.working_dir = other.clone();
        second_context.path_mappings[0].root = other.join("target");
        second_context.path_mappings[1].root = other.clone();
        second_context.inputs = vec![
            ActionInput {
                path: "src/lib.rs".into(),
                digest: digest("source"),
            },
            ActionInput {
                path: "target/debug/deps/libserde.rlib".into(),
                digest: digest("serde"),
            },
        ];
        let second = invocation.action(second_context).unwrap();
        assert_eq!(first.digest, second.digest);
    }

    #[test]
    fn absolute_environment_values_remain_literal_action_inputs() {
        let invocation = common_invocation();
        let mut first_context = context(&[
            ("src/lib.rs", "source"),
            ("target/debug/deps/libserde.rlib", "serde"),
        ]);
        let first_out_dir = workspace().join("target/debug/build/widget/out");
        first_context
            .environment
            .insert("OUT_DIR".into(), Some(first_out_dir.display().to_string()));
        let first = invocation.action(first_context).unwrap();

        let mut second_context = context(&[
            ("src/lib.rs", "source"),
            ("target/debug/deps/libserde.rlib", "serde"),
        ]);
        second_context.environment.insert(
            "OUT_DIR".into(),
            Some(absolute(&["other", "out"]).display().to_string()),
        );
        let second = invocation.action(second_context).unwrap();

        let descriptor = String::from_utf8(first.bytes).unwrap();
        assert!(descriptor.contains(&first_out_dir.display().to_string()));
        assert_ne!(first.digest, second.digest);
    }

    #[test]
    fn content_and_environment_change_the_action_key() {
        let invocation = common_invocation();
        let first = invocation
            .action(context(&[
                ("src/lib.rs", "source"),
                ("target/debug/deps/libserde.rlib", "serde"),
            ]))
            .unwrap();
        let changed_source = invocation
            .action(context(&[
                ("src/lib.rs", "changed"),
                ("target/debug/deps/libserde.rlib", "serde"),
            ]))
            .unwrap();
        let mut changed_environment = context(&[
            ("src/lib.rs", "source"),
            ("target/debug/deps/libserde.rlib", "serde"),
        ]);
        changed_environment
            .environment
            .insert("CARGO_PKG_VERSION".into(), Some("2.0.0".into()));
        let changed_environment = invocation.action(changed_environment).unwrap();
        assert_ne!(first.digest, changed_source.digest);
        assert_ne!(first.digest, changed_environment.digest);
    }

    #[test]
    fn unknown_and_incremental_options_bypass() {
        for (arguments, expected) in [
            (
                vec!["--future-flag", "src/lib.rs"],
                BypassReason::UnknownFlag("--future-flag".into()),
            ),
            (
                vec!["-Cfuture-option=yes", "src/lib.rs"],
                BypassReason::UnknownCodegenOption("future-option".into()),
            ),
            (
                vec!["-Cincremental=target/incremental", "src/lib.rs"],
                BypassReason::Incremental,
            ),
        ] {
            assert_eq!(RustcInvocation::parse(&args(&arguments)), Err(expected));
        }
    }

    #[test]
    fn linked_and_unmodeled_outputs_bypass() {
        for (arguments, expected) in [
            (
                vec!["--crate-type=bin", "--emit=link", "src/main.rs"],
                BypassReason::UnsupportedCrateType("bin".into()),
            ),
            (
                vec!["--crate-type=lib", "--emit=obj", "src/lib.rs"],
                BypassReason::UnsupportedEmit("obj".into()),
            ),
            (
                vec!["--crate-type=lib", "--emit=dep-info", "src/lib.rs"],
                BypassReason::NoCacheableOutput,
            ),
        ] {
            assert_eq!(RustcInvocation::parse(&args(&arguments)), Err(expected));
        }
    }

    #[test]
    fn action_requires_every_direct_input() {
        let error = common_invocation()
            .action(context(&[("src/lib.rs", "source")]))
            .unwrap_err();
        assert_eq!(
            error,
            BypassReason::MissingRequiredInput("${target}/debug/deps/libserde.rlib".into())
        );
    }

    #[test]
    fn action_rejects_unmapped_absolute_paths() {
        let unmapped = absolute(&["tmp", "rustc-output"]);
        let invocation = RustcInvocation::parse(&[
            "--crate-type=lib".into(),
            "--emit=link".into(),
            "src/lib.rs".into(),
            format!("--out-dir={}", unmapped.display()).into(),
        ])
        .unwrap();
        let error = invocation
            .action(context(&[("src/lib.rs", "source")]))
            .unwrap_err();
        assert_eq!(error, BypassReason::UnmappedAbsolutePath(unmapped));
    }

    #[test]
    fn custom_targets_are_required_inputs() {
        let invocation = RustcInvocation::parse(&args(&[
            "--crate-type=lib",
            "--emit=metadata",
            "--target=targets/custom.json",
            "src/lib.rs",
        ]))
        .unwrap();
        let error = invocation
            .action(context(&[("src/lib.rs", "source")]))
            .unwrap_err();
        assert_eq!(
            error,
            BypassReason::MissingRequiredInput("${workspace}/targets/custom.json".into())
        );
    }

    #[test]
    fn remap_destinations_are_stable_virtual_paths() {
        let invocation = RustcInvocation::parse(&[
            "--crate-type=lib".into(),
            "--emit=metadata".into(),
            format!("--remap-path-prefix={}=/src", workspace().display()).into(),
            "src/lib.rs".into(),
        ])
        .unwrap();
        let action = invocation
            .action(context(&[("src/lib.rs", "source")]))
            .unwrap();
        assert!(
            String::from_utf8(action.bytes)
                .unwrap()
                .contains(r#"--remap-path-prefix=${workspace}=/src"#)
        );
    }
}
