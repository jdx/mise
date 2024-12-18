use std::fs::File;
use std::io::Write;

use eyre::Result;
use xx::file;

use crate::config::Config;
use crate::env;
use crate::env::PATH_KEY;
use crate::hash::hash_to_str;
use crate::toolset::ToolsetBuilder;

/// [internal] This is an internal command that writes an envrc file
/// for direnv to consume.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, hide = true)]
pub struct Envrc {}

impl Envrc {
    pub fn run(self, config: &Config) -> Result<()> {
        let ts = ToolsetBuilder::new().build(config)?;

        let envrc_path = env::MISE_TMP_DIR
            .join("direnv")
            .join(hash_to_str(&env::current_dir()?) + ".envrc");

        // TODO: exit early if envrc_path exists and is up to date
        file::mkdirp(envrc_path.parent().unwrap())?;
        let mut file = File::create(&envrc_path)?;

        writeln!(
            file,
            "### Do not edit. This was autogenerated by 'asdf direnv envrc' ###"
        )?;
        for cf in config.config_files.keys() {
            writeln!(file, "watch_file {}", cf.to_string_lossy())?;
        }
        let (env, env_results) = ts.final_env(config)?;
        for (k, v) in env {
            if k == *PATH_KEY {
                writeln!(file, "PATH_add {}", v)?;
            } else {
                writeln!(
                    file,
                    "export {}={}",
                    shell_escape::unix::escape(k.into()),
                    shell_escape::unix::escape(v.into()),
                )?;
            }
        }
        for path in ts.list_final_paths(config, env_results)?.into_iter().rev() {
            writeln!(file, "PATH_add {}", path.to_string_lossy())?;
        }

        miseprintln!("{}", envrc_path.to_string_lossy());
        Ok(())
    }
}
