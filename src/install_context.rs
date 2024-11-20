use crate::toolset::Toolset;
use crate::ui::progress_report::SingleReport;

pub struct InstallContext<'a> {
    pub ts: &'a Toolset,
    pub pr: Box<dyn SingleReport>,
    pub force: bool,
}
