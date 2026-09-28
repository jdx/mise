use crate::config::{Config, Settings};
use crate::toolset::{ResolveOptions, ToolsetBuilder};
use crate::{env, lookup};
use eyre::{Result, bail, eyre};
use std::process::Command;

pub(super) async fn list_shim_names(args: &[String]) -> Result<()> {
    if args.get(2).map(String::as_str) != Some(lookup::SHIM_PROTOCOL) {
        bail!("unsupported lookup shim protocol");
    }
    let config = Config::get().await?;
    let toolset = ToolsetBuilder::new()
        .with_resolve_options(ResolveOptions {
            offline: true,
            ..Default::default()
        })
        .build(&config)
        .await;
    let names = lookup::command_names(&config, toolset.as_ref().ok()).await?;
    miseprintln!("{}", serde_json::to_string(&names)?);
    Ok(())
}

pub(super) fn dispatch(args: &[String]) -> Result<()> {
    let name = args
        .get(2)
        .ok_or_else(|| eyre!("mise: missing lookup command name"))?;
    if Settings::get().activate_mise_lookup != "env_path" {
        bail!(
            "mise: checkout lookup requires activate_mise_lookup=env_path in the current directory"
        );
    }
    let selection = lookup::bootstrap()?.prepare()?;
    let mut command = Command::new(selection.executable);
    if name != "mise" {
        command.arg("x").arg("--").arg(name);
    } else {
        command.env_remove(env::MISE_SHIM_PATH_ENV);
    }
    let status = command.args(args.iter().skip(3)).status()?;
    Err(crate::request_exit(status.code().unwrap_or(1)))
}
