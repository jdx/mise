use std::fs;

use crate::cmd;

#[ctor::ctor]
fn init() {
    std::env::set_var("NO_COLOR", "1");
    env_logger::init();
    let _ = fs::remove_dir_all("test/data/legacy_cache");
    if let Err(err) = cmd!(
        "git",
        "checkout",
        "test/.tool-versions",
        "test/cwd/.tool-versions"
    )
    .run()
    {
        warn!("failed to reset test files: {}", err);
    }
}
