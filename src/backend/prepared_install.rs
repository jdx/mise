use crate::backend::Backend;
use crate::install_context::InstallContext;
use crate::toolset::ToolVersion;
use async_trait::async_trait;
use eyre::Result;
use std::fmt::Debug;

/// An install that has completed every backend-specific preparation step.
///
/// Backends remain on `Legacy` until they can stage all fallible work that must
/// happen before replacing an existing installation.
#[derive(Debug)]
pub struct PreparedInstall {
    state: PreparedInstallState,
}

#[derive(Debug)]
enum PreparedInstallState {
    Legacy,
    Prepared(Box<dyn PreparedInstallPlan>),
}

/// An owned backend-specific plan ready to commit installation side effects.
#[async_trait]
pub(crate) trait PreparedInstallPlan: Debug + Send + 'static {
    async fn execute(self: Box<Self>, ctx: &InstallContext, tv: ToolVersion)
    -> Result<ToolVersion>;
}

impl PreparedInstall {
    pub(crate) fn legacy() -> Self {
        Self {
            state: PreparedInstallState::Legacy,
        }
    }

    pub(crate) fn prepared(plan: impl PreparedInstallPlan + 'static) -> Self {
        Self {
            state: PreparedInstallState::Prepared(Box::new(plan)),
        }
    }

    pub(crate) async fn execute(
        self,
        backend: &(impl Backend + ?Sized),
        ctx: &InstallContext,
        tv: ToolVersion,
    ) -> Result<ToolVersion> {
        match self.state {
            PreparedInstallState::Legacy => backend.install_version_(ctx, tv).await,
            PreparedInstallState::Prepared(plan) => plan.execute(ctx, tv).await,
        }
    }
}
