use eyre::{Result, eyre};
use landlock::{
    ABI, AccessFs, BitFlags, Compatible, PathBeneath, PathFd, Ruleset, RulesetAttr,
    RulesetCreatedAttr,
};

use super::SandboxConfig;

/// System paths that are always readable on Linux.
const SYSTEM_READ_PATHS: &[&str] = &[
    "/usr", "/lib", "/lib64", "/bin", "/sbin", "/etc", "/dev", "/proc", "/sys", "/tmp",
];

fn add_read_rule(
    ruleset: landlock::RulesetCreated,
    path: &str,
    access: BitFlags<AccessFs>,
) -> Result<landlock::RulesetCreated> {
    match PathFd::new(path) {
        Ok(fd) => ruleset
            .add_rule(PathBeneath::new(fd, access))
            .map_err(|e| eyre!("landlock add_rule failed for {path}: {e}")),
        Err(_) => Ok(ruleset), // Path doesn't exist, skip
    }
}

fn add_path_rule(
    ruleset: landlock::RulesetCreated,
    path: &std::path::Path,
    access: BitFlags<AccessFs>,
) -> Result<landlock::RulesetCreated> {
    match PathFd::new(path) {
        Ok(fd) => ruleset
            .add_rule(PathBeneath::new(fd, access))
            .map_err(|e| eyre!("landlock add_rule failed for {}: {e}", path.display())),
        Err(_) => Ok(ruleset), // Path doesn't exist, skip
    }
}

/// Apply Landlock filesystem restrictions.
pub fn apply_landlock(config: &SandboxConfig) -> Result<()> {
    let abi = ABI::V5;

    let read_access = AccessFs::from_read(abi);
    let write_access = AccessFs::from_write(abi);
    let full_access = read_access | write_access;

    let mut ruleset = Ruleset::default()
        .handle_access(full_access)
        .map_err(|e| eyre!("failed to create landlock ruleset: {e}"))?
        .set_compatibility(landlock::CompatLevel::BestEffort)
        .create()
        .map_err(|e| eyre!("failed to create landlock ruleset: {e}"))?;

    let deny_read = config.effective_deny_read();
    let deny_write = config.effective_deny_write();

    if deny_read {
        // System paths always readable
        for path in SYSTEM_READ_PATHS {
            ruleset = add_read_rule(ruleset, path, read_access)?;
        }
        // Allow reading from all PATH directories and their real paths (after symlink resolution)
        // so programs can be exec'd. Also add parent prefixes for paths like /opt/homebrew/bin
        // which may symlink to /opt/homebrew/Cellar/... and need library paths under /opt/homebrew.
        if let Ok(path_var) = std::env::var("PATH") {
            let mut seen = std::collections::HashSet::new();
            for dir in std::env::split_paths(&path_var) {
                if dir.exists() && seen.insert(dir.clone()) {
                    ruleset = add_path_rule(ruleset, &dir, read_access)?;
                    // Also add the canonicalized path (follows symlinks)
                    if let Ok(real) = dir.canonicalize()
                        && seen.insert(real.clone())
                    {
                        ruleset = add_path_rule(ruleset, &real, read_access)?;
                    }
                    // Add parent prefix (e.g., /opt/homebrew for /opt/homebrew/bin)
                    // to handle symlinks and shared libraries
                    if let Some(parent) = dir.parent()
                        && parent != std::path::Path::new("/")
                        && seen.insert(parent.to_path_buf())
                    {
                        ruleset = add_path_rule(ruleset, parent, read_access)?;
                    }
                }
            }
        }
        // Mise install dirs
        let installs_dir: &std::path::Path = &crate::dirs::INSTALLS;
        if installs_dir.exists() {
            ruleset = add_path_rule(ruleset, installs_dir, read_access)?;
        }
        // Mise data dir
        ruleset = add_path_rule(ruleset, &crate::env::MISE_DATA_DIR, read_access)?;
        // User-specified allow_read paths
        for path in &config.allow_read {
            ruleset = add_path_rule(ruleset, path, read_access)?;
        }
        // allow_write paths are implicitly readable
        for path in &config.allow_write {
            ruleset = add_path_rule(ruleset, path, full_access)?;
        }
        // /tmp is always writable
        ruleset = add_read_rule(ruleset, "/tmp", full_access)?;
    } else if deny_write {
        // Allow read everywhere, deny write except allowed paths
        ruleset = add_read_rule(ruleset, "/", read_access)?;
        // /tmp and /dev are always writable (for /dev/null, /dev/tty, etc.)
        ruleset = add_read_rule(ruleset, "/tmp", full_access)?;
        ruleset = add_read_rule(ruleset, "/dev", full_access)?;
        // User-specified allow_write paths
        for path in &config.allow_write {
            ruleset = add_path_rule(ruleset, path, full_access)?;
        }
    }

    ruleset
        .restrict_self()
        .map_err(|e| eyre!("failed to apply landlock restrictions: {e}"))?;

    Ok(())
}
