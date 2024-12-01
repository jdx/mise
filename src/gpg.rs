use crate::cmd::CmdLineRunner;
use crate::install_context::InstallContext;
use crate::Result;

pub fn add_keys_node(ctx: &InstallContext) -> Result<()> {
    add_keys(ctx, include_str!("assets/gpg/node.asc"))
}

pub fn add_keys_swift(ctx: &InstallContext) -> Result<()> {
    add_keys(ctx, include_str!("assets/gpg/swift.asc"))
}

fn add_keys(ctx: &InstallContext, keys: &str) -> Result<()> {
    CmdLineRunner::new("gpg")
        .arg("--quiet")
        .arg("--import")
        .stdin_string(keys)
        .with_pr(ctx.pr.as_ref())
        .execute()
}
