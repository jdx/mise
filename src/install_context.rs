use std::sync::Arc;

use crate::toolset::Toolset;
use crate::ui::progress_report::SingleReport;

pub struct InstallContext {
    pub ts: Arc<Toolset>,
    pub pr: Box<dyn SingleReport>,
    pub force: bool,
}
