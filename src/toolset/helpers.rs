use std::sync::Arc;

use crate::backend::Backend;
use crate::toolset::tool_request::ToolRequest;
use crate::toolset::tool_version::ToolVersion;

pub(super) type TVTuple = (Arc<dyn Backend>, ToolVersion);

pub(super) fn show_python_install_hint(versions: &[ToolRequest]) {
    let num_python = versions
        .iter()
        .filter(|tr| tr.ba().tool_name == "python")
        .count();
    if num_python != 1 {
        return;
    }
    hint!(
        "python_multi",
        "use multiple versions simultaneously with",
        "mise use python@3.12 python@3.11"
    );
}
