use std::fmt::Debug;

use url::Url;

use crate::cache::CacheManager;
use crate::cli::args::ForgeArg;
use crate::cmd::CmdLineRunner;
use crate::config::{Config, Settings};
use crate::file;
use crate::forge::{Forge, ForgeType};
use crate::http::HTTP_FETCH;
use crate::install_context::InstallContext;

#[derive(Debug)]
pub struct GemForge {
    fa: ForgeArg,
    remote_version_cache: CacheManager<Vec<String>>,
}

impl Forge for GemForge {
    fn get_type(&self) -> ForgeType {
        ForgeType::Gem
    }

    fn fa(&self) -> &ForgeArg {
        &self.fa
    }

    fn list_remote_versions(&self) -> eyre::Result<Vec<String>> {
        // The `gem list` command does not supporting listing versions as json output
        // so we use the rubygems.org api to get the list of versions.
        self.remote_version_cache
            .get_or_try_init(|| {
                let raw = HTTP_FETCH.get_text(get_gem_url(self.name())?)?;
                let gem_versions: Vec<GemVersion> = serde_json::from_str(&raw)?;
                let mut versions: Vec<String> = vec![];
                for version in gem_versions.iter().rev() {
                    versions.push(version.number.clone());
                }
                Ok(versions)
            })
            .cloned()
    }

    fn install_version_impl(&self, ctx: &InstallContext) -> eyre::Result<()> {
        let config = Config::try_get()?;
        let settings = Settings::get();
        settings.ensure_experimental("gem backend")?;

        CmdLineRunner::new("gem")
            .arg("install")
            .arg(self.name())
            .arg("--version")
            .arg(&ctx.tv.version)
            .arg("--install-dir")
            .arg(ctx.tv.install_path().join("libexec"))
            // NOTE: Use `#!/usr/bin/env ruby` may cause some gems to not work properly
            //       using a different ruby then they were installed with. Therefore we
            //       we avoid the use of `--env-shebang` for now. However, this means that
            //       uninstalling the ruby version used to install the gem will break the
            //       gem. We should find a way to fix this.
            // .arg("--env-shebang")
            .with_pr(ctx.pr.as_ref())
            .envs(config.env()?)
            .execute()?;

        // We install the gem to {install_path}/libexec and create a wrapper script for each executable
        // in {install_path}/bin that sets GEM_HOME and executes the gem installed
        env_script_all_bin_files(&ctx.tv.install_path())?;

        Ok(())
    }
}

impl GemForge {
    pub fn new(fa: ForgeArg) -> Self {
        Self {
            remote_version_cache: CacheManager::new(
                fa.cache_path.join("remote_versions.msgpack.z"),
            ),
            fa,
        }
    }
}

fn get_gem_url(n: &str) -> eyre::Result<Url> {
    Ok(format!("https://rubygems.org/api/v1/versions/{n}.json").parse()?)
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

#[derive(Debug, serde::Deserialize)]
struct GemVersion {
    number: String,
}
