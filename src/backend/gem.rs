use crate::backend::Backend;
use crate::backend::backend_type::BackendType;
use crate::cli::args::BackendArg;
use crate::cmd::CmdLineRunner;
use crate::file;
use crate::install_context::InstallContext;
use crate::toolset::ToolVersion;
use crate::{Result, config::Config};
use async_trait::async_trait;
use indoc::formatdoc;
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

        parse_gem_versions(&output)
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
            // NOTE: Use `#!/usr/bin/env ruby` may cause some gems to not work properly
            //       using a different ruby then they were installed with. Therefore we
            //       we avoid the use of `--env-shebang` for now. However, this means that
            //       uninstalling the ruby version used to install the gem will break the
            //       gem. We should find a way to fix this.
            // .arg("--env-shebang")
            .with_pr(ctx.pr.as_ref())
            .envs(self.dependency_env(&ctx.config).await?)
            .execute()?;

        // We install the gem to {install_path}/libexec and create a wrapper script for each executable
        // in {install_path}/bin that sets GEM_HOME and executes the gem installed
        env_script_all_bin_files(&tv.install_path())?;

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
            && let Some(paren_end) = line.rfind(')') {
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

fn env_script_all_bin_files(install_path: &std::path::Path) -> eyre::Result<bool> {
    let install_bin_path = install_path.join("bin");
    let install_libexec_path = install_path.join("libexec");

    match std::fs::create_dir_all(&install_bin_path) {
        Ok(_) => {}
        Err(e) => {
            return Err(eyre::eyre!("couldn't create directory: {}", e));
        }
    }

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

fn get_gem_executables(install_path: &std::path::Path) -> eyre::Result<Vec<std::path::PathBuf>> {
    // TODO: Find a way to get the list of executables from the gemspec of the
    //       installed gem rather than just listing the files in the bin directory.
    let install_libexec_bin_path = install_path.join("libexec/bin");
    let mut files = vec![];

    for entry in std::fs::read_dir(install_libexec_bin_path)? {
        let entry = entry?;
        let path = entry.path();
        if file::is_executable(&path) {
            files.push(path);
        }
    }

    Ok(files)
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
}
