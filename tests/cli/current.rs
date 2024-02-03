use crate::cli::prelude::*;
use eyre::Result;
use test_case::test_case;

// From e2e/test_bang
#[test_case("tiny@sub-1:latest", "2.1.0")]
#[test_case("tiny@sub-1:lts", "2.1.0")]
#[test_case("tiny@sub-0.1:3.1", "3.0.1")]
fn test_local(local: &str, version: &str) -> Result<()> {
    mise! {
        when!(
            given!(args "install", "tiny");
            should!(succeed)
        ),
        when!(
            given!(args "local", local);
            should!(succeed)
        ),
        when!(
            given!(args "current", "tiny");
            should!(output_exactly format!("{}\n", version)),
            should!(succeed)
        )
    }
}
