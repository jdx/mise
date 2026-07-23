use crate::backend::Backend;
use crate::install_context::InstallContext;
use crate::toolset::ToolVersion;
use async_trait::async_trait;
use eyre::Result;
use std::fmt::Debug;
use std::sync::Arc;

/// A backend's fixed installation inputs, resolved before installation begins.
///
/// Backends remain on `Legacy` until they migrate their full prepare/execute
/// path. This keeps the transition explicit without changing their behavior.
#[derive(Debug)]
pub struct PreparedInstall {
    state: PreparedInstallState,
}

#[derive(Debug)]
enum PreparedInstallState {
    Legacy,
    Prepared(Box<dyn PreparedInstallPlan>),
}

/// A fixed backend-specific installation plan and its matching executor.
///
/// Owning execution here keeps backend-specific prepared inputs out of the
/// object-safe [`Backend`] trait without requiring a central variant for each
/// backend.
#[async_trait]
pub(crate) trait PreparedInstallPlan: Debug + Send + 'static {
    fn evidence(&self) -> Arc<dyn PreparedInstallEvidencePayload>;

    async fn execute(self: Box<Self>, ctx: &InstallContext, tv: ToolVersion)
    -> Result<ToolVersion>;
}

/// Immutable backend-specific proof of the inputs a prepared plan will use.
pub(crate) trait PreparedInstallEvidencePayload: Debug + Send + Sync + 'static {}

impl<T> PreparedInstallEvidencePayload for T where T: Debug + Send + Sync + 'static {}

#[derive(Debug)]
enum PreparedInstallEvidence {
    Legacy,
    Prepared(Arc<dyn PreparedInstallEvidencePayload>),
}

impl PreparedInstallEvidence {
    fn kind_name(&self) -> &'static str {
        match self {
            Self::Legacy => "legacy",
            Self::Prepared(evidence) => {
                debug_assert!(Arc::strong_count(evidence) > 0);
                "prepared"
            }
        }
    }
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
    ) -> Result<SuccessfulInstall> {
        let (tool_version, evidence) = match self.state {
            PreparedInstallState::Legacy => (
                backend.install_version_(ctx, tv).await?,
                PreparedInstallEvidence::Legacy,
            ),
            PreparedInstallState::Prepared(plan) => {
                let evidence = PreparedInstallEvidence::Prepared(plan.evidence());
                (plan.execute(ctx, tv).await?, evidence)
            }
        };
        Ok(SuccessfulInstall::new(tool_version, evidence))
    }
}

/// A completed installation tied to the exact prepared inputs it consumed.
#[derive(Debug)]
pub struct SuccessfulInstall {
    tool_version: ToolVersion,
    evidence: PreparedInstallEvidence,
}

impl SuccessfulInstall {
    fn new(tool_version: ToolVersion, evidence: PreparedInstallEvidence) -> Self {
        Self {
            tool_version,
            evidence,
        }
    }

    pub(crate) fn into_tool_version(self) -> ToolVersion {
        trace!(
            "completed {} install for {}",
            self.evidence.kind_name(),
            self.tool_version
        );
        self.tool_version
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::VersionInfo;
    use crate::cli::args::BackendArg;
    use crate::config::Config;
    use crate::toolset::{ToolRequest, ToolSource, ToolVersionOptions, Toolset};
    use crate::ui::progress_report::QuietReport;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Debug)]
    struct CountingBackend {
        ba: Arc<BackendArg>,
        install_calls: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl Backend for CountingBackend {
        fn ba(&self) -> &Arc<BackendArg> {
            &self.ba
        }

        async fn _list_remote_versions(&self, _config: &Arc<Config>) -> Result<Vec<VersionInfo>> {
            Ok(vec![])
        }

        async fn install_version_(
            &self,
            _ctx: &InstallContext,
            tv: ToolVersion,
        ) -> Result<ToolVersion> {
            self.install_calls.fetch_add(1, Ordering::SeqCst);
            Ok(tv)
        }
    }

    #[derive(Debug)]
    struct CountingPlan {
        execute_calls: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl PreparedInstallPlan for CountingPlan {
        fn evidence(&self) -> Arc<dyn PreparedInstallEvidencePayload> {
            self.execute_calls.clone()
        }

        async fn execute(
            self: Box<Self>,
            _ctx: &InstallContext,
            tv: ToolVersion,
        ) -> Result<ToolVersion> {
            self.execute_calls.fetch_add(1, Ordering::SeqCst);
            Ok(tv)
        }
    }

    fn tool_version(ba: Arc<BackendArg>) -> ToolVersion {
        let request = ToolRequest::Version {
            backend: ba,
            version: "1.0.0".into(),
            options: ToolVersionOptions::default(),
            source: ToolSource::Argument,
        };
        ToolVersion::new(request, "1.0.0".into())
    }

    #[tokio::test]
    async fn prepared_install_dispatches_legacy_and_owned_plans_separately() {
        let ba = Arc::new(BackendArg::from("prepared-install-test"));
        let legacy_calls = Arc::new(AtomicUsize::new(0));
        let backend = CountingBackend {
            ba: ba.clone(),
            install_calls: legacy_calls.clone(),
        };
        let ctx = InstallContext {
            config: Config::get().await.unwrap(),
            ts: Arc::new(Toolset::default()),
            pr: Box::new(QuietReport::new()),
            force: false,
            dry_run: false,
            locked: false,
            before_date: None,
        };

        let legacy = PreparedInstall::legacy()
            .execute(&backend, &ctx, tool_version(ba.clone()))
            .await
            .unwrap()
            .into_tool_version();
        assert_eq!(legacy.version, "1.0.0");
        assert_eq!(legacy_calls.load(Ordering::SeqCst), 1);

        let execute_calls = Arc::new(AtomicUsize::new(0));
        let prepared = PreparedInstall::prepared(CountingPlan {
            execute_calls: execute_calls.clone(),
        })
        .execute(&backend, &ctx, tool_version(ba))
        .await
        .unwrap()
        .into_tool_version();
        assert_eq!(prepared.version, "1.0.0");
        assert_eq!(execute_calls.load(Ordering::SeqCst), 1);
        assert_eq!(legacy_calls.load(Ordering::SeqCst), 1);
    }
}
