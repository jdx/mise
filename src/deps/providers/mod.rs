mod aube;
mod bun;
mod bundler;
mod composer;
mod custom;
mod dart;
mod deno;
mod git_submodule;
mod go;
mod npm;
mod pip;
mod pnpm;
mod poetry;
mod uv;
mod yarn;

pub use aube::AubeDepsProvider;
pub use bun::BunDepsProvider;
pub use bundler::BundlerDepsProvider;
pub use composer::ComposerDepsProvider;
pub use custom::CustomDepsProvider;
pub use dart::DartDepsProvider;
pub use deno::DenoDepsProvider;
pub use git_submodule::GitSubmoduleDepsProvider;
pub use go::GoDepsProvider;
pub use npm::NpmDepsProvider;
pub use pip::PipDepsProvider;
pub use pnpm::PnpmDepsProvider;
pub use poetry::PoetryDepsProvider;
pub use uv::UvDepsProvider;
pub use yarn::YarnDepsProvider;

use std::path::{Path, PathBuf};

use glob::glob;

use crate::deps::rule::DepsProviderConfig;
use crate::task::task_source_checker::expand_glob_braces;

/// Shared base for all deps providers, holding the id, project root, and config.
/// Provides common implementations for `id` and `is_auto`.
#[derive(Debug)]
pub struct ProviderBase {
    pub(crate) id: String,
    pub(crate) project_root: PathBuf,
    pub(crate) config: DepsProviderConfig,
}

impl ProviderBase {
    pub fn new(id: impl Into<String>, project_root: &Path, config: DepsProviderConfig) -> Self {
        Self {
            id: id.into(),
            project_root: project_root.to_path_buf(),
            config,
        }
    }

    pub fn is_auto(&self) -> bool {
        self.config.auto
    }

    /// Returns the effective root directory for resolving sources/outputs.
    /// When `dir` is set in config, returns `project_root/dir`; otherwise `project_root`.
    pub fn config_root(&self) -> PathBuf {
        match &self.config.dir {
            Some(dir) => self.project_root.join(dir),
            None => self.project_root.clone(),
        }
    }

    pub fn sources(&self, default: Vec<PathBuf>) -> Vec<PathBuf> {
        self.config
            .sources
            .as_deref()
            .map(|patterns| self.resolve_path_patterns(patterns, false))
            .unwrap_or(default)
    }

    pub fn outputs(&self, default: Vec<PathBuf>) -> Vec<PathBuf> {
        self.config
            .outputs
            .as_deref()
            .map(|patterns| self.resolve_path_patterns(patterns, true))
            .unwrap_or(default)
    }

    pub fn optional_outputs(&self, default: Vec<PathBuf>) -> Vec<PathBuf> {
        if self.config.outputs.is_some() {
            vec![]
        } else {
            default
        }
    }

    /// Returns a stable identity for the configured output rules without
    /// expanding glob matches. This distinguishes omitted defaults, explicit
    /// replacements, and an explicit empty list while keeping the identity
    /// unchanged when files matching a glob are added or removed.
    pub fn output_rules_hash(&self) -> String {
        let rules = serde_json::to_string(&(&self.config.dir, &self.config.outputs))
            .expect("deps output rules should serialize");
        crate::hash::hash_blake3_to_str(&rules)
    }

    fn resolve_path_patterns(&self, patterns: &[String], require_glob_match: bool) -> Vec<PathBuf> {
        let mut paths = vec![];

        for pattern in patterns {
            let expanded = match expand_glob_braces(pattern) {
                Ok(expanded) => expanded,
                Err(err) => {
                    debug!("invalid deps path pattern {pattern:?}: {err}");
                    vec![pattern.clone()]
                }
            };
            for pattern in expanded {
                let path = PathBuf::from(&pattern);
                let full_pattern = if path.is_relative() {
                    self.config_root().join(&pattern)
                } else {
                    path
                };

                if pattern.contains('*') || pattern.contains('?') || pattern.contains('[') {
                    if let Ok(entries) = glob(full_pattern.to_string_lossy().as_ref()) {
                        let matches: Vec<_> = entries.flatten().collect();
                        if matches.is_empty() && require_glob_match {
                            // Preserve an unmatched required output pattern as
                            // a non-existent path so freshness remains stale
                            // until the provider produces at least one match.
                            paths.push(full_pattern);
                        } else {
                            paths.extend(matches);
                        }
                    } else if require_glob_match {
                        paths.push(full_pattern);
                    }
                } else {
                    paths.push(full_pattern);
                }
            }
        }

        paths
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn path_overrides_distinguish_omitted_explicit_and_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let default_source = tmp.path().join("default.lock");
        let default_output = tmp.path().join("default-output");
        let optional_output = tmp.path().join("optional-output");

        let base = ProviderBase::new("test", tmp.path(), DepsProviderConfig::default());
        assert_eq!(
            base.sources(vec![default_source.clone()]),
            vec![default_source.clone()]
        );
        assert_eq!(
            base.outputs(vec![default_output.clone()]),
            vec![default_output.clone()]
        );
        assert_eq!(
            base.optional_outputs(vec![optional_output.clone()]),
            vec![optional_output]
        );

        let explicit = ProviderBase::new(
            "test",
            tmp.path(),
            DepsProviderConfig {
                sources: Some(vec!["custom.lock".into()]),
                outputs: Some(vec!["custom-output".into()]),
                ..Default::default()
            },
        );
        assert_eq!(
            explicit.sources(vec![default_source]),
            vec![tmp.path().join("custom.lock")]
        );
        assert_eq!(
            explicit.outputs(vec![default_output]),
            vec![tmp.path().join("custom-output")]
        );
        assert!(explicit.optional_outputs(vec![]).is_empty());

        let empty = ProviderBase::new(
            "test",
            tmp.path(),
            DepsProviderConfig {
                sources: Some(vec![]),
                outputs: Some(vec![]),
                ..Default::default()
            },
        );
        assert!(empty.sources(vec![tmp.path().join("ignored")]).is_empty());
        assert!(empty.outputs(vec![tmp.path().join("ignored")]).is_empty());
        assert!(
            empty
                .optional_outputs(vec![tmp.path().join("ignored")])
                .is_empty()
        );
    }

    #[test]
    fn path_overrides_resolve_from_dir_and_expand_globs() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("project");
        let work = root.join("packages/app");
        fs::create_dir_all(work.join("inputs")).unwrap();
        fs::write(work.join("inputs/a.lock"), "a").unwrap();
        fs::write(work.join("inputs/b.lock"), "b").unwrap();
        fs::write(work.join("inputs/a.json"), "a").unwrap();
        fs::write(work.join("inputs/b.json"), "b").unwrap();
        let absolute = tmp.path().join("absolute-output");

        let base = ProviderBase::new(
            "test",
            &root,
            DepsProviderConfig {
                dir: Some("packages/app".into()),
                sources: Some(vec!["inputs/*.lock".into()]),
                outputs: Some(vec![absolute.to_string_lossy().into_owned()]),
                ..Default::default()
            },
        );

        assert_eq!(
            base.sources(vec![]),
            vec![work.join("inputs/a.lock"), work.join("inputs/b.lock")]
        );
        assert_eq!(base.outputs(vec![]), vec![absolute]);

        let bracket_pattern = ProviderBase::new(
            "test",
            &root,
            DepsProviderConfig {
                dir: Some("packages/app".into()),
                sources: Some(vec!["inputs/[ab].lock".into()]),
                ..Default::default()
            },
        );
        assert_eq!(
            bracket_pattern.sources(vec![]),
            vec![work.join("inputs/a.lock"), work.join("inputs/b.lock")]
        );

        let brace_pattern = ProviderBase::new(
            "test",
            &root,
            DepsProviderConfig {
                dir: Some("packages/app".into()),
                sources: Some(vec!["inputs/{a,b}.json".into()]),
                outputs: Some(vec!["outputs/{app,vendor}.js".into()]),
                ..Default::default()
            },
        );
        assert_eq!(
            brace_pattern.sources(vec![]),
            vec![work.join("inputs/a.json"), work.join("inputs/b.json")]
        );
        assert_eq!(
            brace_pattern.outputs(vec![]),
            vec![work.join("outputs/app.js"), work.join("outputs/vendor.js")]
        );

        let output_pattern = ProviderBase::new(
            "test",
            &root,
            DepsProviderConfig {
                dir: Some("packages/app".into()),
                outputs: Some(vec!["outputs/*.js".into()]),
                ..Default::default()
            },
        );
        assert_eq!(
            output_pattern.outputs(vec![]),
            vec![work.join("outputs/*.js")]
        );
        fs::create_dir_all(work.join("outputs")).unwrap();
        fs::write(work.join("outputs/app.js"), "output").unwrap();
        assert_eq!(
            output_pattern.outputs(vec![]),
            vec![work.join("outputs/app.js")]
        );
    }

    #[test]
    fn output_rules_hash_tracks_declared_rules_not_glob_matches() {
        let tmp = tempfile::tempdir().unwrap();
        let config = DepsProviderConfig {
            outputs: Some(vec!["dist/*.js".into()]),
            ..Default::default()
        };
        let base = ProviderBase::new("test", tmp.path(), config.clone());
        let hash = base.output_rules_hash();

        fs::create_dir_all(tmp.path().join("dist")).unwrap();
        fs::write(tmp.path().join("dist/app.js"), "output").unwrap();
        assert_eq!(base.output_rules_hash(), hash);

        let omitted = ProviderBase::new("test", tmp.path(), DepsProviderConfig::default());
        let empty = ProviderBase::new(
            "test",
            tmp.path(),
            DepsProviderConfig {
                outputs: Some(vec![]),
                ..Default::default()
            },
        );
        let replacement = ProviderBase::new(
            "test",
            tmp.path(),
            DepsProviderConfig {
                outputs: Some(vec!["build/*.js".into()]),
                ..Default::default()
            },
        );
        let different_dir = ProviderBase::new(
            "test",
            tmp.path(),
            DepsProviderConfig {
                dir: Some("package".into()),
                ..config
            },
        );

        assert_ne!(omitted.output_rules_hash(), empty.output_rules_hash());
        assert_ne!(hash, replacement.output_rules_hash());
        assert_ne!(hash, different_dir.output_rules_hash());
    }
}
