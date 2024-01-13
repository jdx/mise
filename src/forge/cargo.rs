use crate::forge::Forge;
use crate::install_context::InstallContext;
use std::fmt::Debug;

#[derive(Debug)]
pub struct CargoForge {
    pub name: String,
}

impl Forge for CargoForge {
    fn name(&self) -> &str {
        &self.name
    }

    fn list_remote_versions(&self) -> eyre::Result<Vec<String>> {
        todo!()
    }

    fn install_version_impl(&self, _ctx: &InstallContext) -> eyre::Result<()> {
        todo!()
    }
}

impl CargoForge {
    pub fn new(name: String) -> Self {
        Self { name }
    }
}
