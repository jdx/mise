use std::sync::Arc;

use crate::ui::progress_report::SingleReport;
use crate::{config::Config, toolset::Toolset};

pub struct InstallContext {
    pub config: Arc<Config>,
    pub ts: Arc<Toolset>,
    pub pr: Box<dyn SingleReport>,
    pub force: bool,
}
