use crate::cli::prelude::*;
use eyre::Result;

#[test]
fn test_tool_versions() -> Result<()> {
    mise! {
        given_environment!(has_root_files CONFIGS.get(".alternate-tool-versions")),
        given_environment!(has_exported_var "MISE_DEFAULT_TOOL_VERSIONS_FILENAME", ".alternate-tool-versions"),
        given_environment!(has_exported_var "MISE_DEFAULT_CONFIG_FILENAME", ".MISSING");
        when!(
            given!(args "install", "shfmt", "shfmt@3.6.0");
            should!(succeed),
        ),
        when!(
            given!(args "exec", "--", "shfmt", "--version");
            should!(output_exactly "v3.5.0\n"),
            should!(succeed),
        ),
        when!(
            given!(args "local");
            should!(output_exactly "shfmt 3.5.0\n"),
            should!(succeed),
        ),
        when!(
            given!(args "local", "-p", "shfmt@3.6.0");
            should!(succeed),
        ),
        when!(
            given!(args "local", "-p");
            should!(output_exactly "shfmt 3.6.0\n"),
            should!(succeed),
        ),
        when!(
            given!(args "exec", "--", "shfmt", "--version");
            should!(output_exactly "v3.6.0\n"),
            should!(succeed),
        ),
        when!(
            given!(args "local", "-p", "shfmt@3.5.0");
            should!(succeed),
        ),
        when!(
            given!(args "local", "shfmt");
            should!(output_exactly "3.5.0\n"),
            should!(succeed),
        ),
    }
}
