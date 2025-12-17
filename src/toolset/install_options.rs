use crate::config::settings::Settings;
use crate::toolset::tool_version::ResolveOptions;

#[derive(Debug, Clone)]
pub struct InstallOptions {
    pub reason: String,
    pub force: bool,
    pub jobs: Option<usize>,
    pub raw: bool,
    /// only install missing tools if passed as arguments
    pub missing_args_only: bool,
    /// completely disable auto-installation when auto_install setting is false
    pub skip_auto_install: bool,
    pub auto_install_disable_tools: Option<Vec<String>>,
    pub resolve_options: ResolveOptions,
    pub dry_run: bool,
    /// require lockfile URLs to be present; fail if not
    pub locked: bool,
}

impl Default for InstallOptions {
    fn default() -> Self {
        InstallOptions {
            jobs: Some(Settings::get().jobs),
            raw: Settings::get().raw,
            reason: "install".to_string(),
            force: false,
            missing_args_only: true,
            skip_auto_install: false,
            auto_install_disable_tools: Settings::get().auto_install_disable_tools.clone(),
            resolve_options: Default::default(),
            dry_run: false,
            locked: Settings::get().locked,
        }
    }
}
