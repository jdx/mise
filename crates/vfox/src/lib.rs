#[cfg(test)]
#[macro_use]
extern crate insta;
#[macro_use]
extern crate log;
#[macro_use]
extern crate mlua;

pub use error::Result as VfoxResult;
pub use error::VfoxError;
pub use plugin::Plugin;
pub use vfox::Vfox;

// Backend hooks
pub mod backend_hooks {
    pub use crate::hooks::backend_exec_env::{BackendExecEnvContext, BackendExecEnvResponse};
    pub use crate::hooks::backend_install::{BackendInstallContext, BackendInstallResponse};
    pub use crate::hooks::backend_list_versions::{BackendListVersionsContext, BackendListVersionsResponse};
    pub use crate::hooks::backend_uninstall::{BackendUninstallContext, BackendUninstallResponse};
}

mod config;
mod context;
mod error;
mod hooks;
mod http;
mod lua_mod;
mod metadata;
mod plugin;
mod registry;
mod runtime;
mod sdk_info;
mod vfox;
