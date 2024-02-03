use crate::cli::prelude::*;
use eyre::Result;
use predicates::prelude::*;

// From e2e/test_global
#[test]
fn test_exec_change_directory() -> Result<()> {
    // Given a .tool-versions file exist in $HOME
    let env = EnvironmentBuilder::new()
        .with_home_files([CONFIGS.get(".tool-versions")])
        .build()?;

    let tool_verisons_path = env.home_path().join(".tool-versions");

    // Given default settings
    // When `mise global node 20.0.0` is run
    // Mise should reference the default global config file
    env.mise()
        .unset_env("MISE_GLOBAL_CONFIG_FILE")
        .unset_env("MISE_CONFIG_FILE")
        .args(["global", "node", "20.0.0"])
        .run()?
        .stdout(predicate::str::contains("~/.config/mise/config.toml"))
        .success();

    // Given MISE_ASDF_COMPAT is enabled
    // When `mise global node 20.0.0` is run
    // Mise should reference the .tool-versions file in $HOME
    env.mise()
        .unset_env("MISE_GLOBAL_CONFIG_FILE")
        .unset_env("MISE_CONFIG_FILE")
        .env("MISE_ASDF_COMPAT", "1")
        .args(["global", "node", "20.0.0"])
        .run()?
        .stdout(predicate::str::contains("~/.tool-versions"))
        .success();

    // Given MISE_CONFIG_FILE is set to the .tool-versions file in $HOME
    // When `mise global node 20.0.0` is run
    // Mise should reference the .tool-versions file in $HOME
    env.mise()
        .unset_env("MISE_GLOBAL_CONFIG_FILE")
        .unset_env("MISE_CONFIG_FILE")
        .env("MISE_CONFIG_FILE", &tool_verisons_path)
        .args(["global", "node", "20.0.0"])
        .run()?
        .stdout(predicate::str::contains("~/.tool-versions"))
        .success();

    // Given MISE_GLOBAL_CONFIG_FILE is set to the .tool-versions file in $HOME
    // When `mise global node 20.0.0` is run
    // Mise should reference the .tool-versions file in $HOME
    env.mise()
        .unset_env("MISE_GLOBAL_CONFIG_FILE")
        .unset_env("MISE_CONFIG_FILE")
        .env("MISE_GLOBAL_CONFIG_FILE", tool_verisons_path)
        .args(["global", "node", "20.0.0"])
        .run()?
        .stdout(predicate::str::contains("~/.tool-versions"))
        .success();

    env.teardown()
}
