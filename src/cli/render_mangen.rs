use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use eyre::Result;

use crate::cli::{version, Cli};

/// internal command to generate markdown from help
#[derive(Debug, clap::Args)]
#[clap(hide = true)]
pub struct RenderMangen {}

impl RenderMangen {
    pub fn run(self) -> Result<()> {
        let cli = Cli::command()
            .version(&*version::RAW_VERSION)
            .disable_colored_help(true);

        let man = clap_mangen::Man::new(cli);
        let mut buffer: Vec<u8> = Default::default();
        man.render(&mut buffer)?;

        let out_dir = project_root().join("man").join("man1");
        fs::create_dir_all(&out_dir)?;
        fs::write(out_dir.join("rtx.1"), buffer)?;

        Ok(())
    }
}

fn project_root() -> PathBuf {
    Path::new(&env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(1)
        .unwrap()
        .to_path_buf()
}

#[cfg(test)]
mod tests {
    use crate::env::HOME;
    use crate::file;

    #[test]
    fn test_render_mangen() {
        let out_dir = HOME.parent().unwrap().join("man").join("man1");
        let orig = file::read_to_string(out_dir.join("rtx.1")).unwrap();
        assert_cli!("render-mangen");
        file::write(out_dir.join("rtx.1"), orig).unwrap();
    }
}
