use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::exit;

use color_eyre::eyre::Result;

use rtx::cli::{version, Cli};

fn main() {
    if let Err(e) = try_main() {
        eprintln!("{}", e);
        exit(-1);
    }
}

fn try_main() -> Result<()> {
    let task = env::args().nth(1);
    match task.as_deref() {
        Some("mangen") => mangen()?,
        _ => print_help(),
    }
    Ok(())
}

fn print_help() {
    eprintln!(
        "Tasks:

mangen            builds man pages
"
    )
}

fn mangen() -> Result<()> {
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

fn project_root() -> PathBuf {
    Path::new(&env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(1)
        .unwrap()
        .to_path_buf()
}
