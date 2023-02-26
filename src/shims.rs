use crate::cli::command::Command;
use crate::cli::exec::Exec;
use color_eyre::eyre::Result;
use std::ffi::OsString;
use std::process::exit;

use crate::config::Config;
use crate::output::Output;

// executes as if it was a shim if the command is not "rtx", e.g.: "node"
pub fn handle_shim(config: Config, args: &[String], out: &mut Output) -> Result<Config> {
    let (_, bin_name) = args[0].rsplit_once('/').unwrap_or(("", &args[0]));
    if bin_name == "rtx" || !config.settings.experimental {
        return Ok(config);
    }
    let mut args: Vec<OsString> = args.iter().map(OsString::from).collect();
    args[0] = OsString::from(bin_name);
    let exec = Exec {
        runtime: vec![],
        c: None,
        command: Some(args),
    };
    exec.run(config, out)?;
    exit(0);
}
