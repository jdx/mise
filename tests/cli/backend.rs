use crate::cli::prelude::*;
use eyre::Result;

const EXPECTED_EZA_OUTPUT: &str = "eza - A modern, maintained replacement for ls
v0.18.24 [+git]
https://github.com/eza-community/eza
";

// From e2e/test_cargo
// requires `cargo-binstall`
#[test]
fn test_cargo_binstall() -> Result<()> {
    mise! {
        when!(
            given!(args "rm", "cargo:eza@0.18.24");
            should!(succeed)
        ),
        when!(
            given!(env_var "MISE_EXPERIMENTAL", "1"),
            given!(env_var "MISE_CARGO_BINSTALL", "1"),
            given!(args "x", "cargo:eza@0.18.24", "--", "eza", "-v");
            should!(output_exactly EXPECTED_EZA_OUTPUT),
            should!(succeed)
        )
    }
}

// From e2e/test_cargo
#[test]
#[ignore]
fn test_cargo_local_build() -> Result<()> {
    mise! {
        when!(
            given!(args "rm", "cargo:eza@0.18.24");
            should!(succeed)
        ),
        when!(
            given!(env_var "MISE_EXPERIMENTAL", "1"),
            given!(args "x", "cargo:eza@0.18.24", "--", "eza", "-v");
            should!(output_exactly EXPECTED_EZA_OUTPUT),
            should!(succeed)
        )
    }
}
