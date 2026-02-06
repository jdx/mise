use crate::backend::Backend;
use crate::backend::VersionInfo;
use crate::backend::backend_type::BackendType;
use crate::cli::args::BackendArg;
use crate::cmd::CmdLineRunner;
use crate::file;
use crate::http::HTTP_FETCH;
use crate::install_context::InstallContext;
use crate::toolset::ToolVersion;
use crate::{Result, config::Config, env};
use async_trait::async_trait;
use indoc::formatdoc;
use once_cell::sync::OnceCell;
use serde::Deserialize;
use std::path::Path;
use std::{fmt::Debug, sync::Arc};

const GEM_PROGRAM: &str = if cfg!(windows) { "gem.cmd" } else { "gem" };

/// Cached gem source URL, memoized globally after first successful detection
static GEM_SOURCE: OnceCell<String> = OnceCell::new();

#[derive(Debug)]
pub struct GemBackend {
    ba: Arc<BackendArg>,
}

#[async_trait]
impl Backend for GemBackend {
    fn get_type(&self) -> BackendType {
        BackendType::Gem
    }

    fn ba(&self) -> &Arc<BackendArg> {
        &self.ba
    }

    fn get_dependencies(&self) -> eyre::Result<Vec<&str>> {
        Ok(vec!["ruby"])
    }

    async fn _list_remote_versions(&self, config: &Arc<Config>) -> eyre::Result<Vec<VersionInfo>> {
        // Get the gem source URL using the mise-managed Ruby environment
        let source_url = self.get_gem_source(config).await;

        // Use RubyGems-compatible API to get versions with timestamps
        let url = format!("{}api/v1/versions/{}.json", source_url, self.tool_name());
        let response: Vec<RubyGemsVersion> = HTTP_FETCH.json(&url).await?;

        // RubyGems API returns newest-first, mise expects oldest-first
        let mut versions: Vec<VersionInfo> = response
            .into_iter()
            .map(|v| VersionInfo {
                version: v.number,
                created_at: v.created_at,
                ..Default::default()
            })
            .collect();
        versions.reverse();

        Ok(versions)
    }

    async fn install_version_(&self, ctx: &InstallContext, tv: ToolVersion) -> Result<ToolVersion> {
        // Check if gem is available
        self.warn_if_dependency_missing(
            &ctx.config,
            "gem",
            "To use gem packages with mise, you need to install Ruby first:\n\
              mise use ruby@latest",
        )
        .await;

        CmdLineRunner::new(GEM_PROGRAM)
            .arg("install")
            .arg(self.tool_name())
            .arg("--version")
            .arg(&tv.version)
            .arg("--install-dir")
            .arg(tv.install_path().join("libexec"))
            .with_pr(ctx.pr.as_ref())
            .envs(self.dependency_env(&ctx.config).await?)
            .execute()?;

        // We install the gem to {install_path}/libexec and create a wrapper script for each executable
        // in {install_path}/bin that sets GEM_HOME and executes the gem installed
        env_script_all_bin_files(&tv.install_path())?;

        #[cfg(unix)]
        {
            // Rewrite shebangs for better compatibility:
            // - System Ruby: uses `#!/usr/bin/env ruby` for PATH-based resolution
            // - Mise Ruby: uses minor version symlink (e.g., .../ruby/3.1/bin/ruby) so patch
            //   upgrades don't break gems, while still being pinned to a minor version
            rewrite_gem_shebangs(&tv.install_path())?;

            // Create a ruby symlink in libexec/bin for polyglot script fallback
            // RubyGems polyglot scripts have: exec "$bindir/ruby" "-x" "$0" "$@"
            create_ruby_symlink(&tv.install_path())?;
        }

        Ok(tv)
    }
}

impl GemBackend {
    pub fn from_arg(ba: BackendArg) -> Self {
        Self { ba: Arc::new(ba) }
    }

    /// Get the primary gem source URL using the mise-managed Ruby environment.
    /// The result is memoized globally after first successful detection.
    async fn get_gem_source(&self, config: &Arc<Config>) -> &'static str {
        const DEFAULT_SOURCE: &str = "https://rubygems.org/";

        // Return cached source if available
        if let Some(source) = GEM_SOURCE.get() {
            return source.as_str();
        }

        // Get the mise-managed Ruby environment
        let env = self.dependency_env(config).await.unwrap_or_default();

        // Try to initialize the source - only memoize on success
        match GEM_SOURCE.get_or_try_init(|| {
            let output = cmd!(GEM_PROGRAM, "sources")
                .full_env(&env)
                .read()
                .map_err(|e| eyre::eyre!("failed to run `gem sources`: {e}"))?;

            Ok::<_, eyre::Report>(parse_gem_source_output(&output))
        }) {
            Ok(source) => source.as_str(),
            Err(e) => {
                warn!("{e}, falling back to rubygems.org");
                DEFAULT_SOURCE
            }
        }
    }
}

/// RubyGems API response for version info
#[derive(Debug, Deserialize)]
struct RubyGemsVersion {
    number: String,
    created_at: Option<String>,
}

/// Parse gem sources output to extract the primary source URL.
/// Output format:
/// ```
/// *** CURRENT SOURCES ***
///
/// https://rubygems.org/
/// ```
fn parse_gem_source_output(output: &str) -> String {
    for line in output.lines() {
        let line = line.trim();
        if line.starts_with("http://") || line.starts_with("https://") {
            // Ensure URL ends with /
            return if line.ends_with('/') {
                line.to_string()
            } else {
                format!("{}/", line)
            };
        }
    }
    // Default to rubygems.org if no source found
    "https://rubygems.org/".to_string()
}

#[cfg(unix)]
fn env_script_all_bin_files(install_path: &Path) -> eyre::Result<bool> {
    let install_bin_path = install_path.join("bin");
    let install_libexec_path = install_path.join("libexec");

    file::create_dir_all(&install_bin_path)?;

    for path in get_gem_executables(install_path)? {
        let file_name = path
            .file_name()
            .ok_or_else(|| eyre::eyre!("invalid gem executable path: {}", path.display()))?;
        let exec_path = install_bin_path.join(file_name);
        let gem_exec_path = path.to_str().ok_or_else(|| {
            eyre::eyre!(
                "gem executable path contains invalid UTF-8: {}",
                path.display()
            )
        })?;
        let gem_home = install_libexec_path.to_str().ok_or_else(|| {
            eyre::eyre!(
                "libexec path contains invalid UTF-8: {}",
                install_libexec_path.display()
            )
        })?;
        file::write(
            &exec_path,
            formatdoc!(
                r#"
                #!/usr/bin/env bash
                GEM_HOME="{gem_home}" exec {gem_exec_path} "$@"
                "#,
                gem_home = gem_home,
                gem_exec_path = gem_exec_path,
            ),
        )?;
        file::make_executable(&exec_path)?;
    }

    Ok(true)
}

#[cfg(windows)]
fn env_script_all_bin_files(install_path: &Path) -> eyre::Result<bool> {
    let install_bin_path = install_path.join("bin");
    let install_libexec_path = install_path.join("libexec");

    file::create_dir_all(&install_bin_path)?;

    for path in get_gem_executables(install_path)? {
        // On Windows, create .cmd wrapper scripts
        let file_stem = path
            .file_stem()
            .ok_or_else(|| eyre::eyre!("invalid gem executable path: {}", path.display()))?
            .to_string_lossy();
        let exec_path = install_bin_path.join(format!("{}.cmd", file_stem));
        let gem_exec_path = path.to_str().ok_or_else(|| {
            eyre::eyre!(
                "gem executable path contains invalid UTF-8: {}",
                path.display()
            )
        })?;
        let gem_home = install_libexec_path.to_str().ok_or_else(|| {
            eyre::eyre!(
                "libexec path contains invalid UTF-8: {}",
                install_libexec_path.display()
            )
        })?;
        file::write(
            &exec_path,
            formatdoc!(
                r#"@echo off
                set "GEM_HOME={gem_home}"
                "{gem_exec_path}" %*
                "#,
                gem_home = gem_home,
                gem_exec_path = gem_exec_path,
            ),
        )?;
    }

    Ok(true)
}

fn get_gem_executables(install_path: &Path) -> eyre::Result<Vec<std::path::PathBuf>> {
    // TODO: Find a way to get the list of executables from the gemspec of the
    //       installed gem rather than just listing the files in the bin directory.
    let install_libexec_bin_path = install_path.join("libexec/bin");
    let files = file::ls(&install_libexec_bin_path)?
        .into_iter()
        .filter(|p| file::is_executable(p))
        .collect();
    Ok(files)
}

#[cfg(unix)]
/// Creates a `ruby` symlink in libexec/bin/ for RubyGems polyglot script fallback.
///
/// RubyGems polyglot scripts include: `exec "$bindir/ruby" "-x" "$0" "$@"`
/// This fallback runs when the script is executed via /bin/sh instead of ruby.
/// We create a symlink to the mise-managed Ruby (using minor version) so
/// the fallback works correctly.
fn create_ruby_symlink(install_path: &Path) -> eyre::Result<()> {
    let libexec_bin = install_path.join("libexec/bin");
    let ruby_symlink = libexec_bin.join("ruby");

    // Don't overwrite if it already exists
    if ruby_symlink.exists() || ruby_symlink.is_symlink() {
        return Ok(());
    }

    // Find which Ruby we're using by checking an existing gem executable's shebang
    let executables = get_gem_executables(install_path)?;
    let Some(exec_path) = executables.first() else {
        return Ok(());
    };

    let content = file::read_to_string(exec_path)?;
    let lines: Vec<&str> = content.lines().collect();
    let Some((_, shebang_line)) = find_ruby_shebang(&lines) else {
        return Ok(());
    };

    // Extract the ruby path from the shebang
    let ruby_path = shebang_line
        .trim_start_matches("#!")
        .split_whitespace()
        .next()
        .unwrap_or("");

    if ruby_path.is_empty() {
        return Ok(());
    }

    // Only create symlink for mise-managed Ruby
    // For system Ruby, the shebang is #!/usr/bin/env ruby, which we can't symlink to
    if !is_mise_ruby_path(ruby_path) {
        return Ok(());
    }

    // Create symlink to the ruby executable
    file::make_symlink(Path::new(ruby_path), &ruby_symlink)?;
    Ok(())
}

#[cfg(unix)]
/// Rewrites shebangs in gem executables to improve compatibility.
///
/// For system Ruby: Uses `#!/usr/bin/env ruby` for PATH-based resolution.
/// For mise-managed Ruby: Uses minor version symlink (e.g., `.../ruby/3.1/bin/ruby`)
/// so that patch upgrades (3.1.0 → 3.1.1) don't break gems.
///
/// Handles both regular Ruby scripts and RubyGems polyglot scripts which have
/// `#!/bin/sh` on line 1 but the actual Ruby shebang after `=end`.
fn rewrite_gem_shebangs(install_path: &Path) -> eyre::Result<()> {
    let executables = get_gem_executables(install_path)?;

    for exec_path in executables {
        let content = file::read_to_string(&exec_path)?;
        let lines: Vec<&str> = content.lines().collect();

        if lines.is_empty() {
            continue;
        }

        // Find the Ruby shebang line - either line 1 or after =end for polyglot scripts
        let (shebang_line_idx, shebang_line) = if let Some(info) = find_ruby_shebang(&lines) {
            info
        } else {
            continue;
        };

        // Extract the Ruby path and any arguments from the shebang
        let shebang_content = shebang_line.trim_start_matches("#!");
        let mut parts = shebang_content.split_whitespace();
        let ruby_path = parts.next().unwrap_or("");
        let shebang_args: Vec<&str> = parts.collect();

        let new_shebang = if is_mise_ruby_path(ruby_path) {
            // Mise-managed Ruby: use minor version symlink, preserving any arguments
            match to_minor_version_shebang(ruby_path) {
                Some(path) => {
                    if shebang_args.is_empty() {
                        format!("#!{path}")
                    } else {
                        format!("#!{path} {}", shebang_args.join(" "))
                    }
                }
                None => continue, // Keep original if we can't parse
            }
        } else {
            // System Ruby: use env-based shebang
            // Note: env shebangs generally can't preserve arguments portably
            "#!/usr/bin/env ruby".to_string()
        };

        // Rewrite the file with new shebang at the correct line
        let mut new_lines: Vec<&str> = lines.clone();
        let new_shebang_ref: &str = &new_shebang;
        new_lines[shebang_line_idx] = new_shebang_ref;
        let trailing_newline = if content.ends_with('\n') { "\n" } else { "" };
        let new_content = format!("{}{trailing_newline}", new_lines.join("\n"));
        file::write(&exec_path, &new_content)?;
    }

    Ok(())
}

#[cfg(unix)]
/// Finds the Ruby shebang line in a script.
/// Returns (line_index, line_content) or None if not found.
///
/// For regular Ruby scripts, this is line 0 with `#!...ruby...`.
/// For RubyGems polyglot scripts (starting with `#!/bin/sh`), the Ruby shebang
/// is the first `#!...ruby...` line after `=end`.
fn find_ruby_shebang<'a>(lines: &'a [&'a str]) -> Option<(usize, &'a str)> {
    let first_line = lines.first()?;

    // Check if first line is a Ruby shebang
    if first_line.starts_with("#!") && first_line.contains("ruby") {
        return Some((0, first_line));
    }

    // Check for polyglot format: #!/bin/sh followed by =end and then Ruby shebang
    if first_line.starts_with("#!/bin/sh") {
        let mut found_end = false;
        for (idx, line) in lines.iter().enumerate().skip(1) {
            if line.trim() == "=end" {
                found_end = true;
                continue;
            }
            if found_end && line.starts_with("#!") && line.contains("ruby") {
                return Some((idx, line));
            }
        }
    }

    None
}

#[cfg(unix)]
/// Checks if a Ruby path is within mise's installs directory.
fn is_mise_ruby_path(ruby_path: &str) -> bool {
    let ruby_installs = env::MISE_INSTALLS_DIR.join("ruby");
    Path::new(ruby_path).starts_with(&ruby_installs)
}

#[cfg(unix)]
/// Converts a full version Ruby shebang to use the minor version symlink.
/// e.g., `/home/user/.mise/installs/ruby/3.1.0/bin/ruby` → `/home/user/.mise/installs/ruby/3.1/bin/ruby`
fn to_minor_version_shebang(ruby_path: &str) -> Option<String> {
    let ruby_installs = env::MISE_INSTALLS_DIR.join("ruby");
    let ruby_installs_str = ruby_installs.to_string_lossy();

    // Check if path matches pattern: {installs}/ruby/{version}/bin/ruby
    let path = Path::new(ruby_path);
    let rel_path = path.strip_prefix(&ruby_installs).ok()?;
    let mut components = rel_path.components();

    // First component should be the version (e.g., "3.1.0")
    let version_component = components.next()?.as_os_str().to_string_lossy();
    let version_str = version_component.as_ref();

    // Extract minor version (e.g., "3.1.0" → "3.1")
    let minor_version = extract_minor_version(version_str)?;

    // Reconstruct the path with minor version
    let remaining: std::path::PathBuf = components.collect();
    Some(format!(
        "{}/{}/{}",
        ruby_installs_str,
        minor_version,
        remaining.display()
    ))
}

#[cfg(any(unix, test))]
/// Extracts major.minor from a version string.
/// e.g., "3.1.0" → "3.1", "3.2.1-preview1" → "3.2"
fn extract_minor_version(version: &str) -> Option<String> {
    let parts: Vec<&str> = version.split('.').collect();
    if parts.len() >= 2 {
        Some(format!("{}.{}", parts[0], parts[1]))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_minor_version() {
        assert_eq!(extract_minor_version("3.1.0"), Some("3.1".to_string()));
        assert_eq!(extract_minor_version("3.2.1"), Some("3.2".to_string()));
        assert_eq!(
            extract_minor_version("3.1.0-preview1"),
            Some("3.1".to_string())
        );
        assert_eq!(extract_minor_version("2.7.8"), Some("2.7".to_string()));
        assert_eq!(extract_minor_version("3"), None);
        assert_eq!(extract_minor_version("latest"), None);
    }
}
