use crate::cli::prelude::*;
use eyre::Result;

#[test]
fn test_task_load_precedence() -> Result<()> {
    mise! {
        given_environment!(has_root_files CONFIGS.get(".mise.toml")),
        given_environment!(has_home_files global_config_fixture());
        when!(
            given!(env_var "MISE_EXPERIMENTAL", "1"),
            given!(args "run", "configtask");
            should!(not_output_exactly "global"),
        ),
    }
}

fn global_config_fixture() -> File {
    File {
        path: ".config/mise/config.toml".into(),
        content: toml::toml! {
            [tasks.configtask]
            run = r#"echo -n "global""#

        }
        .to_string(),
    }
}
