use super::{
    ActionContext, ActionInput, Argument, BypassReason, RustcInvocation, normalize_components,
};
use mise_cache_core::CacheDigest;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// A side-effect-minimized rustc invocation that emits only dependency data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DepInfoCommand {
    arguments: Vec<OsString>,
    output: PathBuf,
}

impl DepInfoCommand {
    /// Arguments for the real compiler, excluding the compiler executable.
    pub fn arguments(&self) -> &[OsString] {
        &self.arguments
    }

    /// Exact file the compiler must populate with dep-info.
    pub fn output(&self) -> &Path {
        &self.output
    }
}

/// The source and environment inputs reported by rustc's dep-info output.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RustcDepInfo {
    pub files: Vec<PathBuf>,
    pub environment: BTreeMap<String, Option<String>>,
}

impl RustcDepInfo {
    /// Read and parse a dep-info file, treating missing or non-UTF-8 output as
    /// an explicit cache bypass.
    pub fn read(path: &Path) -> Result<Self, BypassReason> {
        let contents =
            std::fs::read_to_string(path).map_err(|error| BypassReason::DepInfoRead {
                path: path.to_path_buf(),
                message: error.to_string(),
            })?;
        Self::parse(&contents)
    }

    /// Parse rustc's Makefile-style dep-info format.
    ///
    /// This intentionally follows Cargo's parser contract: the first target
    /// rule contains all source dependencies, spaces are escaped with a
    /// trailing backslash on each token fragment, and `# env-dep:` records
    /// contain the environment observed by `env!` and `option_env!`.
    pub fn parse(contents: &str) -> Result<Self, BypassReason> {
        let mut files = BTreeSet::new();
        let mut environment = BTreeMap::new();
        let mut found_dependencies = false;

        for line in contents.lines() {
            if let Some(record) = line.strip_prefix("# env-dep:") {
                let (name, value) = record
                    .split_once('=')
                    .map_or((record, None), |(name, value)| (name, Some(value)));
                let name = unescape_environment(name)?;
                if name.is_empty() {
                    return Err(BypassReason::MalformedDepInfo(
                        "environment input has an empty name".into(),
                    ));
                }
                let value = value.map(unescape_environment).transpose()?;
                if environment
                    .insert(name.clone(), value.clone())
                    .is_some_and(|previous| previous != value)
                {
                    return Err(BypassReason::ConflictingEnvironment(name));
                }
                continue;
            }

            let Some(separator) = line.find(": ") else {
                continue;
            };
            if found_dependencies {
                continue;
            }
            found_dependencies = true;
            let mut fragments = line[separator + 2..].split_whitespace();
            while let Some(fragment) = fragments.next() {
                let mut file = fragment.to_string();
                while file.ends_with('\\') {
                    file.pop();
                    let continuation = fragments.next().ok_or_else(|| {
                        BypassReason::MalformedDepInfo(
                            "dependency path ends with an unterminated escape".into(),
                        )
                    })?;
                    file.push(' ');
                    file.push_str(continuation);
                }
                if file.is_empty() {
                    return Err(BypassReason::MalformedDepInfo(
                        "dependency path is empty".into(),
                    ));
                }
                files.insert(PathBuf::from(file));
            }
        }

        if !found_dependencies {
            return Err(BypassReason::MalformedDepInfo(
                "dependency rule is missing".into(),
            ));
        }
        if files.is_empty() {
            return Err(BypassReason::MalformedDepInfo(
                "dependency rule contains no inputs".into(),
            ));
        }
        Ok(Self {
            files: files.into_iter().collect(),
            environment,
        })
    }
}

/// A complete, content-addressed compiler input manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredInputs {
    working_dir: PathBuf,
    pub inputs: Vec<ActionInput>,
    pub environment: BTreeMap<String, Option<String>>,
}

impl DiscoveredInputs {
    /// Rehash every discovered file after compilation and before publication.
    /// This closes the discovery/compile race by degrading changed inputs to a
    /// cache miss rather than storing outputs beneath a stale action key.
    pub fn verify(&self) -> Result<(), BypassReason> {
        for input in &self.inputs {
            let matches = input.digest.matches_file(&input.path).map_err(|error| {
                BypassReason::InputRead {
                    path: input.path.clone(),
                    message: error.to_string(),
                }
            })?;
            if !matches {
                return Err(BypassReason::InputChanged(input.path.clone()));
            }
        }
        Ok(())
    }

    /// Merge the manifest into an action context after verifying that both use
    /// the same compiler working directory.
    pub fn apply_to(self, context: &mut ActionContext) -> Result<(), BypassReason> {
        if normalize_components(&context.working_dir) != self.working_dir {
            return Err(BypassReason::DiscoveryWorkingDirectory);
        }
        for (name, value) in &self.environment {
            if context
                .environment
                .get(name)
                .is_some_and(|previous| previous != value)
            {
                return Err(BypassReason::ConflictingEnvironment(name.clone()));
            }
        }
        context.environment.extend(self.environment);
        context.inputs.extend(self.inputs);
        Ok(())
    }
}

impl RustcInvocation {
    /// Replace the original output flags with a single explicit dep-info file.
    pub fn dep_info_command(&self, output: &Path) -> Result<DepInfoCommand, BypassReason> {
        if !output.is_absolute() {
            return Err(BypassReason::RelativeDepInfoPath(output.to_path_buf()));
        }
        let output_text = output
            .to_str()
            .ok_or_else(|| BypassReason::NonUtf8Path(output.to_path_buf()))?;
        if output_text.contains(',') {
            return Err(BypassReason::UnsafeDepInfoPath(output.to_path_buf()));
        }

        let mut arguments = Vec::new();
        for argument in &self.arguments {
            match argument {
                Argument::Emit(_) => {}
                Argument::Path { flag, .. } if flag == "--out-dir" || flag == "-o" => {}
                argument => arguments.push(render_argument(argument)?),
            }
        }
        arguments.push(format!("--emit=dep-info={output_text}").into());
        arguments.push(self.source.clone().into_os_string());
        Ok(DepInfoCommand {
            arguments,
            output: output.to_path_buf(),
        })
    }

    /// Hash dep-info sources plus every direct compiler input already modeled
    /// by the invocation (`--extern` artifacts and custom target specs).
    pub fn discover_inputs(
        &self,
        dep_info: &RustcDepInfo,
        working_dir: &Path,
    ) -> Result<DiscoveredInputs, BypassReason> {
        if !working_dir.is_absolute() {
            return Err(BypassReason::RelativeWorkingDirectory(
                working_dir.to_path_buf(),
            ));
        }
        let working_dir = normalize_components(working_dir);
        let paths = dep_info
            .files
            .iter()
            .chain(&self.required_inputs)
            .map(|path| {
                let absolute = if path.is_absolute() {
                    path.to_path_buf()
                } else {
                    working_dir.join(path)
                };
                normalize_components(&absolute)
            })
            .collect::<BTreeSet<_>>();
        let mut inputs = Vec::with_capacity(paths.len());
        for path in paths {
            let metadata = std::fs::metadata(&path).map_err(|error| BypassReason::InputRead {
                path: path.clone(),
                message: error.to_string(),
            })?;
            if !metadata.is_file() {
                return Err(BypassReason::InputRead {
                    path,
                    message: "input is not a regular file".into(),
                });
            }
            let digest =
                CacheDigest::blake3_file(&path).map_err(|error| BypassReason::InputRead {
                    path: path.clone(),
                    message: error.to_string(),
                })?;
            inputs.push(ActionInput { path, digest });
        }
        Ok(DiscoveredInputs {
            working_dir,
            inputs,
            environment: dep_info.environment.clone(),
        })
    }
}

fn render_argument(argument: &Argument) -> Result<OsString, BypassReason> {
    let rendered = match argument {
        Argument::Plain(value) => value.clone(),
        Argument::Path { flag, path } => format!(
            "{flag}={}",
            path.to_str()
                .ok_or_else(|| BypassReason::NonUtf8Path(path.clone()))?
        ),
        Argument::SearchPath { kind, path } => format!(
            "-L{kind}={}",
            path.to_str()
                .ok_or_else(|| BypassReason::NonUtf8Path(path.clone()))?
        ),
        Argument::Extern { name, path } => match path {
            Some(path) => format!(
                "--extern={name}={}",
                path.to_str()
                    .ok_or_else(|| BypassReason::NonUtf8Path(path.clone()))?
            ),
            None => format!("--extern={name}"),
        },
        Argument::Emit(_) => unreachable!("emit arguments are removed before rendering"),
        Argument::RemapPath { from, to } => format!(
            "--remap-path-prefix={}={to}",
            from.to_str()
                .ok_or_else(|| BypassReason::NonUtf8Path(from.clone()))?
        ),
    };
    Ok(rendered.into())
}

fn unescape_environment(value: &str) -> Result<String, BypassReason> {
    let mut output = String::with_capacity(value.len());
    let mut characters = value.chars();
    while let Some(character) = characters.next() {
        if character != '\\' {
            output.push(character);
            continue;
        }
        match characters.next() {
            Some('\\') => output.push('\\'),
            Some('n') => output.push('\n'),
            Some('r') => output.push('\r'),
            Some(character) => {
                return Err(BypassReason::MalformedDepInfo(format!(
                    "unknown environment escape \\{character}"
                )));
            }
            None => {
                return Err(BypassReason::MalformedDepInfo(
                    "environment input ends with an unterminated escape".into(),
                ));
            }
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn parses_files_spaces_and_environment_records() {
        let parsed = RustcDepInfo::parse(
            "target/lib.rlib: src/lib.rs src/a\\ file.rs generated.rs\n\
             src/lib.rs:\n\
             # env-dep:SET=value\\nnext\n\
             # env-dep:UNSET\n\
             # env-dep:SLASH=a\\\\b\n",
        )
        .unwrap();
        assert_eq!(
            parsed.files,
            vec![
                PathBuf::from("generated.rs"),
                PathBuf::from("src/a file.rs"),
                PathBuf::from("src/lib.rs"),
            ]
        );
        assert_eq!(parsed.environment["SET"], Some("value\nnext".into()));
        assert_eq!(parsed.environment["UNSET"], None);
        assert_eq!(parsed.environment["SLASH"], Some(r"a\b".into()));
    }

    #[test]
    fn malformed_dep_info_bypasses_caching() {
        for contents in [
            "",
            "target: ",
            "target: src/trailing\\\n",
            "target: src/lib.rs\n# env-dep:NAME=bad\\q\n",
        ] {
            assert!(RustcDepInfo::parse(contents).is_err(), "{contents:?}");
        }
    }

    #[test]
    fn discovery_command_removes_real_outputs() {
        let invocation = RustcInvocation::parse(&args(&[
            "--crate-name=widget",
            "--crate-type=lib",
            "--emit=dep-info,metadata,link",
            "--out-dir=target/debug/deps",
            "-o",
            "target/debug/libwidget.rlib",
            "src/lib.rs",
        ]))
        .unwrap();
        let output = if cfg!(windows) {
            PathBuf::from(r"C:\tmp\mise cache\inputs.d")
        } else {
            PathBuf::from("/tmp/mise cache/inputs.d")
        };
        let command = invocation.dep_info_command(&output).unwrap();
        let arguments = command
            .arguments()
            .iter()
            .map(|value| value.to_string_lossy())
            .collect::<Vec<_>>();
        assert_eq!(
            arguments,
            vec![
                "--crate-name=widget",
                "--crate-type=lib",
                &format!("--emit=dep-info={}", output.display()),
                "src/lib.rs",
            ]
        );
    }

    #[test]
    fn discovery_hashes_externs_and_custom_targets() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let source = root.join("src/lib.rs");
        let external = root.join("target/libdependency.rlib");
        let target = root.join("targets/custom.json");
        std::fs::create_dir_all(source.parent().unwrap()).unwrap();
        std::fs::create_dir_all(external.parent().unwrap()).unwrap();
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(&source, "pub fn library() {}\n").unwrap();
        std::fs::write(&external, "dependency artifact\n").unwrap();
        std::fs::write(&target, "{}\n").unwrap();

        let invocation = RustcInvocation::parse(&[
            "--crate-name=widget".into(),
            "--crate-type=lib".into(),
            "--emit=metadata".into(),
            format!("--extern=dependency={}", external.display()).into(),
            format!("--target={}", target.display()).into(),
            source.clone().into_os_string(),
        ])
        .unwrap();
        let dep_info = RustcDepInfo::parse(&format!("output: {}\n", source.display())).unwrap();
        let discovered = invocation.discover_inputs(&dep_info, root).unwrap();
        assert_eq!(discovered.inputs.len(), 3);

        std::fs::remove_file(&external).unwrap();
        assert!(matches!(
            invocation.discover_inputs(&dep_info, root),
            Err(BypassReason::InputRead { path, .. }) if path == external
        ));
    }

    #[test]
    fn discovery_resolves_parent_components_against_the_working_directory() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        let shared = directory.path().join("shared.rs");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(&shared, "pub fn shared() {}\n").unwrap();

        let invocation = RustcInvocation::parse(&args(&[
            "--crate-name=widget",
            "--crate-type=lib",
            "--emit=metadata",
            "../shared.rs",
        ]))
        .unwrap();
        let dep_info = RustcDepInfo::parse("output: ../shared.rs\n").unwrap();
        let discovered = invocation.discover_inputs(&dep_info, &root).unwrap();

        assert_eq!(discovered.inputs.len(), 1);
        assert_eq!(discovered.inputs[0].path, shared);
    }

    #[test]
    fn rustc_dep_info_round_trip_discovers_real_inputs() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        std::fs::write(
            root.join("lib.rs"),
            "mod child; const _: &str = include_str!(\"data file.txt\"); \
             const _: &str = env!(\"MISE_CACHE_DISCOVERY_TEST\"); \
             const _: Option<&str> = option_env!(\"MISE_CACHE_DISCOVERY_UNSET\");",
        )
        .unwrap();
        std::fs::write(root.join("child.rs"), "pub fn child() {}\n").unwrap();
        std::fs::write(root.join("data file.txt"), "included\n").unwrap();

        let invocation = RustcInvocation::parse(&args(&[
            "--crate-name=mise_cache_discovery_test",
            "--crate-type=lib",
            "--emit=metadata,link",
            "lib.rs",
        ]))
        .unwrap();
        let dep_info_path = root.join("discovery inputs.d");
        let discovery_command = invocation.dep_info_command(&dep_info_path).unwrap();
        let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
        let output = match Command::new(rustc)
            .args(discovery_command.arguments())
            .current_dir(root)
            .env("MISE_CACHE_DISCOVERY_TEST", "observed")
            .env_remove("MISE_CACHE_DISCOVERY_UNSET")
            .output()
        {
            Ok(output) => output,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
            Err(error) => panic!("failed to execute rustc: {error}"),
        };
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let parsed = RustcDepInfo::read(&dep_info_path).unwrap();
        let discovered = invocation.discover_inputs(&parsed, root).unwrap();
        assert_eq!(
            discovered.environment["MISE_CACHE_DISCOVERY_TEST"],
            Some("observed".into())
        );
        assert_eq!(discovered.environment["MISE_CACHE_DISCOVERY_UNSET"], None);
        assert_eq!(discovered.inputs.len(), 3);
        assert!(
            discovered
                .inputs
                .iter()
                .all(|input| input.digest.algorithm == "blake3")
        );
        let mut context = ActionContext {
            compiler: crate::CompilerIdentity {
                toolchain: "core:rust@test".into(),
                rustc_version: "test".into(),
                host: std::env::consts::ARCH.into(),
            },
            working_dir: root.to_path_buf(),
            path_mappings: vec![crate::PathMapping::new(root, "workspace")],
            environment: BTreeMap::new(),
            inputs: Vec::new(),
        };
        discovered.clone().apply_to(&mut context).unwrap();
        let action = invocation.action(context).unwrap();
        assert!(
            String::from_utf8(action.bytes)
                .unwrap()
                .contains(r#""MISE_CACHE_DISCOVERY_TEST":"observed""#)
        );
        discovered.verify().unwrap();
        std::fs::write(root.join("child.rs"), "pub fn changed() {}\n").unwrap();
        assert_eq!(
            discovered.verify(),
            Err(BypassReason::InputChanged(root.join("child.rs")))
        );
    }
}
