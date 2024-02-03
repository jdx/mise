use crate::cli::prelude::*;
use eyre::Result;

// From e2e/test_uninstall
#[test]
fn test_uninstall_tiny() -> Result<()> {
    mise! {
        given_environment!(has_root_files CONFIGS.get(".mise.toml")),
        given_environment!(has_exported_var "CLICOLOR", "0");
        when!(
            given!(args "install");
            should!(succeed),
        ),
        when!(
            given!(args "uninstall", "tiny");
            should!(succeed),
        ),
        when!(
            given!(args "ls", "tiny");
            should!(output "3.1.0 (missing)"),
            should!(succeed),
        ),
        when!(
            given!(args "install", "tiny@1", "tiny@2.0", "tiny@2.1");
            should!(succeed),
        ),
        when!(
            given!(args "ls", "tiny");
            should!(output "1.1.0"),
            should!(output "2.0.1"),
            should!(output "2.1.0"),
            should!(succeed),
        ),
        when!(
            given!(args "rm", "-a", "tiny@2");
            should!(succeed),
        ),
        when!(
            given!(args "ls", "tiny");
            should!(output "1.1.0"),
            should!(not_output "2.0.1"),
            should!(not_output "2.1.0"),
            should!(succeed),
        ),
        when!(
            given!(args "rm", "-a", "tiny");
            should!(succeed),
        ),
        when!(
            given!(args "ls", "tiny");
            should!(not_output "1.1.0"),
            should!(not_output "2.0.1"),
            should!(not_output "2.1.0"),
            should!(succeed),
        ),
    }
}
