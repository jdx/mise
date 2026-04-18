use std::sync::Arc;

use jiff::Timestamp;

use crate::ui::progress_report::SingleReport;
use crate::{config::Config, toolset::Toolset};

#[derive(Debug)]
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
