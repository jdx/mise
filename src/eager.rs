use crate::cli::version::VERSION;
use crate::cli::CLI;
use crate::config::CONFIG;
use crate::plugins::VERSION_REGEX;
use crate::{env, logger};

/// initializes slow parts of mise eagerly in the background
pub fn early_init() {
    rayon::spawn(|| {
        let _ = &*env::MISE_BIN_NAME; // used in handle_shim
    });
    rayon::spawn(|| {
        let _ = &*VERSION_REGEX; // initialize regex library
    });
    rayon::spawn(|| {
        let _ = &*VERSION; // load the current mise version, used by several things
    });
    rayon::spawn(|| {
        let _ = &*CLI; // generate the clap CLI command
    });
    rayon::spawn(|| {
        logger::init();
        let _ = &*CONFIG;
    });
    // rayon::spawn(|| {
    //     let _ = CONFIG.get_tool_request_set(); // initialize tool request set
    // })
}
