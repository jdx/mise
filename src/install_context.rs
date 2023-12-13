use crate::config::Config;
use crate::toolset::{ToolVersion, Toolset};
use crate::ui::progress_report::ProgressReport;

pub struct InstallContext<'a> {
    pub config: &'a Config,
    pub ts: &'a Toolset,
    pub tv: ToolVersion,
    pub pr: ProgressReport,
    pub raw: bool,
    pub force: bool,
}
