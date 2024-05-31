mod prelude;

// Tests not included:
// e2e/test_doctor
// e2e/test_env
// e2e/test_go_install
// e2e/test_npm
// e2e/test_path_safety
// e2e/test_poetry
// e2e/test_run
// e2e/test_shell
// e2e/test_shims
// e2e/test_system
// e2e/test_tiny
// e2e/test_top_runtimes
// e2e/test_upgrade
// e2e/test_use
// e2e/test_zigmod

// Tests `mise current`
#[cfg(test)]
mod current;

// Tests `mise env`
#[cfg(test)]
mod env;

// Tests `mise exec`
#[cfg(test)]
mod exec;

// Tests backends
#[cfg(test)]
mod backend;

// Tests `mise global`
#[cfg(test)]
mod global;

#[cfg(test)]
mod legacy;

// Tests `mise local`
#[cfg(test)]
mod local;

// Tests global options for `mise`
#[cfg(test)]
mod global_options;

// Tests `mise plugins`
#[cfg(test)]
mod plugins;

// Tests `mise run`
#[cfg(test)]
mod run;

// Tests `mise install`
#[cfg(test)]
mod install;

// Tests `mise uninstall`
#[cfg(test)]
mod uninstall;

// Tests `mise use`
#[cfg(test)]
mod r#use;
