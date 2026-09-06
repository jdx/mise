use std::sync::Arc;

use eyre::{Result, WrapErr, bail, eyre};
use indexmap::IndexSet;
use jiff::Timestamp;
use tokio::sync::OnceCell;

use crate::cli::args::BackendArg;
use crate::ui::progress_report::SingleReport;
use crate::{
    config::Config,
    toolset::{ResolveOptions, ToolRequest, ToolRequestSet, Toolset},
};

/// The normalized dependency declarations that apply while installing one tool.
///
/// Backend/plugin dependencies (including optional and recursive dependencies) and
/// direct per-tool `depends` entries share one ordered, deduplicated collection.
/// Matching uses `BackendArg::all_fulls`, just like install scheduling, so aliases
/// and explicit backend identities select the same configured tools.
#[derive(Debug, Default)]
pub(crate) struct InstallDependencyDeclarations {
    dependencies: IndexSet<BackendArg>,
    metadata_error: Option<String>,
}

impl InstallDependencyDeclarations {
    pub(crate) fn iter(&self) -> impl Iterator<Item = &BackendArg> {
        self.dependencies.iter()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.dependencies.is_empty()
    }

    pub(crate) fn matches(&self, candidate: &BackendArg) -> bool {
        self.dependencies
            .iter()
            .any(|dependency| backend_args_match(dependency, candidate))
    }

    fn validate(&self) -> Result<()> {
        match &self.metadata_error {
            Some(err) => Err(eyre!(err.clone())),
            None => Ok(()),
        }
    }

    fn insert(&mut self, target: &BackendArg, dependency: BackendArg) {
        if backend_args_match(target, &dependency)
            || self
                .dependencies
                .iter()
                .any(|existing| backend_args_match(existing, &dependency))
        {
            return;
        }
        self.dependencies.insert(dependency);
    }

    fn matching_requests(&self, requests: &ToolRequestSet) -> ToolRequestSet {
        requests
            .iter()
            .filter(|(ba, ..)| self.matches(ba))
            .map(|(ba, requests, source)| (ba.clone(), requests.clone(), source.clone()))
            .collect()
    }
}

fn backend_args_match(left: &BackendArg, right: &BackendArg) -> bool {
    let left = left.all_fulls();
    let right = right.all_fulls();
    left.iter().any(|identity| right.contains(identity))
}

/// Compute all install-time dependency declarations for a request.
///
/// The returned collection always includes direct user declarations. A backend
/// metadata error is retained so strict consumers can propagate it while the install
/// graph can preserve its historical behavior of ignoring that error.
pub(crate) fn install_dependency_declarations(
    request: &ToolRequest,
) -> InstallDependencyDeclarations {
    let mut declarations = InstallDependencyDeclarations::default();
    match request
        .backend()
        .and_then(|backend| backend.get_all_dependencies(true))
    {
        Ok(dependencies) => {
            for dependency in dependencies {
                declarations.insert(request.ba(), dependency);
            }
        }
        Err(err) => declarations.metadata_error = Some(format!("{err:#}")),
    }

    if let Some(dependencies) = request.options().core.depends {
        for raw in dependencies {
            let dependency = BackendArg::from(raw.as_str());
            declarations.insert(request.ba(), dependency);
        }
    }
    declarations
}

/// The configured and offline-resolved view of one tool's install dependencies.
#[derive(Debug)]
pub(crate) struct InstallDependencyContext {
    pub(crate) declarations: InstallDependencyDeclarations,
    pub(crate) requests: ToolRequestSet,
    pub(crate) toolset: Toolset,
    pub(crate) paths: Vec<std::path::PathBuf>,
}

impl InstallDependencyContext {
    async fn resolve(config: &Arc<Config>, request: &ToolRequest) -> Result<Self> {
        let declarations = install_dependency_declarations(request);
        declarations.validate()?;
        let requests = declarations.matching_requests(config.get_tool_request_set().await?);
        let mut toolset: Toolset = requests.clone().into();
        let resolve_options = ResolveOptions {
            offline: true,
            ..Default::default()
        };
        for (dependency, versions) in &mut toolset.versions {
            versions
                .resolve(config, &resolve_options)
                .await
                .wrap_err_with(|| {
                    format!(
                        "failed to resolve configured install dependency '{}' for '{}'",
                        dependency, request
                    )
                })?;
        }
        for (backend, dependency) in toolset.list_current_versions() {
            if !backend.is_version_installed(config, &dependency, true) {
                let unresolved = !dependency.resolved_from_lockfile()
                    && match &dependency.request {
                        ToolRequest::Prefix { .. } | ToolRequest::Sub { .. } => true,
                        ToolRequest::Version { version, .. } => {
                            version == "latest" || backend.is_rolling_channel(version)
                        }
                        ToolRequest::Ref { .. }
                        | ToolRequest::Path { .. }
                        | ToolRequest::System { .. } => false,
                    };
                if unresolved {
                    bail!(
                        "failed to resolve configured install dependency '{}' for '{}' while offline",
                        dependency.request,
                        request
                    );
                }
                bail!(
                    "tool '{}' requires configured install dependency '{}', but its selected version is not installed\n\
                     hint: Run `mise install {}` before installing '{}'. Remove the dependency from configuration to allow the install hook to rely on system PATH instead.",
                    request,
                    dependency.request,
                    dependency.request,
                    request,
                );
            }
        }
        let paths = toolset.list_paths_strict(config, request).await?;
        let context = Self {
            declarations,
            requests,
            toolset,
            paths,
        };
        debug_assert_eq!(context.requests.tools.len(), context.toolset.versions.len());
        debug_assert!(
            context
                .toolset
                .versions
                .keys()
                .all(|backend| context.declarations.matches(backend))
        );
        Ok(context)
    }
}

pub(crate) struct InstallContext {
    pub config: Arc<Config>,
    pub ts: Arc<Toolset>,
    pub pr: Box<dyn SingleReport>,
    pub force: bool,
    pub dry_run: bool,
    /// require lockfile URLs to be present; fail if not
    pub locked: bool,
    pub before_date: Option<Timestamp>,
    /// One install context belongs to exactly one tool request, so this cache is
    /// intentionally unkeyed. Every caller must use that same request.
    pub(crate) dependency_context: OnceCell<InstallDependencyContext>,
}

impl InstallContext {
    /// Resolve the dependency context for this install's single tool request.
    pub(crate) async fn dependency_context(
        &self,
        request: &ToolRequest,
    ) -> Result<&InstallDependencyContext> {
        self.dependency_context
            .get_or_try_init(|| InstallDependencyContext::resolve(&self.config, request))
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::toolset::{ToolSource, parse_tool_options};

    fn request(tool: &str, options: &str) -> ToolRequest {
        ToolRequest::new_with_options(
            Arc::new(BackendArg::from(tool)),
            "1.0.0",
            parse_tool_options(options),
            ToolSource::Argument,
        )
        .unwrap()
    }

    fn names(declarations: &InstallDependencyDeclarations) -> Vec<String> {
        declarations.iter().map(|ba| ba.short.clone()).collect()
    }

    #[tokio::test]
    async fn backend_dependencies_include_required_optional_and_recursive_collection() {
        let _config = Config::get().await.unwrap();
        let declarations = install_dependency_declarations(&request("cargo:eza", ""));
        declarations.validate().unwrap();
        let names = names(&declarations);
        assert!(names.contains(&"rust".to_string()));
        assert!(names.contains(&"cargo-binstall".to_string()));
        assert!(names.contains(&"sccache".to_string()));
    }

    #[tokio::test]
    async fn per_tool_dependencies_are_included() {
        let _config = Config::get().await.unwrap();
        let declarations = install_dependency_declarations(&request(
            "http:example",
            r#"depends=["node","python"]"#,
        ));
        declarations.validate().unwrap();
        assert_eq!(names(&declarations), vec!["node", "python"]);
    }

    #[tokio::test]
    async fn backend_and_per_tool_dependencies_are_unioned_deterministically() {
        let _config = Config::get().await.unwrap();
        let declarations = install_dependency_declarations(&request(
            "cargo:eza",
            r#"depends=["python","rust","python"]"#,
        ));
        declarations.validate().unwrap();
        assert_eq!(
            names(&declarations),
            vec!["rust", "cargo-binstall", "sccache", "python"]
        );
    }

    #[tokio::test]
    async fn aliases_and_full_backend_identities_are_deduplicated() {
        let _config = Config::get().await.unwrap();
        let declarations = install_dependency_declarations(&request(
            "http:example",
            r#"depends=["nodejs","node","core:node"]"#,
        ));
        declarations.validate().unwrap();
        assert_eq!(names(&declarations), vec!["node"]);
        assert!(declarations.matches(&BackendArg::from("core:node")));
    }

    #[tokio::test]
    async fn self_dependencies_are_removed_across_aliases() {
        let _config = Config::get().await.unwrap();
        let declarations = install_dependency_declarations(&request(
            "cargo:cargo-binstall",
            r#"depends=["cargo-binstall","cargo:cargo-binstall"]"#,
        ));
        declarations.validate().unwrap();
        assert!(!declarations.matches(&BackendArg::from("cargo-binstall")));
        assert_eq!(names(&declarations), vec!["rust", "sccache"]);
    }

    #[tokio::test]
    async fn metadata_errors_preserve_direct_dependencies() {
        let _config = Config::get().await.unwrap();
        let declarations =
            install_dependency_declarations(&request("unknown:example", r#"depends=["node"]"#));
        assert!(declarations.validate().is_err());
        assert_eq!(names(&declarations), vec!["node"]);
    }
}
