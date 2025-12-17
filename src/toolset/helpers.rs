use std::collections::HashSet;
use std::sync::Arc;

use eyre::Result;
use itertools::Itertools;

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

pub(super) fn get_leaf_dependencies(requests: &[ToolRequest]) -> Result<Vec<ToolRequest>> {
    // reverse maps potential shorts like "cargo-binstall" for "cargo:cargo-binstall"
    let versions_hash = requests
        .iter()
        .flat_map(|tr| tr.ba().all_fulls())
        .collect::<HashSet<_>>();
    let leaves = requests
        .iter()
        .map(|tr| {
            match tr.backend()?.get_all_dependencies(true)?.iter().all(|dep| {
                // dep is a dependency of tr so if it is in versions_hash (meaning it's also being installed) then it is not a leaf node
                !dep.all_fulls()
                    .iter()
                    .any(|full| versions_hash.contains(full))
            }) {
                true => Ok(Some(tr)),
                false => Ok(None),
            }
        })
        .flatten_ok()
        .map_ok(|tr| tr.clone())
        .collect::<Result<Vec<_>>>()?;
    Ok(leaves)
}
