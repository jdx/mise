use crate::cli::prelude::*;
use eyre::Result;

// From e2e/test_log_level
#[test]
fn test_log_level() -> Result<()> {
    mise! {
        when!(
            given!(args "exec", "node@20.0.0", "--log-level", "debug", "--", "node", "-v");
            should!(output_exactly "v20.0.0\n")
        ),
        when!(
            given!(args "exec", "node@20.0.0", "--log-level=debug", "--", "node", "-v");
            should!(output_exactly "v20.0.0\n")
        ),
        when!(
            given!(args "exec", "node@20.0.0", "--debug", "--", "node", "-v");
            should!(output_exactly "v20.0.0\n")
        ),
        when!(
            given!(args "exec", "node@20.0.0", "--trace", "--", "node", "-v");
            should!(output_exactly "v20.0.0\n")
        ),
    }
}
