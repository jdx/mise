use std::path::PathBuf;

use clap::ValueHint;
use eyre::{Context, Result, bail};
use tempfile::TempDir;

use crate::cli::oci::common::perform_build;
use crate::config::Settings;
use crate::oci::{BuildOptions, LayerOwner, registry};

/// [experimental] Build an OCI image and push it to a registry
///
/// Pushes with mise's built-in registry client — no skopeo/crane/docker
/// required. If `--image-dir` is not passed, builds fresh from the current
/// mise.toml first. Only blobs the registry doesn't already have are
/// uploaded, so repeat pushes of mostly-unchanged toolsets are cheap.
///
/// Credentials are read from the same places docker and podman use:
/// `$REGISTRY_AUTH_FILE`, `$XDG_RUNTIME_DIR/containers/auth.json`,
/// `~/.config/containers/auth.json`, and `~/.docker/config.json`
/// (including credential helpers) — so `docker login` / `podman login`
/// is all the setup needed.
///
/// Requires `mise settings experimental=true` (or `MISE_EXPERIMENTAL=1`).
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Push {
    /// Destination registry reference (e.g. `ghcr.io/me/devenv:latest`)
    #[clap(value_name = "REF")]
    reference: String,

    /// Base image for the build (ignored with --image-dir)
    #[clap(long)]
    from: Option<String>,

    /// Push an already-built OCI image layout (skip the build step)
    #[clap(long, value_hint = ValueHint::DirPath, conflicts_with_all = &["from", "mount_point", "no_mise", "owner", "include_global"])]
    image_dir: Option<PathBuf>,

    /// Also include tools from the global / system config (default: project-only)
    ///
    /// See `mise oci build --help` for details.
    #[clap(long)]
    include_global: bool,

    /// Override in-image mount point (ignored with --image-dir)
    #[clap(long)]
    mount_point: Option<String>,

    /// Don't embed the mise binary (ignored with --image-dir)
    #[clap(long)]
    no_mise: bool,

    /// UID[:GID] to assign to every tar entry when building (conflicts with --image-dir)
    ///
    /// Overrides [oci].user_id / [oci].group_id. Defaults to 0:0. If GID is
    /// omitted, it defaults to UID. This affects file ownership only; [oci].user
    /// controls the image USER directive.
    #[clap(long, value_name = "UID[:GID]")]
    owner: Option<LayerOwner>,
}

impl Push {
    pub async fn run(self) -> Result<()> {
        Settings::get().ensure_experimental("mise oci push")?;

        if !self.reference.contains('/') {
            bail!(
                "push destination must be a fully-qualified reference \
                 (e.g. `ghcr.io/you/devenv:tag`); got {:?}",
                self.reference
            );
        }
        // Keep the temp dir alive for the duration of the push — it removes
        // itself on drop, so multi-hundred-megabyte image layouts don't
        // accumulate in /tmp.
        let (image_dir, _tempdir_guard): (PathBuf, Option<TempDir>) =
            if let Some(d) = &self.image_dir {
                if !d.join("index.json").is_file() {
                    bail!(
                        "{}: does not look like an OCI image layout (missing index.json)",
                        d.display()
                    );
                }
                (d.clone(), None)
            } else {
                let td = TempDir::with_prefix("mise-oci-push-")
                    .wrap_err("creating temp dir for oci build output")?;
                let out_dir = td.path().join("image");
                let opts = BuildOptions {
                    out_dir: out_dir.clone(),
                    from: self.from.clone(),
                    tag: Some(self.reference.clone()),
                    mount_point: self.mount_point.clone(),
                    owner: self.owner,
                    include_mise: !self.no_mise,
                    copy: vec![],
                };
                let built = perform_build(opts, self.include_global).await?;
                info!("built image: {}", built.manifest_digest);
                (out_dir, Some(td))
            };

        let summary = registry::push_image(&image_dir, &self.reference).await?;
        let mounted = if summary.mounted > 0 {
            format!(", {} mounted from base repo", summary.mounted)
        } else {
            String::new()
        };
        miseprintln!(
            "pushed {} to {} ({} blob(s) uploaded, {} already present{mounted})",
            summary.manifest_digest,
            self.reference,
            summary.uploaded,
            summary.skipped
        );
        Ok(())
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    Build and push to GHCR:
    $ <bold>mise oci push ghcr.io/me/devenv:latest</bold>

    Push an image built earlier:
    $ <bold>mise oci build -o ./img</bold>
    $ <bold>mise oci push --image-dir ./img ghcr.io/me/devenv:v1</bold>

<bold><underline>Auth:</underline></bold>

    Credentials are resolved the same way docker/podman resolve them:
    <bold>$REGISTRY_AUTH_FILE</bold>, <bold>$XDG_RUNTIME_DIR/containers/auth.json</bold>,
    <bold>~/.config/containers/auth.json</bold>, then <bold>~/.docker/config.json</bold>
    (inline auths and credential helpers). Log in with either:
    $ <bold>docker login ghcr.io</bold>
    $ <bold>podman login ghcr.io</bold>
"#
);
