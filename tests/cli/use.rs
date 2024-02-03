use crate::cli::prelude::*;
use eyre::Result;

// From e2e/test_go
#[test]
fn test_use_golang() -> Result<()> {
    mise! {
        given_environment!(has_root_files CONFIGS.get(".default-go-packages")),
        given_environment!(has_exported_var "MISE_EXPERIMENTAL", "1"),
        given_environment!(has_exported_var "MISE_GO_DEFAULT_PACKAGES_FILE", ".default-go-packages");
        when!(
            given!(args "use", "golang@prefix:1.20");
            should!(succeed),
        ),
        when!(
            given!(args "x", "--", "go", "version");
            should!(output "go version go1.20"),
            should!(succeed),
        ),
        when!(
            given!(args "env", "-s", "bash");
            should!(output "GOBIN"),
            should!(succeed),
        ),
        when!(
            given!(args "use", "golang@system");
            should!(succeed),
        ),
        when!(
            given!(args "env", "-s", "bash");
            should!(not_output "GOPATH"),
            should!(succeed),
        ),
    }
}
