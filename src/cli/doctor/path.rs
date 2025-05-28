use crate::Result;
use crate::config::Config;
use std::env;

/// Print the current PATH entries mise is providing
#[derive(Debug, clap::Args)]
#[clap(alias="paths", verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Path {
    /// Print all entries including those not provided by mise
    #[clap(long, short, verbatim_doc_comment)]
    full: bool,
}

impl Path {
    pub async fn run(self) -> Result<()> {
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
            println!("{}", path.display());
        }
        Ok(())
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    Get the current PATH entries mise is providing
    $ mise path
    /home/user/.local/share/mise/installs/node/24.0.0/bin
    /home/user/.local/share/mise/installs/rust/1.90.0/bin
    /home/user/.local/share/mise/installs/python/3.10.0/bin
"#
);
