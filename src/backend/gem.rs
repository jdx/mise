use crate::backend::Backend;
use crate::backend::backend_type::BackendType;
use crate::cli::args::BackendArg;
use crate::cmd::CmdLineRunner;
use crate::file;
use crate::install_context::InstallContext;
use crate::toolset::ToolVersion;
use crate::{Result, config::Config, env};
use async_trait::async_trait;
use indoc::formatdoc;
use std::path::Path;
use std::{fmt::Debug, sync::Arc};

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

    async fn _list_remote_versions(&self, config: &Arc<Config>) -> eyre::Result<Vec<String>> {
        // Use `gem info` to list versions, which respects configured gem sources/mirrors
        let env = self.dependency_env(config).await.unwrap_or_default();

        let output = cmd!(
            "gem",
            "info",
            "--remote",
            "--all",
            "--exact",
            self.tool_name(),
        )
        .full_env(&env)
        .read()?;

        let mut versions = parse_gem_versions(&output)?;
        // gem info returns versions newest-first, but mise expects oldest-first
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

        CmdLineRunner::new("gem")
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

        // Rewrite shebangs for better compatibility:
        // - System Ruby: uses `#!/usr/bin/env ruby` for PATH-based resolution
        // - Mise Ruby: uses minor version symlink (e.g., .../ruby/3.1/bin/ruby) so patch
        //   upgrades don't break gems, while still being pinned to a minor version
        rewrite_gem_shebangs(&tv.install_path())?;

        Ok(tv)
    }
}

impl GemBackend {
    pub fn from_arg(ba: BackendArg) -> Self {
        Self { ba: Arc::new(ba) }
    }
}

fn parse_gem_versions(output: &str) -> eyre::Result<Vec<String>> {
    // Parse gem info output format:
    // *** REMOTE GEMS ***
    //
    // gemname (1.2.3, 1.2.2, 1.2.1, ...)
    //     Authors: ...
    //     Homepage: ...

    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("***") {
            continue;
        }

        // Format: "gemname (version1, version2, ...)"
        // Stop at first line that starts with content (the gem line)
        // Subsequent lines will be metadata
        if let Some(paren_start) = line.find('(')
            && let Some(paren_end) = line.rfind(')')
        {
            let versions_str = &line[paren_start + 1..paren_end];
            let versions: Vec<String> = versions_str
                .split(',')
                .map(|v| v.trim().to_string())
                .collect();
            return Ok(versions);
        }
    }

    Err(eyre::eyre!("Gem not found"))
}

fn env_script_all_bin_files(install_path: &Path) -> eyre::Result<bool> {
    let install_bin_path = install_path.join("bin");
    let install_libexec_path = install_path.join("libexec");

    file::create_dir_all(&install_bin_path)?;

    get_gem_executables(install_path)?
        .into_iter()
        .for_each(|path| {
            let exec_path = install_bin_path.join(path.file_name().unwrap());
            file::write(
                &exec_path,
                formatdoc!(
                    r#"
                    #!/usr/bin/env bash
                    GEM_HOME="{gem_home}" exec {gem_exec_path} "$@"
                    "#,
                    gem_home = install_libexec_path.to_str().unwrap(),
                    gem_exec_path = path.to_str().unwrap(),
                ),
            )
            .unwrap();
            file::make_executable(&exec_path).unwrap();
        });

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

/// Rewrites shebangs in gem executables to improve compatibility.
///
/// For system Ruby: Uses `#!/usr/bin/env ruby` for PATH-based resolution.
/// For mise-managed Ruby: Uses minor version symlink (e.g., `.../ruby/3.1/bin/ruby`)
/// so that patch upgrades (3.1.0 → 3.1.1) don't break gems.
fn rewrite_gem_shebangs(install_path: &Path) -> eyre::Result<()> {
    let executables = get_gem_executables(install_path)?;

    for exec_path in executables {
        let content = file::read_to_string(&exec_path)?;
        let Some(first_line) = content.lines().next() else {
            continue;
        };

        // Check if it's a Ruby shebang
        if !first_line.starts_with("#!") || !first_line.contains("ruby") {
            continue;
        }

        // Extract the Ruby path and any arguments from the shebang
        let shebang_content = first_line.trim_start_matches("#!");
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

        // Rewrite the file with new shebang, preserving trailing newline
        let rest_of_file = content.lines().skip(1).collect::<Vec<_>>().join("\n");
        let trailing_newline = if content.ends_with('\n') { "\n" } else { "" };
        let new_content = format!("{new_shebang}\n{rest_of_file}{trailing_newline}");
        file::write(&exec_path, &new_content)?;
    }

    Ok(())
}

/// Checks if a Ruby path is within mise's installs directory.
fn is_mise_ruby_path(ruby_path: &str) -> bool {
    let ruby_installs = env::MISE_INSTALLS_DIR.join("ruby");
    Path::new(ruby_path).starts_with(&ruby_installs)
}

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
    fn test_parse_gem_versions() {
        let output = r#"*** REMOTE GEMS ***

rake (13.3.0, 13.2.1, 13.2.0, 13.1.0, 13.0.6, 13.0.5)
    Authors: Hiroshi SHIBATA
    Homepage: https://github.com/ruby/rake"#;

        let versions = parse_gem_versions(output).unwrap();
        assert_eq!(
            versions,
            vec!["13.3.0", "13.2.1", "13.2.0", "13.1.0", "13.0.6", "13.0.5"]
        );
    }

    #[test]
    fn test_parse_gem_versions_empty() {
        let output = r#"*** REMOTE GEMS ***

"#;

        let result = parse_gem_versions(output);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().to_string(), "Gem not found");
    }

    #[test]
    fn test_parse_gem_versions_single() {
        let output = r#"*** REMOTE GEMS ***

bundler (2.5.4)"#;

        let versions = parse_gem_versions(output).unwrap();
        assert_eq!(versions, vec!["2.5.4"]);
    }

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
