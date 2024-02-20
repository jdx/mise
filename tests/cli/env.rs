use crate::cli::prelude::*;
use eyre::Result;

// From e2e/test_bin_paths
#[test]
fn test_bin_paths() -> Result<()> {
    mise! {
        when!(
            given!(args "use", "go@system");
            should!(succeed),
        ),
        when!(
            given!(args "env", "-s", "bash");
            should!(not_output "go/system"),
            should!(succeed),
        ),
    }
}

// From e2e/test_env_source
#[test]
fn test_env_source() -> Result<()> {
    mise! {
        given_environment!(
            has_home_files
            CONFIGS.get(".config/mise/config.toml"),
            CONFIGS.get(".config/mise/source.sh"),
        );
        when!(
            given!(env_var "MISE_EXPERIMENTAL", "1"),
            given!(args "env", "-s", "bash");
            should!(output "MISE_TEST_SOURCE=1234"),
            should!(succeed),
        ),
    }
}
