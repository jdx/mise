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
/// Tool layers whose tool, version, mount point, and file owner match the
/// previously pushed image (or `--cache-from`) are reused without being
/// rebuilt — those tools don't even need to be installed locally. Pass
/// `--no-cache` to force a full local rebuild.
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

    /// Reuse unchanged tool layers from this image instead of the destination ref
    ///
    /// Must live in the same repository as the destination. Useful when each
    /// push gets a unique tag (e.g. per-commit tags in CI):
    /// `--cache-from ghcr.io/me/dev:latest ghcr.io/me/dev:$SHA`.
    #[clap(long, value_name = "REF", conflicts_with_all = &["no_cache", "image_dir"])]
    cache_from: Option<String>,

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

    /// Don't reuse tool layers from the previously pushed image
    #[clap(long)]
    no_cache: bool,

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

    /// Maintain the tag as a multi-arch image index
    ///
    /// Pushes this build's manifest by digest and points the tag at an OCI
    /// image index containing one entry per platform, preserving entries
    /// other architectures pushed. Run `mise oci push --update-index` from
    /// one runner per platform to assemble a multi-arch tag.
    #[clap(long)]
    update_index: bool,
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
        let mut reused_layers = 0;
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
                    reuse_from: self.fetch_layer_cache().await?,
                };
                let built = perform_build(opts, self.include_global).await?;
                reused_layers = built.tool_layers.iter().filter(|l| l.reused).count();
                info!("built image: {}", built.manifest_digest);
                (out_dir, Some(td))
            };

        let summary = registry::push_image(&image_dir, &self.reference, self.update_index).await?;
        let mut extras = String::new();
        if summary.mounted > 0 {
            extras.push_str(&format!(", {} mounted from base repo", summary.mounted));
        }
        if reused_layers > 0 {
            extras.push_str(&format!(
                ", {reused_layers} tool layer(s) reused from previous image"
            ));
        }
        miseprintln!(
            "pushed {} to {} ({} blob(s) uploaded, {} already present{extras})",
            summary.manifest_digest,
            self.reference,
            summary.uploaded,
            summary.skipped
        );
        if let Some(index_digest) = &summary.index_digest {
            miseprintln!("updated image index: {index_digest}");
        }
        Ok(())
    }

    /// Fetch the layer-reuse cache image: `--cache-from` if given, otherwise
    /// the destination ref itself (the previously pushed image under this
    /// tag). Returns `None` with `--no-cache`, when no previous image exists,
    /// or when the lookup fails — a broken cache must never fail the push.
    async fn fetch_layer_cache(&self) -> Result<Option<registry::RemoteImage>> {
        if self.no_cache {
            return Ok(None);
        }
        let cache_ref = self.cache_from.as_deref().unwrap_or(&self.reference);
        if let Some(cache_from) = &self.cache_from {
            // Reused layer blobs are never uploaded — they must already live
            // in the destination repository, so a cache image from a
            // different repo would produce a manifest referencing blobs the
            // destination doesn't have.
            let dest = registry::Reference::parse(&self.reference)?;
            let cache = registry::Reference::parse(cache_from)?;
            if dest.registry != cache.registry || dest.repository != cache.repository {
                bail!(
                    "--cache-from must reference the same repository as the destination \
                     (got {}/{}, destination is {}/{})",
                    cache.registry,
                    cache.repository,
                    dest.registry,
                    dest.repository
                );
            }
        }
        match registry::fetch_remote_image(cache_ref).await {
            Ok(remote) => {
                if remote.is_none() {
                    debug!("no previous image at {cache_ref} — building all layers locally");
                }
                Ok(remote)
            }
            Err(e) => {
                warn!("could not fetch layer cache from {cache_ref}: {e} — building all layers");
                Ok(None)
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

<bold><underline>Auth:</underline></bold>

    Credentials are resolved the same way docker/podman resolve them:
    <bold>$REGISTRY_AUTH_FILE</bold>, <bold>$XDG_RUNTIME_DIR/containers/auth.json</bold>,
    <bold>~/.config/containers/auth.json</bold>, then <bold>~/.docker/config.json</bold>
    (inline auths and credential helpers). Log in with either:
    $ <bold>docker login ghcr.io</bold>
    $ <bold>podman login ghcr.io</bold>
"#
);
