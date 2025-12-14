use std::collections::BTreeMap;
use std::path::PathBuf;

use eyre::Result;

use crate::prepare::rule::PrepareProviderConfig;
use crate::prepare::{PrepareCommand, PrepareProvider};

/// Package manager types that npm provider can handle
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PackageManager {
    Npm,
    Yarn,
    Pnpm,
    Bun,
}

impl PackageManager {
    fn install_command(&self) -> (&'static str, Vec<&'static str>) {
        match self {
            PackageManager::Npm => ("npm", vec!["install"]),
            PackageManager::Yarn => ("yarn", vec!["install"]),
            PackageManager::Pnpm => ("pnpm", vec!["install"]),
            PackageManager::Bun => ("bun", vec!["install"]),
        }
    }

    fn name(&self) -> &'static str {
        match self {
            PackageManager::Npm => "npm",
            PackageManager::Yarn => "yarn",
            PackageManager::Pnpm => "pnpm",
            PackageManager::Bun => "bun",
        }
    }
}

/// Prepare provider for Node.js package managers (npm, yarn, pnpm, bun)
#[derive(Debug)]
pub struct NpmPrepareProvider {
    project_root: PathBuf,
    package_manager: Option<PackageManager>,
    lockfile: Option<PathBuf>,
    config: Option<PrepareProviderConfig>,
}

impl NpmPrepareProvider {
    pub fn new(project_root: &PathBuf, config: Option<&PrepareProviderConfig>) -> Self {
        let (package_manager, lockfile) = Self::detect_package_manager(project_root);

        Self {
            project_root: project_root.clone(),
            package_manager,
            lockfile,
            config: config.cloned(),
        }
    }

    /// Detect the package manager from lockfile presence
    fn detect_package_manager(project_root: &PathBuf) -> (Option<PackageManager>, Option<PathBuf>) {
        // Check in order of specificity
        let lockfiles = [
            ("bun.lockb", PackageManager::Bun),
            ("bun.lock", PackageManager::Bun),
            ("pnpm-lock.yaml", PackageManager::Pnpm),
            ("yarn.lock", PackageManager::Yarn),
            ("package-lock.json", PackageManager::Npm),
        ];

        for (lockfile, pm) in lockfiles {
            let path = project_root.join(lockfile);
            if path.exists() {
                return (Some(pm), Some(path));
            }
        }

        // Check if package.json exists (default to npm)
        let package_json = project_root.join("package.json");
        if package_json.exists() {
            return (Some(PackageManager::Npm), Some(package_json));
        }

        (None, None)
    }
}

impl PrepareProvider for NpmPrepareProvider {
    fn id(&self) -> &str {
        "npm"
    }

    fn sources(&self) -> Vec<PathBuf> {
        let mut sources = vec![];

        // Add lockfile as primary source
        if let Some(lockfile) = &self.lockfile {
            sources.push(lockfile.clone());
        }

        // Add package.json as secondary source
        let package_json = self.project_root.join("package.json");
        if package_json.exists() {
            sources.push(package_json);
        }

        // Add extra sources from config
        if let Some(config) = &self.config {
            for extra in &config.extra_sources {
                let path = self.project_root.join(extra);
                if path.exists() {
                    sources.push(path);
                }
            }
        }

        sources
    }

    fn outputs(&self) -> Vec<PathBuf> {
        let mut outputs = vec![self.project_root.join("node_modules")];

        // Add extra outputs from config
        if let Some(config) = &self.config {
            for extra in &config.extra_outputs {
                outputs.push(self.project_root.join(extra));
            }
        }

        outputs
    }

    fn prepare_command(&self) -> Result<PrepareCommand> {
        // Check for custom command override
        if let Some(config) = &self.config
            && let Some(custom_run) = &config.run
        {
            let parts: Vec<&str> = custom_run.split_whitespace().collect();
            let (program, args) = parts.split_first().unwrap_or((&"npm", &[]));

            let mut env = BTreeMap::new();
            for (k, v) in &config.env {
                env.insert(k.clone(), v.clone());
            }

            return Ok(PrepareCommand {
                program: program.to_string(),
                args: args.iter().map(|s| s.to_string()).collect(),
                env,
                cwd: config
                    .dir
                    .as_ref()
                    .map(|d| self.project_root.join(d))
                    .or_else(|| Some(self.project_root.clone())),
                description: format!("Installing {} dependencies", self.id()),
            });
        }

        // Use detected package manager
        let pm = self.package_manager.unwrap_or(PackageManager::Npm);
        let (program, args) = pm.install_command();

        let mut env = BTreeMap::new();
        if let Some(config) = &self.config {
            for (k, v) in &config.env {
                env.insert(k.clone(), v.clone());
            }
        }

        Ok(PrepareCommand {
            program: program.to_string(),
            args: args.iter().map(|s| s.to_string()).collect(),
            env,
            cwd: Some(self.project_root.clone()),
            description: format!("Installing {} dependencies", pm.name()),
        })
    }

    fn is_applicable(&self) -> bool {
        // Check if disabled in config
        if let Some(config) = &self.config
            && !config.enabled
        {
            return false;
        }

        // Applicable if we detected a package manager
        self.package_manager.is_some()
    }

    fn priority(&self) -> u32 {
        100
    }
}
