use std::collections::BTreeMap;
use std::path::PathBuf;

use eyre::Result;
use glob::glob;

use crate::prepare::rule::PrepareRule;
use crate::prepare::{PrepareCommand, PrepareProvider};

/// Prepare provider for user-defined rules from mise.toml [prepare.rules.*]
#[derive(Debug)]
pub struct CustomPrepareProvider {
    id: String,
    rule: PrepareRule,
    project_root: PathBuf,
}

impl CustomPrepareProvider {
    pub fn new(id: String, rule: PrepareRule, project_root: PathBuf) -> Self {
        Self {
            id,
            rule,
            project_root,
        }
    }

    /// Expand glob patterns in sources/outputs
    fn expand_globs(&self, patterns: &[String]) -> Vec<PathBuf> {
        let mut paths = vec![];

        for pattern in patterns {
            let full_pattern = if PathBuf::from(pattern).is_relative() {
                self.project_root.join(pattern)
            } else {
                PathBuf::from(pattern)
            };

            // Check if it's a glob pattern
            if pattern.contains('*') || pattern.contains('{') || pattern.contains('?') {
                if let Ok(entries) = glob(full_pattern.to_string_lossy().as_ref()) {
                    for entry in entries.flatten() {
                        paths.push(entry);
                    }
                }
            } else if full_pattern.exists() {
                paths.push(full_pattern);
            } else {
                // Include even if doesn't exist (for outputs that may not exist yet)
                paths.push(full_pattern);
            }
        }

        paths
    }
}

impl PrepareProvider for CustomPrepareProvider {
    fn id(&self) -> &str {
        &self.id
    }

    fn sources(&self) -> Vec<PathBuf> {
        self.expand_globs(&self.rule.sources)
    }

    fn outputs(&self) -> Vec<PathBuf> {
        self.expand_globs(&self.rule.outputs)
    }

    fn prepare_command(&self) -> Result<PrepareCommand> {
        let parts: Vec<&str> = self.rule.run.split_whitespace().collect();
        let (program, args) = parts
            .split_first()
            .ok_or_else(|| eyre::eyre!("prepare rule {} has empty run command", self.id))?;

        let env: BTreeMap<String, String> = self.rule.env.clone();

        let cwd = self
            .rule
            .dir
            .as_ref()
            .map(|d| self.project_root.join(d))
            .unwrap_or_else(|| self.project_root.clone());

        let description = self
            .rule
            .description
            .clone()
            .unwrap_or_else(|| format!("Running prepare rule: {}", self.id));

        Ok(PrepareCommand {
            program: program.to_string(),
            args: args.iter().map(|s| s.to_string()).collect(),
            env,
            cwd: Some(cwd),
            description,
        })
    }

    fn is_applicable(&self) -> bool {
        self.rule.enabled
    }

    fn priority(&self) -> u32 {
        self.rule.priority
    }
}
