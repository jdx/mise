use crate::toolset::ToolVersion;

/// A backend's fixed installation inputs, resolved before installation begins.
///
/// Backends remain on `Legacy` until they migrate their full prepare/execute
/// path. This keeps the transition explicit without changing their behavior.
#[derive(Debug)]
pub struct PreparedInstall {
    kind: PreparedInstallKind,
}

#[derive(Debug)]
enum PreparedInstallKind {
    Legacy,
    Http(Box<PreparedHttpInstall>),
}

/// The HTTP inputs that installation is allowed to consume.
///
/// Configured verification inputs apply to fresh resolution. Once a lock URL
/// is replayed, its checksum and size are the sole artifact contract.
#[derive(Debug)]
pub(crate) struct PreparedHttpInstall {
    pub(crate) target: String,
    pub(crate) url: String,
    pub(crate) lock_checksum: Option<String>,
    pub(crate) lock_size: Option<u64>,
    pub(crate) configured_checksum: Option<String>,
    pub(crate) configured_size: Option<u64>,
    pub(crate) format: Option<String>,
    pub(crate) strip_components: Option<String>,
    pub(crate) bin: Option<String>,
    pub(crate) rename_exe: Option<String>,
    pub(crate) bin_path: Option<String>,
}

impl PreparedInstall {
    pub(crate) fn legacy() -> Self {
        Self {
            kind: PreparedInstallKind::Legacy,
        }
    }

    pub(crate) fn http(spec: PreparedHttpInstall) -> Self {
        Self {
            kind: PreparedInstallKind::Http(Box::new(spec)),
        }
    }

    pub(crate) fn is_legacy(&self) -> bool {
        matches!(&self.kind, PreparedInstallKind::Legacy)
    }

    pub(crate) fn http_spec(&self) -> eyre::Result<&PreparedHttpInstall> {
        match &self.kind {
            PreparedInstallKind::Http(spec) => Ok(spec.as_ref()),
            PreparedInstallKind::Legacy => Err(eyre::eyre!("expected prepared HTTP installation")),
        }
    }

    fn kind_name(&self) -> &'static str {
        match &self.kind {
            PreparedInstallKind::Legacy => "legacy",
            PreparedInstallKind::Http(_) => "http",
        }
    }
}

/// A completed installation tied to the exact prepared inputs it consumed.
#[derive(Debug)]
pub struct SuccessfulInstall {
    tool_version: ToolVersion,
    prepared: PreparedInstall,
}

impl SuccessfulInstall {
    pub(crate) fn new(tool_version: ToolVersion, prepared: PreparedInstall) -> Self {
        Self {
            tool_version,
            prepared,
        }
    }

    pub(crate) fn into_tool_version(self) -> ToolVersion {
        trace!(
            "completed {} prepared install for {}",
            self.prepared.kind_name(),
            self.tool_version
        );
        self.tool_version
    }
}
