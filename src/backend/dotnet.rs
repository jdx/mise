use crate::backend::backend_type::BackendType;
use crate::backend::Backend;
use crate::cli::args::BackendArg;

#[derive(Debug)]
pub struct DotnetBackend {
    ba: BackendArg,
}

impl Backend for DotnetBackend {
    fn get_type(&self) -> BackendType {
        BackendType::Dotnet
    }

    fn ba(&self) -> &BackendArg {
        todo!()
    }

    fn _list_remote_versions(&self) -> eyre::Result<Vec<String>> {
        todo!()
    }

    fn install_version_(
        &self,
        ctx: &crate::install_context::InstallContext,
        tv: crate::toolset::ToolVersion,
    ) -> eyre::Result<crate::toolset::ToolVersion> {
        todo!()
    }
}

impl DotnetBackend {

    pub fn from_arg(ba: BackendArg) -> Self {
        Self { ba }
    }
}
