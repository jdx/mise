//! This crate imports active integration suites for testing
//! and can be run with `cargo test --test integration`.

// Run with `cargo test --test integration -- cli`
// Environment Variables:
// INTEGRATION_CACHE_DIR - Path to persistent cache directory
// INTEGRATION_DATA_DIR - Path to persistent data directory
// INTEGRATION_HOME_DIR - Path to persistent home directory
// INTEGRATION_ROOT_DIR - Path to persistent root directory
//
// If a path to an existing directory is passed with the
// above env vars the cli suite will write/read from the
// referenced directory instead of using a temporary one.
// WARNING: using a persistent directory can pollute
//          the global test environment potentially
//          invalidating test results.
// #[cfg(test)]
// mod cli;
