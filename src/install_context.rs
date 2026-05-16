use std::sync::Arc;

use indexmap::IndexMap;
use jiff::Timestamp;

use crate::toolset::ToolVersion;
use crate::ui::progress_report::SingleReport;
use crate::{config::Config, toolset::Toolset};

pub struct InstallContext {
    pub config: Arc<Config>,
    pub ts: Arc<Toolset>,
    pub pr: Box<dyn SingleReport>,
    pub force: bool,
    pub dry_run: bool,
    /// require lockfile URLs to be present; fail if not
    pub locked: bool,
    pub before_date: Option<Timestamp>,
}

impl InstallContext {
    pub fn install_env(&self, tv: &ToolVersion) -> IndexMap<String, String> {
        tv.request.options().core.install_env
    }
}
