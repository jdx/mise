use std::path::{Path, PathBuf};
use std::{env, fs};

use eyre::Result;
use itertools::Itertools;
use xx::file;

use crate::cli::{version, CLI};

/// internal command to generate markdown from help
#[derive(Debug, clap::Args)]
#[clap(hide = true)]
pub struct RenderMangen {}

impl RenderMangen {
    pub fn run(self) -> Result<()> {
        let cli = CLI
            .clone()
            .version(version::V.to_string())
            .disable_colored_help(true);

        let man = clap_mangen::Man::new(cli);
        let mut buffer: Vec<u8> = Default::default();
        man.render(&mut buffer)?;

        let out_dir = project_root().join("man").join("man1");
        file::mkdirp(&out_dir)?;
        let s = String::from_utf8(buffer)?
            .lines()
            .map(|l| l.trim_end())
            .join("\n")
            + "\n";
        fs::write(out_dir.join("mise.1"), s)?;

        Ok(())
    }
}

fn project_root() -> PathBuf {
    Path::new(&env!("CARGO_MANIFEST_DIR")).to_path_buf()
}
