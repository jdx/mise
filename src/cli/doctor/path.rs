use crate::Result;
use crate::config::Config;
use std::env;

/// Print the directories mise adds to PATH
///
/// Lists tool bin directories and `_.path` entries from [env]. --full prints the
/// whole PATH mise would set. `mise bin-paths` lists tool bin directories only.
#[derive(Debug, usage_rs::Args)]
#[usage(
    alias = "paths",
    verbatim_doc_comment,
    example(
        r###"mise doctor path
/home/user/.local/share/mise/installs/node/24.11.0/bin
/home/user/.local/share/mise/installs/python/3.13.1/bin"###,
        help = "Print the directories mise adds to PATH"
    ),
    example(
        "mise doctor path --full",
        help = "Print the whole PATH mise would set"
    )
)]
pub(crate) struct Path {
    /// Print every PATH entry, including those mise does not provide
    #[usage(long, short, verbatim_doc_comment)]
    full: bool,
}

impl Path {
    pub(crate) async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        let ts = config.get_toolset().await?;
        let paths = if self.full {
            let env = ts.env_with_path(&config).await?;
            let path = env.get("PATH").cloned().unwrap_or_default();
            env::split_paths(&path).collect()
        } else {
            let (_env, env_results) = ts.final_env(&config).await?;
            ts.list_final_paths(&config, env_results).await?
        };
        for path in paths {
            miseprintln!("{}", path.display());
        }
        Ok(())
    }
}
