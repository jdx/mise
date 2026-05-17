use std::path::PathBuf;
use std::process::Command;

use clap::ValueHint;
use eyre::{Context, Result, bail};
use tempfile::TempDir;

use crate::cli::oci::common::perform_build;
use crate::config::Settings;
use crate::file;
use crate::oci::BuildOptions;

/// [experimental] Build an OCI image and push it to a registry
///
/// Requires `skopeo` (or `crane`) on PATH. If `--image-dir` is not passed,
/// builds fresh from the current mise.toml first, then shells out to
/// `skopeo copy oci:<dir> docker://<ref>` (or `crane push <dir> <ref>`).
/// Authentication is handled by the underlying tool — configure it the same
/// way you would for a plain `skopeo` / `crane` push (e.g. `docker login`,
/// `REGISTRY_AUTH_FILE`, `~/.config/containers/auth.json`).
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
    #[clap(long, value_hint = ValueHint::DirPath, conflicts_with_all = &["from", "mount_point", "no_mise", "include_global"])]
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

    /// Force the push tool (`auto`, `skopeo`, `crane`). Default `auto`.
    #[clap(long, default_value = "auto")]
    tool: Tool,
}

#[derive(Debug, Clone, Copy, PartialEq, clap::ValueEnum)]
enum Tool {
    Auto,
    Skopeo,
    Crane,
}

impl Push {
    pub async fn run(self) -> Result<()> {
        Settings::get().ensure_experimental("mise oci push")?;

        // Validate arguments BEFORE we go looking for an external tool,
        // so argument errors always win over "tool not installed" errors.
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
                    include_mise: !self.no_mise,
                };
                let built = perform_build(opts, self.include_global).await?;
                info!("built image: {}", built.manifest_digest);
                (out_dir, Some(td))
            };

        // Resolve tool after argument validation so bad args don't mask
        // "tool missing" errors (and vice versa).
        let tool = select_tool(self.tool)?;

        match tool {
            Tool::Skopeo => {
                let src = format!("oci:{}", image_dir.display());
                let dst = format!("docker://{}", self.reference);
                info!("skopeo copy {src} {dst}");
                let status = Command::new("skopeo")
                    .args(["copy", &src, &dst])
                    .status()
                    .wrap_err("running `skopeo copy`")?;
                if !status.success() {
                    bail!("skopeo copy exited with {status:?}");
                }
            }
            Tool::Crane => {
                // `crane push <dir> <ref>` takes an OCI image layout directly.
                info!("crane push {} {}", image_dir.display(), self.reference);
                let status = Command::new("crane")
                    .arg("push")
                    .arg(&image_dir)
                    .arg(&self.reference)
                    .status()
                    .wrap_err("running `crane push`")?;
                if !status.success() {
                    bail!("crane push exited with {status:?}");
                }
            }
            Tool::Auto => unreachable!(),
        }

        miseprintln!("pushed {} to {}", image_dir.display(), self.reference);
        Ok(())
    }
}

fn select_tool(requested: Tool) -> Result<Tool> {
    match requested {
        Tool::Skopeo => {
            if file::which("skopeo").is_none() {
                bail!("--tool skopeo requested but `skopeo` was not found on PATH");
            }
            Ok(Tool::Skopeo)
        }
        Tool::Crane => {
            if file::which("crane").is_none() {
                bail!("--tool crane requested but `crane` was not found on PATH");
            }
            Ok(Tool::Crane)
        }
        Tool::Auto => {
            if file::which("skopeo").is_some() {
                Ok(Tool::Skopeo)
            } else if file::which("crane").is_some() {
                Ok(Tool::Crane)
            } else {
                bail!(
                    "no supported push tool found. Install one of:\n  \
                       - skopeo (recommended)\n  \
                       - crane\nand configure registry auth for it."
                )
            }
        }
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    Build and push to GHCR:
    $ <bold>mise oci push ghcr.io/me/devenv:latest</bold>

    Push an image built earlier:
    $ <bold>mise oci build -o ./img</bold>
    $ <bold>mise oci push --image-dir ./img ghcr.io/me/devenv:v1</bold>

    Force a specific push tool:
    $ <bold>mise oci push --tool crane ghcr.io/me/devenv:latest</bold>

<bold><underline>Auth:</underline></bold>

    mise shells out to <bold>skopeo</bold> (preferred) or <bold>crane</bold>; configure registry
    credentials the usual way — `docker login`, `REGISTRY_AUTH_FILE`,
    or `~/.config/containers/auth.json` for skopeo; `crane auth login`
    for crane.
"#
);
