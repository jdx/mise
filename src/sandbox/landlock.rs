use eyre::{Result, eyre};
use landlock::{
    ABI, AccessFs, BitFlags, Compatible, PathBeneath, PathFd, Ruleset, RulesetAttr,
    RulesetCreatedAttr,
};

use super::SandboxConfig;

/// System paths that are always readable on Linux.
/// Note: /tmp and /dev are handled separately with full (read+write) access.
const SYSTEM_READ_PATHS: &[&str] = &[
    "/usr",
    "/lib",
    "/lib64",
    "/bin",
    "/sbin",
    "/etc",
    "/proc",
    "/sys",
    "/nix",
    "/snap",
    "/home/linuxbrew",
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
        Err(_) => {
            // Path doesn't exist — on Linux, Landlock requires existing paths.
            // This affects cases like --allow-write=./dist where the dir doesn't exist yet.
            // We warn rather than silently skipping or granting broader ancestor access.
            eprintln!(
                "mise sandbox: path '{}' does not exist, sandbox rule may not apply as expected",
                path.display()
            );
            Ok(ruleset)
        }
    }
}

/// Apply Landlock filesystem restrictions.
pub fn apply_landlock(config: &SandboxConfig) -> Result<()> {
    let abi = ABI::V5;

    let read_access = AccessFs::from_read(abi);
    let write_access = AccessFs::from_write(abi);
    let full_access = read_access | write_access;

    let deny_read = config.effective_deny_read();
    let deny_write = config.effective_deny_write();

    // Only handle the access types we're actually restricting.
    // If we handle_access(full_access) but only add read rules,
    // writes to un-ruled paths get blocked too (Landlock denies by default).
    let handled_access = match (deny_read, deny_write) {
        (true, true) => full_access,
        (true, false) => read_access,
        (false, true) => full_access, // need full to add read+write rules for allowed paths
        (false, false) => return Ok(()), // nothing to restrict
    };

    let mut ruleset = Ruleset::default()
        .handle_access(handled_access)
        .map_err(|e| eyre!("failed to create landlock ruleset: {e}"))?
        .set_compatibility(landlock::CompatLevel::BestEffort)
        .create()
        .map_err(|e| eyre!("failed to create landlock ruleset: {e}"))?;

    if deny_read && deny_write {
        // Both restricted: add read rules for system paths, full for /tmp and /dev
        for path in SYSTEM_READ_PATHS {
            ruleset = add_read_rule(ruleset, path, read_access)?;
        }
        ruleset = add_read_rule(ruleset, "/tmp", full_access)?;
        ruleset = add_read_rule(ruleset, "/dev", full_access)?;
        let installs_dir: &std::path::Path = &crate::dirs::INSTALLS;
        if installs_dir.exists() {
            ruleset = add_path_rule(ruleset, installs_dir, read_access)?;
        }
        ruleset = add_path_rule(ruleset, &crate::env::MISE_DATA_DIR, read_access)?;
        for path in &config.allow_read {
            ruleset = add_path_rule(ruleset, path, read_access)?;
        }
        for path in &config.allow_write {
            ruleset = add_path_rule(ruleset, path, full_access)?;
        }
    } else if deny_read {
        // Only reads restricted — only handle read access so writes are unaffected
        for path in SYSTEM_READ_PATHS {
            ruleset = add_read_rule(ruleset, path, read_access)?;
        }
        // /tmp and /dev need read access (not in SYSTEM_READ_PATHS, handled separately)
        ruleset = add_read_rule(ruleset, "/tmp", read_access)?;
        ruleset = add_read_rule(ruleset, "/dev", read_access)?;
        let installs_dir: &std::path::Path = &crate::dirs::INSTALLS;
        if installs_dir.exists() {
            ruleset = add_path_rule(ruleset, installs_dir, read_access)?;
        }
        ruleset = add_path_rule(ruleset, &crate::env::MISE_DATA_DIR, read_access)?;
        for path in &config.allow_read {
            ruleset = add_path_rule(ruleset, path, read_access)?;
        }
        // allow_write paths are implicitly readable
        for path in &config.allow_write {
            ruleset = add_path_rule(ruleset, path, read_access)?;
        }
    } else if deny_write {
        // Only writes restricted — allow read everywhere, deny write except allowed paths
        ruleset = add_read_rule(ruleset, "/", read_access)?;
        ruleset = add_read_rule(ruleset, "/tmp", full_access)?;
        ruleset = add_read_rule(ruleset, "/dev", full_access)?;
        for path in &config.allow_write {
            ruleset = add_path_rule(ruleset, path, full_access)?;
        }
    }

    ruleset
        .restrict_self()
        .map_err(|e| eyre!("failed to apply landlock restrictions: {e}"))?;

    Ok(())
}
