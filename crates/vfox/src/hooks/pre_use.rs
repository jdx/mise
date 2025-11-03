use indexmap::IndexMap;
use std::path::Path;

use crate::error::Result;
use crate::sdk_info::SdkInfo;
use crate::{Plugin, Vfox};

#[allow(dead_code)]
#[derive(Debug)]
pub struct PreUseContext {
    pub installed_sdks: IndexMap<String, SdkInfo>,
}

#[derive(Debug)]
pub struct PreUseResponse {
    pub version: Option<String>,
}

impl Plugin {
    pub async fn pre_use(&self, _vfox: &Vfox, _legacy_file: &Path) -> Result<PreUseResponse> {
        debug!("[vfox:{}] pre_use", &self.name);
        // let ctx = PreUseContext {
        //     installed_sdks: vfox.list_installed_versions(&self.name)?,
        // };

        unimplemented!("pre_use hook is not implemented");
    }
}
