use crate::cli::prelude::*;
use eyre::Result;

// From e2e/test_local
#[test]
fn test_local() -> Result<()> {
    mise! {
        given_environment!(has_root_files CONFIGS.get(".mise.toml"));
        when!(
            given!(args "install", "shfmt@3.5.0", "shfmt@3.6.0");
            should!(succeed),
        ),
        when!(
            given!(args "local");
            should!(not_output "shfmt"),
            should!(succeed),
        ),
        when!(
            given!(args "local", "shfmt@3.5.0");
            should!(succeed),
        ),
        when!(
            given!(args "local");
            should!(output r#"shfmt = "3.5.0""#),
            should!(not_output r#"shfmt = "3.6.0""#),
            should!(succeed),
        ),
        when!(
            given!(args "local", "shfmt@3.6.0");
            should!(succeed),
        ),
        when!(
            given!(args "local");
            should!(output r#"shfmt = "3.6.0""#),
            should!(not_output r#"shfmt = "3.5.0""#),
            should!(succeed),
        ),
        when!(
            given!(args "exec", "--", "shfmt", "--version");
            should!(output "v3.6.0"),
            should!(succeed),
        ),
        when!(
            given!(args "local", "--rm", "shfmt");
            should!(succeed),
        ),
        when!(
            given!(args "local");
            should!(not_output "shfmt"),
            should!(succeed),
        ),
    }
}

// From e2e/test_local
#[test]
fn test_local_no_config() -> Result<()> {
    mise! {
        given_environment!(has_root_files CONFIGS.get(".tool-versions"));
        when!(
            given!(args "install", "shfmt@3.5.0", "shfmt@3.6.0");
            should!(succeed),
        ),
        when!(
            given!(args "local");
            should!(output "shfmt      3.6.0 # test comment"),
            should!(succeed),
        ),
        when!(
            given!(args "local", "shfmt@3.5.0");
            should!(succeed),
        ),
        when!(
            given!(args "local");
            should!(output "shfmt      3.5.0 # test comment"),
            should!(succeed),
        ),
        when!(
            given!(args "exec", "--", "shfmt", "--version");
            should!(output "v3.5.0"),
            should!(succeed),
        ),
        when!(
            given!(args "local", "shfmt@3.6.0");
            should!(succeed),
        ),
        when!(
            given!(args "local");
            should!(output "shfmt      3.6.0 # test comment"),
            should!(succeed),
        ),
        when!(
            given!(args "exec", "--", "shfmt", "--version");
            should!(output "v3.6.0"),
            should!(succeed),
        ),
    }
}
