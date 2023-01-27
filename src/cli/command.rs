use crate::config::Config;
use color_eyre::eyre::Result;

use crate::output::Output;

/// described a CLI command
///
/// e.g.: `rtx plugins ls`
pub trait Command: Sized {
    /// CLI command entry point
    fn run(self, config: Config, output: &mut Output) -> Result<()>;
}
