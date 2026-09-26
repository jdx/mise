//! Orchestrates building an OCI image from a resolved mise Toolset.
//!
//! Produces an OCI image layout with one layer per tool version so that
//! bumping any single tool invalidates exactly one content-addressable blob.
//! See the README in this module for the design.

use std::path::PathBuf;
use std::sync::Arc;

use eyre::{Context, Result, bail};
use indexmap::{IndexMap, IndexSet};
use sha2::{Digest, Sha256};

use crate::backend::backend_type::BackendType;
use crate::config::{Config, Settings};
use crate::file;
use crate::oci::layer::{self, LayerBlob, LayerOwner, PythonRelocation};
use crate::oci::layout::ImageLayout;
use crate::oci::manifest::{self, Descriptor, ImageConfig, ImageManifest, Platform, RootFs};
use crate::oci::packages;
use crate::oci::registry;
use crate::oci::{OciConfig, OciCopy};
use crate::system::ManagerPackages;
use crate::system::files::{FileMode, FileRequest};
use crate::toolset::{ToolVersion, Toolset};

/// Annotations mise writes on tool layer descriptors. Together they form the
/// cache key for reusing a layer from a previously pushed image: same tool,
/// same version, same in-image prefix, same file ownership.
pub(crate) const ANNOTATION_TOOL_SHORT: &str = "dev.mise.tool.short";
pub(crate) const ANNOTATION_TOOL_VERSION: &str = "dev.mise.tool.version";
pub(crate) const ANNOTATION_LAYER_PREFIX: &str = "dev.mise.layer.prefix";
pub(crate) const ANNOTATION_LAYER_OWNER: &str = "dev.mise.layer.owner";
pub(crate) const ANNOTATION_LAYER_RELOCATION: &str = "dev.mise.layer.relocation";
/// Fingerprint of the vfox plugin that installed a tool layer. Part of the
/// reuse key: a plugin update can change what the same version installs.
/// Absent for tools that aren't installed by a plugin.
pub(crate) const ANNOTATION_TOOL_PLUGIN: &str = "dev.mise.tool.plugin";
/// Marks a layer holding a vfox plugin's sources under `<mount>/plugins/`.
pub(crate) const ANNOTATION_PLUGIN: &str = "dev.mise.plugin";

const TOOL_LAYER_RELOCATION_VERSION: &str = "2";

/// Options passed to the builder from the CLI.
#[derive(Debug, Clone)]
pub(crate) struct BuildOptions {
    /// Output directory for the OCI image layout.
    pub out_dir: PathBuf,
    /// Base image reference (overrides mise.toml and default setting).
    pub from: Option<String>,
    /// Tag to write into index.json (ref.name annotation).
    pub tag: Option<String>,
    /// Where mise tools get installed inside the image.
    pub mount_point: Option<String>,
    /// Numeric owner assigned to every tar entry in generated layers.
    pub owner: Option<LayerOwner>,
    /// Embed the current mise binary at /usr/local/bin/mise.
    pub include_mise: bool,
    /// CLI-provided host paths copied after config-provided entries.
    pub copy: Vec<OciCopy>,
    /// A previously pushed image to reuse unchanged tool layers from
    /// (`mise oci push` only). Reused layers skip the local tar/gzip build
    /// entirely — and the tool doesn't even need to be installed locally.
    /// NOTE: the resulting layout omits reused layer blobs, so it is only
    /// valid to push to the repository the cache image came from.
    pub reuse_from: Option<registry::RemoteImage>,
    /// Push destination; permits base blobs already in that repository to
    /// remain remote when no build step needs to unpack them.
    pub push_destination: Option<String>,
    /// Bypass the local tool-layer cache (remote reuse is supplied separately).
    pub no_cache: bool,
}

/// Cache key for tool-layer reuse. All parts must match — a layer built
/// for a different mount point or file owner has different bytes even for
/// the same tool version.
#[derive(Debug, PartialEq, Eq, Hash)]
struct ReuseKey {
    short: String,
    version: String,
    prefix: String,
    owner: String,
    relocation: String,
    /// Empty for tools not installed by a plugin.
    plugin: String,
}

/// A tool layer taken verbatim from the remote cache image.
#[derive(Debug, Clone)]
struct ReusedLayer {
    media_type: String,
    digest: String,
    size: u64,
    diff_id: String,
}

/// Index a remote image's tool layers by their reuse cache key. Layers
/// missing any key annotation (e.g. images pushed by older mise versions)
/// are simply not reusable.
fn build_reuse_index(remote: &registry::RemoteImage) -> IndexMap<ReuseKey, ReusedLayer> {
    let mut index = IndexMap::new();
    for (layer, diff_id) in remote.manifest.layers.iter().zip(&remote.diff_ids) {
        let a = &layer.annotations;
        let (Some(short), Some(version), Some(prefix), Some(owner), Some(relocation)) = (
            a.get(ANNOTATION_TOOL_SHORT),
            a.get(ANNOTATION_TOOL_VERSION),
            a.get(ANNOTATION_LAYER_PREFIX),
            a.get(ANNOTATION_LAYER_OWNER),
            a.get(ANNOTATION_LAYER_RELOCATION),
        ) else {
            continue;
        };
        index.insert(
            ReuseKey {
                short: short.clone(),
                version: version.clone(),
                prefix: prefix.clone(),
                owner: owner.clone(),
                relocation: relocation.clone(),
                // Optional: layers from images pushed before plugin support
                // lack it, and only match tools without a plugin.
                plugin: a.get(ANNOTATION_TOOL_PLUGIN).cloned().unwrap_or_default(),
            },
            ReusedLayer {
                media_type: layer.media_type.clone(),
                digest: layer.digest.clone(),
                size: layer.size,
                diff_id: diff_id.clone(),
            },
        );
    }
    index
}

pub(crate) struct Builder {
    pub cfg: Arc<Config>,
    pub ts: Toolset,
    pub oci: OciConfig,
    pub opts: BuildOptions,
    pub dotfiles: Vec<FileRequest>,
    pub system_packages: Vec<ManagerPackages>,
}

/// Output summary returned to the CLI.
pub(crate) struct BuildOutput {
    pub out_dir: PathBuf,
    pub manifest_digest: String,
    pub tool_layers: Vec<ToolLayerInfo>,
}

pub(crate) struct ToolLayerInfo {
    pub short: String,
    pub version: String,
    pub digest: String,
    pub size: u64,
    /// Taken verbatim from the remote cache image instead of built locally.
    pub reused: bool,
}

impl Builder {
    pub(crate) fn new(cfg: Arc<Config>, ts: Toolset, oci: OciConfig, opts: BuildOptions) -> Self {
        Self {
            cfg,
            ts,
            oci,
            opts,
            dotfiles: vec![],
            system_packages: vec![],
        }
    }

    pub(crate) fn with_dotfiles(mut self, dotfiles: Vec<FileRequest>) -> Self {
        self.dotfiles = dotfiles;
        self
    }

    pub(crate) fn with_system_packages(mut self, system_packages: Vec<ManagerPackages>) -> Self {
        self.system_packages = system_packages;
        self
    }

    /// Build the image and write it to the output directory.
    pub(crate) async fn build(self) -> Result<BuildOutput> {
        let versions = self.ts.list_current_versions();
        if versions.is_empty() {
            warn!("mise oci build: no tools in the toolset — image will have only the base layer");
        }
        reject_unsupported_backends(&versions)?;

        file::create_dir_all(&self.opts.out_dir)?;
        let layout = ImageLayout::init(&self.opts.out_dir)?;

        let mount_point = self
            .opts
            .mount_point
            .clone()
            .or_else(|| self.oci.mount_point.clone())
            .unwrap_or_else(|| Settings::get().oci.default_mount_point.clone());
        let mount_point = mount_point.trim_end_matches('/').to_string();
        if mount_point.is_empty() {
            bail!("oci mount_point must not be empty");
        }
        if !mount_point.starts_with('/') {
            bail!(
                "oci mount_point must be an absolute path (got {mount_point:?}); \
                 a relative value makes MISE_DATA_DIR inside the container \
                 depend on the working directory and mis-resolve tools."
            );
        }

        let owner = resolve_layer_owner(self.opts.owner, &self.oci);
        let copies: Vec<&OciCopy> = self.oci.copy.iter().chain(&self.opts.copy).collect();

        // --- 1. Base image (optional) ---
        let from_ref = self
            .opts
            .from
            .clone()
            .or_else(|| self.oci.from.clone())
            .or_else(|| {
                let s = Settings::get().oci.default_from.clone();
                if s.is_empty() { None } else { Some(s) }
            })
            .filter(|r| !r.is_empty() && r != "scratch");

        let mut base_layers: Vec<Descriptor> = Vec::new();
        let mut base_diff_ids: Vec<String> = Vec::new();
        let mut base_config_json: Option<serde_json::Value> = None;
        let mut platform: Option<Platform> = None;

        if let Some(ref_) = &from_ref {
            info!("pulling base image: {ref_}");
            let desired = Some((
                crate::oci::normalize_arch(std::env::consts::ARCH),
                crate::oci::normalize_os(std::env::consts::OS),
            ));
            let destination = self
                .opts
                .push_destination
                .as_deref()
                .filter(|_| self.system_packages.is_empty());
            let pull = registry::pull_base_image(ref_, &layout, desired, destination)
                .await
                .wrap_err_with(|| format!("pulling base image {ref_}"))?;
            base_layers = pull
                .layers
                .iter()
                .map(|l| Descriptor {
                    media_type: manifest::media_type_to_oci(&l.media_type).to_string(),
                    size: l.size,
                    digest: l.digest.clone(),
                    annotations: l.annotations.clone(),
                    platform: l.platform.clone(),
                })
                .collect();

            // Extract the base's diff_ids. The OCI spec requires the image
            // config's `rootfs.diff_ids` to have exactly one entry per
            // manifest layer; a registry whose config is missing or
            // malformed here would silently produce an image that podman /
            // skopeo reject. Fail loudly instead.
            let diff_ids_raw = pull
                .config_json
                .get("rootfs")
                .and_then(|r| r.get("diff_ids"))
                .and_then(|d| d.as_array())
                .ok_or_else(|| {
                    eyre::eyre!(
                        "pulled base image {ref_} has no rootfs.diff_ids in its config \
                         — cannot produce a valid OCI image on top of it"
                    )
                })?;
            base_diff_ids = diff_ids_raw
                .iter()
                .map(|v| {
                    v.as_str().map(String::from).ok_or_else(|| {
                        eyre::eyre!("base image {ref_} has a non-string entry in rootfs.diff_ids")
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            if base_diff_ids.len() != base_layers.len() {
                bail!(
                    "base image {ref_} has {} layers in its manifest but {} diff_ids in its \
                     config — refusing to emit an OCI-spec-violating image",
                    base_layers.len(),
                    base_diff_ids.len()
                );
            }
            platform = pull.platform;
            base_config_json = Some(pull.config_json);
        }

        // --- 2. Decide layer reuse and validate tool installs ---
        // A tool layer is reused from the remote cache image when tool,
        // version, in-image prefix, and file owner all match — in that case
        // the layer is never built locally and the tool doesn't need to be
        // installed at all.
        let owner_str = format!("{}:{}", owner.uid, owner.gid);
        let plugins = build_plugin_layers(&self.cfg, &versions, &mount_point, owner).await?;
        let python_relocations: Vec<PythonRelocation> = versions
            .iter()
            .filter(|(backend, _)| is_python_backend(backend.as_ref()))
            .map(|(_, tv)| PythonRelocation {
                version: tv.version.clone(),
                host: tv.install_path(),
                image: PathBuf::from(tool_in_image_path(&mount_point, tv)),
            })
            .collect();
        let reuse_index = self
            .opts
            .reuse_from
            .as_ref()
            .map(build_reuse_index)
            .unwrap_or_default();
        let tool_reuse: Vec<Option<ReusedLayer>> = versions
            .iter()
            .zip(&plugins.tool_fingerprints)
            .map(|((_, tv), plugin)| {
                reuse_index
                    .get(&ReuseKey {
                        short: tv.ba().short.clone(),
                        version: tv.version.clone(),
                        prefix: tool_tar_prefix(&mount_point, tv),
                        owner: owner_str.clone(),
                        relocation: tool_layer_relocation_key(tv, &python_relocations),
                        plugin: plugin.clone(),
                    })
                    .cloned()
            })
            .collect();

        // Tool installs are host-native binaries. On non-linux hosts they'll
        // fail at runtime inside the linux container with `Exec format error`
        // — emit a single warning up front so the user isn't surprised after
        // the image appears to build successfully. (`--no-mise` silences the
        // mise-binary warning below but doesn't help with tool binaries; only
        // running the build on a linux host does.)
        // Count only layers actually built on this host; reused layers came
        // from the cache image and aren't rebuilt, so they don't carry
        // host-native binaries. Warning on reused-only pushes (the CI re-push
        // the reuse feature speeds up) would be a false alarm.
        let built_tool_count = tool_reuse.iter().filter(|r| r.is_none()).count();
        if built_tool_count > 0 && std::env::consts::OS != "linux" {
            warn!(
                "building on {host} host — {n} tool layer(s) contain {host} binaries that \
                 will fail with `Exec format error` inside a linux container. Run \
                 `mise oci build` on a linux host (or in a linux container) for a working image.",
                host = std::env::consts::OS,
                n = built_tool_count
            );
        }
        // Fail cheaply before unpacking a rootfs or installing system packages.
        // Confirm a missing path under the lock: a reinstall may temporarily
        // remove it. The packaging check remains authoritative for present paths.
        for (i, (_, tv)) in versions.iter().enumerate() {
            if tool_reuse[i].is_none() {
                preflight_tool_install(tv)?;
            }
        }

        // --- 3. System package layer (optional) ---
        let system_packages_layer = packages::build_system_packages_layer(
            &layout,
            &base_layers,
            &self.system_packages,
            platform
                .as_ref()
                .map(|p| p.architecture.as_str())
                .unwrap_or_else(|| crate::oci::normalize_arch(std::env::consts::ARCH)),
        )?;

        // --- 4. Per-tool layers ---
        struct ToolLayerEntry {
            short: String,
            version: String,
            prefix: String,
            relocation: String,
            plugin: String,
            layer: ToolLayer,
        }
        enum ToolLayer {
            Built(LayerBlob),
            Reused(ReusedLayer),
        }
        let mut tool_layers: Vec<ToolLayerEntry> = Vec::new();
        for (i, (_, tv)) in versions.iter().enumerate() {
            let tv_prefix = tool_tar_prefix(&mount_point, tv);
            let relocation_key = tool_layer_relocation_key(tv, &python_relocations);
            let layer = if let Some(reused) = &tool_reuse[i] {
                info!(
                    "oci: reusing {} layer from the cache image (unchanged)",
                    tv.style()
                );
                ToolLayer::Reused(reused.clone())
            } else {
                // Coordinate with install, link, and uninstall before inspecting
                // the source. Hold through fingerprinting, packaging, and cache
                // publication so a same-version reinstall cannot poison a key.
                let _install_lock = lock_tool_install(tv)?;
                require_tool_install(tv)?;
                let is_pipx = tv.ba().backend_type() == BackendType::Pipx;
                // Only pipx layers are expected to link into another tool's
                // install. Other backends get their own mapping for shebang
                // relocation without making their reuse key depend on the
                // complete toolset.
                let mut paths = vec![(
                    tv.install_path(),
                    PathBuf::from(tool_in_image_path(&mount_point, tv)),
                )];
                if is_pipx {
                    paths.extend(
                        python_relocations
                            .iter()
                            .map(|python| (python.host.clone(), python.image.clone())),
                    );
                }
                let pythons = if is_pipx {
                    python_relocations.clone()
                } else {
                    Vec::new()
                };
                let relocation = layer::ToolRelocation::new(paths).with_pythons(pythons);
                let blob = if self.opts.no_cache {
                    layer::build_relocated_tool_layer_from_dir(
                        &tv.install_path(),
                        &tv_prefix,
                        owner,
                        &relocation,
                    )
                } else {
                    layer::build_cached_tool_layer(
                        &tv.install_path(),
                        &tv_prefix,
                        owner,
                        &relocation,
                        &tv.cache_path().join("oci-layers"),
                    )
                    .map(|(blob, hit)| {
                        if hit {
                            info!("oci: reusing {} layer from the local cache", tv.style());
                        }
                        blob
                    })
                }
                .wrap_err_with(|| format!("building layer for {}", tv.style()))?;
                ToolLayer::Built(blob)
            };
            tool_layers.push(ToolLayerEntry {
                short: tv.ba().short.clone(),
                version: tv.version.clone(),
                prefix: tv_prefix,
                relocation: relocation_key,
                plugin: plugins.tool_fingerprints[i].clone(),
                layer,
            });
        }

        // --- 5. Arbitrary host-path layers ---
        let mut copy_layers: Vec<(&OciCopy, LayerBlob)> = Vec::new();
        for copy in copies {
            copy.validate().map_err(eyre::Report::msg)?;
            warn!(
                "mise oci build: copying host path {} into the image at {} — review its \
                 contents for secrets or credentials before sharing the image",
                copy.host.display(),
                copy.image
            );
            let blob = layer::build_layer_from_path(&copy.host, &copy.image, owner).wrap_err_with(
                || {
                    format!(
                        "copying host path {} to {}",
                        copy.host.display(),
                        copy.image
                    )
                },
            )?;
            copy_layers.push((copy, blob));
        }

        // --- 6. mise binary layer (optional) ---
        let mut mise_layer: Option<LayerBlob> = None;
        if self.opts.include_mise {
            // OCI images are linux-targeted in v1 (we normalize `os` to
            // "linux" above). Embedding a darwin/windows mise binary would
            // pass the build but explode with `Exec format error` the first
            // time anything inside the container invokes `mise`. Warn loudly.
            if std::env::consts::OS != "linux" {
                warn!(
                    "embedding a {} mise binary in a linux OCI image — it will fail at runtime. \
                     Run `mise oci build` on linux, or pass --no-mise to skip embedding.",
                    std::env::consts::OS
                );
            }
            match std::env::current_exe() {
                Ok(exe) => {
                    let bytes = std::fs::read(&exe)
                        .wrap_err_with(|| format!("reading mise binary at {}", exe.display()))?;
                    let files = vec![("usr/local/bin/mise".to_string(), bytes, 0o755u32)];
                    mise_layer = Some(layer::build_layer_from_files(&files, owner)?);
                }
                Err(e) => {
                    warn!("could not locate mise binary to embed in image: {e}");
                }
            }
        }

        // --- 5. Dotfiles layer (optional) ---
        let dotfiles_layer = if self.dotfiles.is_empty() {
            None
        } else {
            Some(build_dotfiles_layer(&self.cfg, &self.dotfiles, owner)?)
        };

        // --- 6. Config layer: /etc/mise/config.toml ---
        let config_layer = {
            let config_toml = synthesize_embedded_config_toml(&versions, &mount_point);
            let files = vec![(
                "etc/mise/config.toml".to_string(),
                config_toml.into_bytes(),
                0o644u32,
            )];
            layer::build_layer_from_files(&files, owner)?
        };

        // --- 7. Write all layer blobs into the layout ---
        let mut tool_layer_infos = Vec::new();
        let mut manifest_layers: Vec<Descriptor> = base_layers.clone();
        let mut all_diff_ids: Vec<String> = base_diff_ids.clone();

        if let Some(m) = &mise_layer {
            layout.write_blob_with_digest(&m.digest, &m.bytes)?;
            manifest_layers.push(Descriptor {
                media_type: manifest::MEDIA_TYPE_OCI_LAYER_GZIP.to_string(),
                size: m.size,
                digest: m.digest.clone(),
                annotations: Default::default(),
                platform: None,
            });
            all_diff_ids.push(m.diff_id.clone());
        }

        if let Some(blob) = &system_packages_layer {
            layout.write_blob_with_digest(&blob.blob.digest, &blob.blob.bytes)?;
            let mut annotations = IndexMap::new();
            annotations.insert(
                "dev.mise.system.packages".to_string(),
                blob.manager.to_string(),
            );
            manifest_layers.push(Descriptor {
                media_type: manifest::MEDIA_TYPE_OCI_LAYER_GZIP.to_string(),
                size: blob.blob.size,
                digest: blob.blob.digest.clone(),
                annotations,
                platform: None,
            });
            all_diff_ids.push(blob.blob.diff_id.clone());
        }

        for (name, blob) in &plugins.layers {
            layout.write_blob_with_digest(&blob.digest, &blob.bytes)?;
            let mut annotations = IndexMap::new();
            annotations.insert(ANNOTATION_PLUGIN.to_string(), name.clone());
            manifest_layers.push(Descriptor {
                media_type: manifest::MEDIA_TYPE_OCI_LAYER_GZIP.to_string(),
                size: blob.size,
                digest: blob.digest.clone(),
                annotations,
                platform: None,
            });
            all_diff_ids.push(blob.diff_id.clone());
        }

        for entry in &tool_layers {
            let mut annotations = IndexMap::new();
            annotations.insert(ANNOTATION_TOOL_SHORT.to_string(), entry.short.clone());
            annotations.insert(ANNOTATION_TOOL_VERSION.to_string(), entry.version.clone());
            annotations.insert(ANNOTATION_LAYER_PREFIX.to_string(), entry.prefix.clone());
            annotations.insert(ANNOTATION_LAYER_OWNER.to_string(), owner_str.clone());
            annotations.insert(
                ANNOTATION_LAYER_RELOCATION.to_string(),
                entry.relocation.clone(),
            );
            if !entry.plugin.is_empty() {
                annotations.insert(ANNOTATION_TOOL_PLUGIN.to_string(), entry.plugin.clone());
            }
            let (media_type, digest, size, diff_id, reused) = match &entry.layer {
                ToolLayer::Built(blob) => {
                    layout.write_blob_with_digest(&blob.digest, &blob.bytes)?;
                    (
                        manifest::MEDIA_TYPE_OCI_LAYER_GZIP.to_string(),
                        blob.digest.clone(),
                        blob.size,
                        blob.diff_id.clone(),
                        false,
                    )
                }
                // Reused layers reference the remote blob; no local bytes
                // exist, which is fine because the push destination (where
                // the cache image lives) already has them.
                ToolLayer::Reused(r) => (
                    r.media_type.clone(),
                    r.digest.clone(),
                    r.size,
                    r.diff_id.clone(),
                    true,
                ),
            };
            manifest_layers.push(Descriptor {
                media_type,
                size,
                digest: digest.clone(),
                annotations,
                platform: None,
            });
            all_diff_ids.push(diff_id);
            tool_layer_infos.push(ToolLayerInfo {
                short: entry.short.clone(),
                version: entry.version.clone(),
                digest,
                size,
                reused,
            });
        }

        for (copy, blob) in &copy_layers {
            layout.write_blob_with_digest(&blob.digest, &blob.bytes)?;
            let mut annotations = IndexMap::new();
            annotations.insert("dev.mise.copy".to_string(), copy.image.clone());
            manifest_layers.push(Descriptor {
                media_type: manifest::MEDIA_TYPE_OCI_LAYER_GZIP.to_string(),
                size: blob.size,
                digest: blob.digest.clone(),
                annotations,
                platform: None,
            });
            all_diff_ids.push(blob.diff_id.clone());
        }

        if let Some(blob) = &dotfiles_layer {
            layout.write_blob_with_digest(&blob.digest, &blob.bytes)?;
            let mut annotations = IndexMap::new();
            annotations.insert("dev.mise.dotfiles".to_string(), "true".to_string());
            manifest_layers.push(Descriptor {
                media_type: manifest::MEDIA_TYPE_OCI_LAYER_GZIP.to_string(),
                size: blob.size,
                digest: blob.digest.clone(),
                annotations,
                platform: None,
            });
            all_diff_ids.push(blob.diff_id.clone());
        }

        {
            layout.write_blob_with_digest(&config_layer.digest, &config_layer.bytes)?;
            manifest_layers.push(Descriptor {
                media_type: manifest::MEDIA_TYPE_OCI_LAYER_GZIP.to_string(),
                size: config_layer.size,
                digest: config_layer.digest.clone(),
                annotations: Default::default(),
                platform: None,
            });
            all_diff_ids.push(config_layer.diff_id.clone());
        }

        // --- 8. Image config ---
        let image_config = self
            .build_image_config(
                &versions,
                &tool_reuse,
                &mount_point,
                base_config_json.as_ref(),
                all_diff_ids.clone(),
                &platform,
            )
            .await?;

        let config_bytes = serde_json::to_vec(&image_config)?;
        let (config_digest, config_size) = layout.write_blob(&config_bytes)?;

        let config_descriptor = Descriptor {
            media_type: manifest::MEDIA_TYPE_OCI_CONFIG.to_string(),
            size: config_size,
            digest: config_digest.clone(),
            annotations: Default::default(),
            platform: None,
        };

        // --- 9. Manifest ---
        // Record the base image (standard annotation) so `mise oci push` can
        // attempt cross-repository blob mounts when the base lives on the
        // destination registry.
        let mut manifest_annotations: IndexMap<String, String> = Default::default();
        if let Some(ref_) = &from_ref {
            manifest_annotations.insert(
                crate::oci::registry::ANNOTATION_BASE_NAME.to_string(),
                ref_.clone(),
            );
        }
        let image_manifest = ImageManifest {
            schema_version: 2,
            media_type: manifest::MEDIA_TYPE_OCI_MANIFEST.to_string(),
            config: config_descriptor,
            layers: manifest_layers,
            annotations: manifest_annotations,
        };
        let (manifest_digest, manifest_size) = layout.write_manifest(&image_manifest)?;

        // --- 10. index.json ---
        let tag = self.opts.tag.clone().or_else(|| self.oci.tag.clone());
        layout.write_index(&manifest_digest, manifest_size, platform, tag.as_deref())?;

        Ok(BuildOutput {
            out_dir: self.opts.out_dir.clone(),
            manifest_digest,
            tool_layers: tool_layer_infos,
        })
    }

    async fn build_image_config(
        &self,
        versions: &[(Arc<dyn crate::backend::Backend>, ToolVersion)],
        tool_reuse: &[Option<ReusedLayer>],
        mount_point: &str,
        base_config_json: Option<&serde_json::Value>,
        diff_ids: Vec<String>,
        platform: &Option<Platform>,
    ) -> Result<ImageConfig> {
        use crate::oci::manifest::Config as ImgConfig;

        // Inherit from base config where possible.
        let mut env_pairs: IndexMap<String, String> = IndexMap::new();
        let mut cmd: Option<Vec<String>> = None;
        let mut entrypoint: Option<Vec<String>> = None;
        let mut working_dir: Option<String> = None;
        let mut user: Option<String> = None;

        if let Some(base) = base_config_json
            && let Some(bc) = base.get("config")
        {
            if let Some(env) = bc.get("Env").and_then(|e| e.as_array()) {
                for e in env {
                    if let Some(s) = e.as_str()
                        && let Some((k, v)) = s.split_once('=')
                    {
                        env_pairs.insert(k.to_string(), v.to_string());
                    }
                }
            }
            if let Some(c) = bc.get("Cmd").and_then(|c| c.as_array()) {
                cmd = Some(
                    c.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect(),
                );
            }
            if let Some(e) = bc.get("Entrypoint").and_then(|e| e.as_array()) {
                entrypoint = Some(
                    e.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect(),
                );
            }
            if let Some(wd) = bc.get("WorkingDir").and_then(|w| w.as_str())
                && !wd.is_empty()
            {
                working_dir = Some(wd.to_string());
            }
            if let Some(u) = bc.get("User").and_then(|u| u.as_str())
                && !u.is_empty()
            {
                user = Some(u.to_string());
            }
        }

        // User env from mise.toml (best-effort: use the already-merged config.env).
        // NOTE: we don't re-resolve templates here — they were resolved at load time.
        //
        // WARNING: values sourced from `.env` files or `_.file = "..."` can
        // include secrets (DATABASE_URL, AWS_SECRET_ACCESS_KEY, etc.). Baking
        // them into the image config makes them visible to anyone who does
        // `skopeo inspect` / `docker inspect`. Surface that loudly so users
        // aren't surprised.
        let env = self
            .cfg
            .env()
            .await
            .wrap_err("resolving [env] for oci build (template error, missing file, etc.)")?;
        if !env.is_empty() {
            warn!(
                "mise oci build: baking {} [env] var(s) into the image config. \
                 These are visible via `docker inspect` / `skopeo inspect`; \
                 if you have secrets in [env] or referenced .env files, move \
                 them to runtime (e.g. `docker run -e` or secret mounts) and \
                 use the [oci].env section for image-only vars.",
                env.len()
            );
            for (k, v) in env {
                env_pairs.insert(k, v);
            }
        }

        // Per-tool exec_env (JAVA_HOME, GOROOT, GEM_HOME, etc.). Paths in
        // these values point at the host install dir; rebase them to the
        // in-image location so they're valid inside the container.
        for (backend, tv) in versions {
            let host_install = tv.install_path();
            let in_image_root = tool_in_image_path(mount_point, tv);
            let from_plugin = matches!(
                backend.get_type(),
                BackendType::Vfox | BackendType::VfoxBackend(_)
            );
            match backend.exec_env(&self.cfg, &self.ts, tv).await {
                Ok(tool_env) => {
                    let mut host_paths = vec![];
                    for (k, v) in tool_env {
                        let rebased = rebase_path_value(&v, &host_install, &in_image_root);
                        if from_plugin && refers_to_host_home(&rebased) {
                            host_paths.push(k.clone());
                        }
                        env_pairs.insert(k, rebased);
                    }
                    // Plugin env hooks run on the build host. A value under
                    // the host's home (e.g. `GOPATH=$HOME/go`) is baked into
                    // the image verbatim and won't exist in the container.
                    if !host_paths.is_empty() {
                        warn!(
                            "mise oci build: {} sets {} to a path under the build host's home \
                             directory; the path is baked into the image as-is and likely \
                             doesn't exist in the container. Override it with [oci].env.",
                            tv.style(),
                            host_paths.join(", ")
                        );
                    }
                }
                Err(e) => {
                    warn!(
                        "failed to resolve exec_env for {}: {e} — \
                         any vars that tool needs (e.g. JAVA_HOME) will be missing",
                        tv.style()
                    );
                }
            }
        }

        // Extra env from [oci].env section (explicit image-only vars).
        for (k, v) in &self.oci.env {
            env_pairs.insert(k.clone(), v.clone());
        }

        // Mise data/config dirs — insert LAST so the user's [env] section
        // can't accidentally shadow them (the embedded mise binary inside
        // the container must see these in-image paths, not whatever was
        // baked in from the host config).
        env_pairs.insert("MISE_DATA_DIR".to_string(), mount_point.to_string());
        env_pairs.insert("MISE_CONFIG_DIR".to_string(), "/etc/mise".to_string());

        // PATH: prepend each tool's real bin paths (from `list_bin_paths`),
        // rebased from the host install path to the in-image location. This
        // handles backends that expose paths other than `<install>/bin`
        // (e.g. sbin, libexec, or the install root itself). Falls back to
        // `<install>/bin` if a backend returns nothing or paths outside its
        // install dir.
        let mut path_entries: Vec<String> = Vec::new();
        for (i, (backend, tv)) in versions.iter().enumerate() {
            let install_path = crate::file::canonicalize_or_self(&tv.install_path());
            let in_image_tool_root = tool_in_image_path(mount_point, tv);
            if tool_reuse[i].is_some() {
                let cached_entries = self
                    .opts
                    .reuse_from
                    .as_ref()
                    .map(|remote| cached_tool_path_entries(remote, &in_image_tool_root))
                    .unwrap_or_default();
                if !cached_entries.is_empty() {
                    path_entries.extend(cached_entries);
                    continue;
                }
            }
            let bin_paths = backend
                .list_bin_paths(&self.cfg, tv)
                .await
                .unwrap_or_default();
            let mut had_one = false;
            for p in bin_paths {
                let p = crate::file::canonicalize_or_self(&p);
                if let Ok(rel) = p.strip_prefix(&install_path) {
                    let rel = rel.to_string_lossy();
                    let entry = if rel.is_empty() {
                        in_image_tool_root.clone()
                    } else {
                        format!("{in_image_tool_root}/{rel}")
                    };
                    path_entries.push(entry);
                    had_one = true;
                }
            }
            if !had_one {
                path_entries.push(format!("{in_image_tool_root}/bin"));
            }
        }
        let inherited_path = env_pairs.get("PATH").cloned().unwrap_or_else(|| {
            "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin".to_string()
        });
        let final_path = if path_entries.is_empty() {
            inherited_path
        } else {
            format!("{}:{}", path_entries.join(":"), inherited_path)
        };
        env_pairs.insert("PATH".to_string(), final_path);

        // Apply CLI-level overrides from [oci].
        if let Some(wd) = &self.oci.workdir {
            working_dir = Some(wd.clone());
        }
        if let Some(ep) = &self.oci.entrypoint {
            entrypoint = Some(ep.clone());
        }
        if let Some(c) = &self.oci.cmd {
            cmd = Some(c.clone());
        }
        if let Some(u) = &self.oci.user {
            user = Some(u.clone());
        }
        if working_dir.is_none() {
            working_dir = Some("/workspace".to_string());
        }

        // Capture the build timestamp once so the label and the image config
        // `created` field can never disagree.
        let created = rfc3339_now();

        // Labels.
        let mut labels: IndexMap<String, String> = IndexMap::new();
        labels.insert(
            "org.opencontainers.image.created".to_string(),
            created.clone(),
        );
        labels.insert(
            "org.opencontainers.image.source".to_string(),
            "mise oci build".to_string(),
        );
        labels.insert(
            "dev.mise.version".to_string(),
            crate::version::VERSION_PLAIN.to_string(),
        );
        for (_, tv) in versions {
            labels.insert(
                format!("dev.mise.tools.{}", sanitize_label(&tv.ba().short)),
                tv.version.clone(),
            );
        }
        for (k, v) in &self.oci.labels {
            labels.insert(k.clone(), v.clone());
        }

        let config = ImgConfig {
            env: env_pairs.iter().map(|(k, v)| format!("{k}={v}")).collect(),
            cmd,
            entrypoint,
            working_dir,
            user,
            labels,
            exposed_ports: Default::default(),
            volumes: Default::default(),
            stop_signal: None,
        };

        let (arch, os) = if let Some(p) = platform {
            (p.architecture.clone(), p.os.clone())
        } else {
            (
                crate::oci::normalize_arch(std::env::consts::ARCH).to_string(),
                crate::oci::normalize_os(std::env::consts::OS).to_string(),
            )
        };

        Ok(ImageConfig {
            created: Some(created),
            author: Some("mise".to_string()),
            architecture: arch,
            os,
            variant: None,
            config: Some(config),
            rootfs: RootFs {
                type_: "layers".to_string(),
                diff_ids,
            },
            history: vec![],
        })
    }
}

/// Coordinate source checks and packaging with installation transactions.
fn lock_tool_install(tv: &ToolVersion) -> Result<fslock::LockFile> {
    crate::toolset::install_state::lock_tool_version_with_notice(
        &tv.ba().short,
        &tv.tv_pathname(),
        &|pid| {
            info!(
                "oci: {} {}",
                tv.style(),
                crate::backend::install_lock_wait_message(pid)
            )
        },
    )
}

/// Check cheaply when present, but wait for a reinstall before reporting absence.
fn preflight_tool_install(tv: &ToolVersion) -> Result<()> {
    if !tv.install_path().is_dir() {
        let _install_lock = lock_tool_install(tv)?;
        require_tool_install(tv)?;
    }
    Ok(())
}

fn require_tool_install(tv: &ToolVersion) -> Result<()> {
    let install_path = tv.install_path();
    if !install_path.is_dir() {
        bail!(
            "{} install path does not exist: {}. Run `mise install` first.",
            tv.style(),
            install_path.display()
        );
    }
    Ok(())
}

fn resolve_layer_owner(opts_owner: Option<LayerOwner>, oci: &OciConfig) -> LayerOwner {
    opts_owner.unwrap_or_else(|| {
        let uid = oci.user_id.unwrap_or(0);
        let gid = oci.group_id.unwrap_or(uid);
        LayerOwner::new(uid, gid)
    })
}

fn build_dotfiles_layer(
    cfg: &Config,
    requests: &[FileRequest],
    owner: LayerOwner,
) -> Result<LayerBlob> {
    let mut entries = DotfilesLayerEntries::default();

    for req in requests {
        if req.mode.has_source() && req.mode != FileMode::Track && !req.source.exists() {
            bail!(
                "[dotfiles].\"{}\": source does not exist: {}",
                req.target_raw,
                req.source.display()
            );
        }

        if req.dot_prefix {
            add_source_files(req, &oci_target_path(req)?, &mut entries)?;
            continue;
        }

        match req.mode {
            // a tracked file lives on the machine that tracks it, and a
            // permissions-only entry adjusts a file the image does not
            // provide; an image has nothing to copy for either
            FileMode::Track | FileMode::Permissions => continue,
            // an absent target is not added, and a whiteout hides one a
            // base layer may already hold there
            FileMode::Absent => entries.add_whiteout(&oci_target_path(req)?)?,
            // footprint validation rejects `permissions` on a directory copy
            FileMode::Symlink | FileMode::Copy => match req.permissions {
                Some(permissions) => {
                    entries.add_file(
                        oci_target_path(req)?,
                        file::read(&req.source)?,
                        permissions,
                    )?;
                }
                // apply copies a directory file by file, so the same
                // filtered walk decides what the image gets
                None if req.mode == FileMode::Copy && req.source.is_dir() => {
                    add_source_files(req, &oci_target_path(req)?, &mut entries)?;
                }
                None => {
                    collect_source_as_files(&req.source, &oci_target_path(req)?, &mut entries)
                        .wrap_err_with(|| {
                            format!("adding [dotfiles].\"{}\" to OCI image", req.target_raw)
                        })?;
                }
            },
            FileMode::SymlinkEach => {
                if !req.source.is_dir() {
                    bail!(
                        "[dotfiles].\"{}\": mode symlink-each requires a directory source: {}",
                        req.target_raw,
                        req.source.display()
                    );
                }
                add_source_files(req, &oci_target_path(req)?, &mut entries)?;
            }
            FileMode::Template => {
                let rendered = crate::system::files::render_template_for_oci(cfg, req)?;
                // an empty render with remove_empty declares no file at all;
                // a whiteout hides one a base layer may already hold there
                if crate::system::files::removes_target(req, Some(&rendered)) {
                    entries.add_whiteout(&oci_target_path(req)?)?;
                    continue;
                }
                entries.add_file(
                    oci_target_path(req)?,
                    rendered.into_bytes(),
                    match req.permissions {
                        Some(permissions) => permissions,
                        None => source_mode(&req.source)?,
                    },
                )?;
            }
            FileMode::Content => {
                entries.add_file(
                    oci_target_path(req)?,
                    req.content
                        .as_deref()
                        .expect("inline content")
                        .as_bytes()
                        .to_vec(),
                    req.permissions.unwrap_or(0o600),
                )?;
            }
        }
    }

    info!("oci: adding {} [dotfiles] entries", requests.len());
    let (files, dirs) = entries.into_layer_inputs();
    layer::build_layer_from_files_and_dirs(&files, &dirs, owner)
}

/// A directory-walking entry (`symlink-each`, a directory `copy`, or any
/// `dot_prefix` entry) goes through the same filtered walk apply uses, so
/// `exclude` and `manifest` decide which files reach the image and, with
/// `dot_prefix`, two sources never claim one path.
fn add_source_files(
    req: &FileRequest,
    target: &str,
    entries: &mut DotfilesLayerEntries,
) -> Result<()> {
    entries.add_dir(target.to_string())?;
    for (source, deployed) in crate::system::files::directory_source_files(req)? {
        // a FIFO or socket would block or fail the read, and so would a link
        // to one; a dangling link or a link to a directory still fails the
        // read below, so a declared dotfile is never silently left out
        if std::fs::metadata(&source).is_ok_and(|meta| !meta.is_file() && !meta.is_dir()) {
            warn!(
                "oci: skipping non-file [dotfiles] source entry {}",
                source.display()
            );
            continue;
        }
        let rel = deployed.strip_prefix(&req.target)?;
        for dir in rel.ancestors().skip(1) {
            if !dir.as_os_str().is_empty() {
                entries.add_dir(oci_join(target, dir))?;
            }
        }
        entries.add_file(
            oci_join(target, rel),
            file::read(&source)?,
            source_mode(&source)?,
        )?;
    }
    Ok(())
}

fn oci_join(target: &str, rel: &std::path::Path) -> String {
    format!("{target}/{}", rel.to_string_lossy().replace('\\', "/"))
}

fn collect_source_as_files(
    source: &std::path::Path,
    target: &str,
    entries: &mut DotfilesLayerEntries,
) -> Result<()> {
    if source.is_dir() {
        entries.add_dir(target.to_string())?;
        for entry in walkdir::WalkDir::new(source).sort_by_file_name() {
            let entry = entry?;
            if entry.file_type().is_dir() {
                let rel = entry.path().strip_prefix(source)?;
                if !rel.as_os_str().is_empty() {
                    entries.add_dir(format!(
                        "{target}/{}",
                        rel.to_string_lossy().replace('\\', "/")
                    ))?;
                }
                continue;
            }
            let ft = entry.file_type();
            if !(ft.is_file() || ft.is_symlink()) {
                warn!(
                    "oci: skipping non-file [dotfiles] source entry {}",
                    entry.path().display()
                );
                continue;
            }
            let rel = entry.path().strip_prefix(source)?;
            let path = format!("{target}/{}", rel.to_string_lossy().replace('\\', "/"));
            entries.add_file(path, file::read(entry.path())?, source_mode(entry.path())?)?;
        }
    } else {
        entries.add_file(
            target.to_string(),
            file::read(source)?,
            source_mode(source)?,
        )?;
    }
    Ok(())
}

#[derive(Default)]
struct DotfilesLayerEntries {
    files: IndexMap<String, (Vec<u8>, u32)>,
    dirs: IndexSet<String>,
}

type DotfilesLayerFile = (String, Vec<u8>, u32);
type DotfilesLayerFiles = Vec<DotfilesLayerFile>;
type DotfilesLayerDirs = Vec<String>;

impl DotfilesLayerEntries {
    fn add_file(&mut self, path: String, contents: Vec<u8>, mode: u32) -> Result<()> {
        if self.dirs.contains(&path) {
            bail!("[dotfiles]: duplicate OCI path {path:?} as both file and directory");
        }
        if let Some((existing_contents, existing_mode)) = self.files.get(&path) {
            if existing_contents != &contents || *existing_mode != mode {
                bail!("[dotfiles]: duplicate OCI file path {path:?}");
            }
            return Ok(());
        }
        self.files.insert(path, (contents, mode));
        Ok(())
    }

    fn add_dir(&mut self, path: String) -> Result<()> {
        if self.files.contains_key(&path) {
            bail!("[dotfiles]: duplicate OCI path {path:?} as both file and directory");
        }
        self.dirs.insert(path);
        Ok(())
    }

    /// Hide `path` from the layers below, per the OCI image spec: an empty
    /// `.wh.<name>` file beside it. A path the base never had is unaffected.
    fn add_whiteout(&mut self, path: &str) -> Result<()> {
        let whiteout = match path.rsplit_once('/') {
            Some((parent, name)) => format!("{parent}/.wh.{name}"),
            None => format!(".wh.{path}"),
        };
        self.add_file(whiteout, vec![], 0o644)
    }

    fn into_layer_inputs(self) -> (DotfilesLayerFiles, DotfilesLayerDirs) {
        let files = self
            .files
            .into_iter()
            .map(|(path, (contents, mode))| (path, contents, mode))
            .collect();
        let dirs = self.dirs.into_iter().collect();
        (files, dirs)
    }
}

fn oci_target_path(req: &FileRequest) -> Result<String> {
    let raw = req.target_raw.as_str();
    let path = if raw == "~" {
        "root".to_string()
    } else if let Some(rest) = raw.strip_prefix("~/") {
        format!("root/{rest}")
    } else {
        req.target
            .strip_prefix("/")
            .map_err(|_| eyre::eyre!("dotfile target must be absolute: {}", req.target_raw))?
            .to_string_lossy()
            .replace('\\', "/")
    };
    if path.is_empty() || path.split('/').any(|p| p == "..") {
        bail!(
            "[dotfiles].\"{}\": target is not a safe OCI path",
            req.target_raw
        );
    }
    Ok(path)
}

fn source_mode(path: &std::path::Path) -> Result<u32> {
    let md = path.metadata()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        Ok(md.permissions().mode() & 0o7777)
    }
    #[cfg(not(unix))]
    {
        let _ = md;
        Ok(0o644)
    }
}

fn reject_unsupported_backends(
    versions: &[(Arc<dyn crate::backend::Backend>, ToolVersion)],
) -> Result<()> {
    // Ask the actual backend instance rather than parsing the short name:
    // `BackendType::guess` only matches a literal "asdf" prefix and misses
    // plugin-named tools that resolve to asdf.
    //
    // vfox plugins are accepted: mise extracts their downloads into the
    // per-version directory, and their env hooks run on the host at build
    // time. asdf plugins stay rejected — their bash install scripts commonly
    // write outside the install dir, and their `exec-env` scripts expect
    // bash at runtime.
    let bad: Vec<String> = versions
        .iter()
        .filter(|(backend, _)| backend.get_type() == BackendType::Asdf)
        .map(|(_, tv)| tv.ba().short.clone())
        .collect();
    if !bad.is_empty() {
        bail!(
            "mise oci build does not support asdf plugins (their install scripts can \
             write outside the per-version directory, breaking the one-layer-per-tool invariant). \
             Use a vfox plugin or another backend for: {}",
            bad.join(", ")
        );
    }
    Ok(())
}

/// Plugin layers for an image, plus each tool's plugin fingerprint.
struct PluginLayers {
    /// One layer per distinct on-disk plugin, keyed by its directory name
    /// under `<mount>/plugins/`.
    layers: IndexMap<String, LayerBlob>,
    /// Parallel to the builder's `versions`: the reuse-key fingerprint of the
    /// plugin that installed each tool, or empty when no plugin did.
    tool_fingerprints: Vec<String>,
}

/// Package the vfox plugins that installed the toolset's tools.
///
/// The embedded mise resolves these tools through `<mount>/plugins/<name>`
/// just as it does on the host. Without the plugin there, any mise command
/// inside the container would try to clone it — and a plugin declared by URL
/// in a project `[plugins]` section couldn't be found at all, since that
/// section isn't carried into the image config.
///
/// Plugins embedded in the mise binary have no directory to copy; the
/// embedded mise carries them, and their fingerprint hashes their sources.
async fn build_plugin_layers(
    config: &Arc<Config>,
    versions: &[(Arc<dyn crate::backend::Backend>, ToolVersion)],
    mount_point: &str,
    owner: LayerOwner,
) -> Result<PluginLayers> {
    use crate::plugins::{Plugin, PluginEnum};
    use crate::ui::multi_progress_report::MultiProgressReport;

    let mut layers: IndexMap<String, LayerBlob> = IndexMap::new();
    let mut tool_fingerprints = Vec::with_capacity(versions.len());
    for (backend, tv) in versions {
        let plugin = match backend.plugin() {
            Some(PluginEnum::Vfox(p) | PluginEnum::VfoxBackend(p)) => p,
            _ => {
                tool_fingerprints.push(String::new());
                continue;
            }
        };
        // The image env is derived by running the plugin's hooks, so it must
        // be present even when the tool layer is reused from a registry.
        plugin
            .ensure_installed(config, &MultiProgressReport::get(), false, false)
            .await
            .wrap_err_with(|| format!("installing plugin {} for {}", plugin.name, tv.style()))?;
        if !plugin.plugin_path.exists() {
            if let Some(fingerprint) = embedded_plugin_fingerprint(&plugin.name) {
                tool_fingerprints.push(fingerprint);
                continue;
            }
            bail!(
                "plugin {} for {} is not installed at {}",
                plugin.name,
                tv.style(),
                plugin.plugin_path.display()
            );
        }
        let name = plugin
            .plugin_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| plugin.name.clone());
        if !layers.contains_key(&name) {
            // Plugins linked from a local directory or a git subdirectory are
            // symlinks; package what they point at.
            let src = std::fs::canonicalize(&plugin.plugin_path).wrap_err_with(|| {
                format!("resolving plugin path {}", plugin.plugin_path.display())
            })?;
            // Everything in the plugin directory except `.git` ships, including
            // untracked local files; name the source so it can be reviewed.
            info!(
                "oci: copying plugin {name} from {} into the image (all files except .git)",
                crate::file::display_path(&src)
            );
            let prefix = format!("{}/plugins/{name}", mount_point.trim_start_matches('/'));
            let blob = layer::build_plugin_layer_from_dir(&src, &prefix, owner)
                .wrap_err_with(|| format!("building layer for plugin {name}"))?;
            layers.insert(name.clone(), blob);
        }
        tool_fingerprints.push(layers[&name].diff_id.clone());
    }
    Ok(PluginLayers {
        layers,
        tool_fingerprints,
    })
}

/// Rewrite any occurrence of the host install path in an `exec_env` value to
/// the corresponding in-image path. Handles both exact matches
/// (`JAVA_HOME=<install>`) and colon-separated PATH-like values
/// (`SOMETHING=<install>/foo:<install>/bar`).
fn rebase_path_value(value: &str, host_prefix: &std::path::Path, in_image_prefix: &str) -> String {
    let host: &str = &host_prefix.to_string_lossy();
    if host.is_empty() || !value.contains(host) {
        return value.to_string();
    }
    value.replace(host, in_image_prefix)
}

/// Content hash of a plugin compiled into the mise binary, over its metadata,
/// hook, and library sources. `None` when no such plugin is embedded.
fn embedded_plugin_fingerprint(name: &str) -> Option<String> {
    let plugin = vfox::embedded_plugins::get_embedded_plugin(name)?;
    let mut hasher = Sha256::new();
    // Length-prefix every field so adjacent fields can't run together.
    let mut field = |bytes: &[u8]| {
        hasher.update((bytes.len() as u64).to_le_bytes());
        hasher.update(bytes);
    };
    field(plugin.metadata.as_bytes());
    for (kind, files) in [("hooks", plugin.hooks), ("lib", plugin.lib)] {
        field(kind.as_bytes());
        field(&(files.len() as u64).to_le_bytes());
        for (path, source) in files {
            field(path.as_bytes());
            field(source.as_bytes());
        }
    }
    Some(format!(
        "embedded:sha256:{}",
        layer::hex_encode(&hasher.finalize())
    ))
}

/// Whether an env value names a path under the build host's home directory.
fn refers_to_host_home(value: &str) -> bool {
    value_refers_to_dir(value, &crate::dirs::HOME)
}

fn value_refers_to_dir(value: &str, dir: &std::path::Path) -> bool {
    let dir = dir.to_string_lossy();
    let dir = dir.trim_end_matches('/');
    // An empty or root home would match every absolute path.
    if dir.is_empty() {
        return false;
    }
    std::env::split_paths(value).any(|p| {
        p.to_str()
            .and_then(|p| p.strip_prefix(dir))
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
    })
}

/// In-image absolute path for a tool's install dir, e.g.
/// `/mise/installs/node/20.0.0`. Used when we need a path that mise (or the
/// image config's PATH / env vars) can reference at runtime.
///
/// Uses the canonical directory names that mise itself uses on the host
/// (via `BackendArg::tool_dir_name` / `ToolVersion::tv_pathname`). A naive
/// `short.replace([':', '/'], "-")` would diverge — the real path name goes
/// through `to_kebab_case()` which also strips non-alphanumerics and splits
/// camelCase boundaries. Mismatching would mean files land at a path mise
/// can't resolve inside the container.
fn tool_in_image_path(mount_point: &str, tv: &ToolVersion) -> String {
    let plugin_dir = tv.ba().tool_dir_name();
    let version_dir = tv.tv_pathname();
    // `mount_point` is guaranteed absolute and trimmed of trailing slashes
    // by the validation in Builder::build (empty + must-start-with-/
    // checks), so a single `/` between segments is always correct.
    format!("{mount_point}/installs/{plugin_dir}/{version_dir}")
}

/// Same as `tool_in_image_path` but stripped of its leading `/` for use as
/// a tar entry name (tar paths are conventionally relative).
fn tool_tar_prefix(mount_point: &str, tv: &ToolVersion) -> String {
    tool_in_image_path(mount_point, tv)
        .trim_start_matches('/')
        .to_string()
}

fn tool_layer_relocation_key(tv: &ToolVersion, pythons: &[PythonRelocation]) -> String {
    if tv.ba().backend_type() == BackendType::Pipx {
        format!(
            "{TOOL_LAYER_RELOCATION_VERSION}:python={}",
            python_relocation_fingerprint(pythons)
        )
    } else {
        TOOL_LAYER_RELOCATION_VERSION.to_string()
    }
}

fn is_python_backend(backend: &dyn crate::backend::Backend) -> bool {
    backend.get_type() == BackendType::Core && backend.tool_name() == "python"
}

fn python_relocation_fingerprint(pythons: &[PythonRelocation]) -> String {
    let mut pythons = pythons.to_vec();
    pythons.sort_by(|a, b| (&a.version, &a.host, &a.image).cmp(&(&b.version, &b.host, &b.image)));
    let mut hash = Sha256::new();
    for python in pythons {
        for input in [
            python.version,
            python.host.to_string_lossy().into_owned(),
            python.image.to_string_lossy().into_owned(),
        ] {
            hash.update((input.len() as u64).to_le_bytes());
            hash.update(input.as_bytes());
        }
    }
    layer::hex_encode(&hash.finalize())
}

fn synthesize_embedded_config_toml(
    versions: &[(Arc<dyn crate::backend::Backend>, ToolVersion)],
    _mount_point: &str,
) -> String {
    let mut s = String::from("# Auto-generated by `mise oci build`. Do not edit.\n[tools]\n");
    for (_, tv) in versions {
        // Tool short names and versions can contain `"` and `\` (rare, but
        // possible for e.g. ref/branch specifiers), so serialize via the
        // TOML library rather than string interpolation.
        let mut tbl = toml::value::Table::new();
        tbl.insert(
            tv.ba().short.clone(),
            toml::Value::String(tv.version.clone()),
        );
        let rendered = toml::to_string(&tbl).unwrap_or_default();
        s.push_str(&rendered);
    }
    s
}

fn rfc3339_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    // Honor SOURCE_DATE_EPOCH for reproducible builds. If it's set but
    // unparseable, warn and fall back to the real clock rather than silently
    // using epoch zero — which would be indistinguishable from an intentional
    // `SOURCE_DATE_EPOCH=0` and leave the user wondering why the timestamp
    // looks wrong.
    let secs = if let Ok(s) = std::env::var("SOURCE_DATE_EPOCH") {
        match s.parse::<u64>() {
            Ok(n) => n,
            Err(_) => {
                warn!(
                    "ignoring SOURCE_DATE_EPOCH={s:?}: not a non-negative integer. \
                     Using the system clock instead."
                );
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0)
            }
        }
    } else {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    };
    format_rfc3339_utc(secs)
}

fn format_rfc3339_utc(secs: u64) -> String {
    // Minimal, dependency-free RFC3339 formatter. Good enough for image
    // labels. Uses civil calendar math per POSIX.
    let days = (secs / 86_400) as i64;
    let time_of_day = secs % 86_400;
    let h = time_of_day / 3600;
    let m = (time_of_day / 60) % 60;
    let s = time_of_day % 60;
    let (y, mo, d) = days_to_ymd(days);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}Z")
}

fn days_to_ymd(days: i64) -> (i64, u32, u32) {
    // Days since 1970-01-01 → (year, month, day). Civil-from-days algorithm
    // (Howard Hinnant, date.h).
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

fn sanitize_label(s: &str) -> String {
    s.replace([':', '/'], ".")
}

/// Recover the PATH entries that described a reused tool in the cache image.
/// A reused tool may not be installed locally, so asking its backend for bin
/// paths can return nothing even though the remote layer has a non-standard
/// layout such as an executable directly in the install root.
fn cached_tool_path_entries(remote: &registry::RemoteImage, tool_root: &str) -> Vec<String> {
    remote
        .config
        .get("config")
        .and_then(|config| config.get("Env"))
        .and_then(|env| env.as_array())
        .and_then(|env| {
            env.iter()
                .filter_map(|entry| entry.as_str())
                .filter_map(|entry| entry.strip_prefix("PATH="))
                .next_back()
        })
        .map(|path| {
            path.split(':')
                .filter(|entry| {
                    let has_parent = PathBuf::from(entry)
                        .components()
                        .any(|component| component == std::path::Component::ParentDir);
                    !has_parent
                        && (*entry == tool_root
                            || entry
                                .strip_prefix(tool_root)
                                .is_some_and(|suffix| suffix.starts_with('/')))
                })
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_removed_dotfile_is_whited_out_in_the_layer() -> Result<()> {
        let mut entries = DotfilesLayerEntries::default();
        entries.add_file("root/.bashrc".into(), b"kept\n".to_vec(), 0o644)?;
        entries.add_whiteout("root/.config/app/work.toml")?;
        let (files, dirs) = entries.into_layer_inputs();
        let blob = layer::build_layer_from_files_and_dirs(&files, &dirs, LayerOwner::default())?;
        let mut archive =
            jdx_tar::Archive::new(flate2::read::GzDecoder::new(blob.bytes.as_slice()));
        let mut whiteout = None;
        let mut paths = vec![];
        for entry in archive.entries()? {
            let entry = entry?;
            let path = entry.path()?.to_string_lossy().into_owned();
            if path == "root/.config/app/.wh.work.toml" {
                whiteout = Some((entry.entry_type(), entry.size()));
            }
            paths.push(path);
        }
        assert_eq!(whiteout, Some((jdx_tar::EntryType::File, 0)), "{paths:?}");
        assert!(!paths.iter().any(|p| p == "root/.config/app/work.toml"));
        Ok(())
    }

    #[tokio::test]
    async fn directory_entries_use_the_filtered_walk() -> Result<()> {
        let config = Config::get().await?;
        let dir = tempfile::tempdir()?;
        let source = dir.path().join("src");
        file::create_dir_all(source.join("app"))?;
        file::create_dir_all(source.join("cache"))?;
        file::write(source.join("app/config.toml"), "config")?;
        file::write(source.join("bashrc"), "bashrc")?;
        file::write(source.join("debug.log"), "log")?;
        file::write(source.join("cache/blob"), "blob")?;
        let request = |mode, manifest| FileRequest {
            target_raw: "~".into(),
            target: dir.path().join("home"),
            source: source.clone(),
            content: None,
            mode,
            exclude: vec![
                glob::Pattern::new("*.log").unwrap(),
                glob::Pattern::new("cache").unwrap(),
            ],
            include: None,
            manifest,
            permissions: None,
            base: dir.path().to_path_buf(),
            origin: crate::system::resources::ResourceOrigin {
                config: dir.path().join("mise.toml"),
                config_root: dir.path().to_path_buf(),
                environment: vec![],
                source: Some(source.clone()),
            },
            policy: crate::system::files::FilePolicy::for_mode(mode),
            variants: vec![],
            enabled: true,
            remove_empty: false,
            relative: false,
            dot_prefix: false,
        };
        // every path in the layer build_dotfiles_layer produces, so the
        // test covers which walk each mode is routed through
        let layer_paths = |req: FileRequest| -> Result<Vec<String>> {
            let blob = build_dotfiles_layer(&config, &[req], LayerOwner::default())?;
            let mut archive =
                jdx_tar::Archive::new(flate2::read::GzDecoder::new(blob.bytes.as_slice()));
            let mut paths = vec![];
            for entry in archive.entries()? {
                let path = entry?.path()?.to_string_lossy().into_owned();
                paths.push(path.trim_end_matches('/').to_string());
            }
            paths.sort();
            Ok(paths)
        };

        for mode in [FileMode::SymlinkEach, FileMode::Copy] {
            assert_eq!(
                layer_paths(request(mode, None))?,
                ["root", "root/app", "root/app/config.toml", "root/bashrc"],
                "{mode:?}"
            );
        }

        // with a git manifest, a file git does not track stays out too
        let git = |args: &[&str]| -> Result<()> {
            // an inherited GIT_DIR or GIT_INDEX_FILE would point these at
            // another repository
            let mut cmd = std::process::Command::new("git");
            crate::git::sanitize_git_command(&mut cmd);
            let status = cmd
                .arg("-C")
                .arg(&source)
                .args(args)
                .stdout(std::process::Stdio::null())
                .status()?;
            eyre::ensure!(status.success(), "git {args:?} failed");
            Ok(())
        };
        git(&["init", "-q"])?;
        git(&["add", "app/config.toml", "debug.log", "cache/blob"])?;
        for mode in [FileMode::SymlinkEach, FileMode::Copy] {
            assert_eq!(
                layer_paths(request(mode, Some(crate::system::files::FileManifest::Git)))?,
                ["root", "root/app", "root/app/config.toml"],
                "{mode:?}"
            );
        }
        Ok(())
    }

    /// A `dot_prefix` entry for `~` over a source with a nested dotted
    /// directory and an excluded `.bashrc` that would otherwise collide.
    fn dot_prefix_req(dir: &std::path::Path) -> Result<FileRequest> {
        let source = dir.join("src");
        file::create_dir_all(source.join("dot-config/app"))?;
        file::write(source.join("dot-bashrc"), "dotted")?;
        file::write(source.join("dot-config/app/config.toml"), "config")?;
        // excluded, so it neither collides with dot-bashrc nor ships
        file::write(source.join(".bashrc"), "plain")?;
        Ok(FileRequest {
            target_raw: "~".into(),
            target: dir.join("home"),
            source: source.clone(),
            content: None,
            mode: FileMode::SymlinkEach,
            exclude: vec![glob::Pattern::new(".bashrc")?],
            include: None,
            manifest: None,
            permissions: None,
            base: dir.to_path_buf(),
            origin: crate::system::resources::ResourceOrigin {
                config: dir.join("mise.toml"),
                config_root: dir.to_path_buf(),
                environment: vec![],
                source: Some(source.clone()),
            },
            policy: crate::system::files::FilePolicy::for_mode(FileMode::SymlinkEach),
            variants: vec![],
            enabled: true,
            remove_empty: false,
            relative: false,
            dot_prefix: true,
        })
    }

    #[test]
    fn dot_prefix_entries_use_the_filtered_walk() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let mut req = dot_prefix_req(dir.path())?;
        let mut entries = DotfilesLayerEntries::default();
        add_source_files(&req, "root", &mut entries)?;
        let files = entries
            .files
            .iter()
            .map(|(path, (contents, _))| (path.as_str(), contents.as_slice()))
            .collect::<Vec<_>>();
        assert_eq!(
            files,
            vec![
                ("root/.bashrc", b"dotted".as_slice()),
                ("root/.config/app/config.toml", b"config".as_slice()),
            ]
        );
        assert!(entries.dirs.contains("root/.config/app"));

        #[cfg(unix)]
        {
            let source = &req.source;
            // reading a FIFO would wait for a writer forever
            nix::unistd::mkfifo(
                &source.join("dot-pipe"),
                nix::sys::stat::Mode::from_bits_truncate(0o600),
            )?;
            let mut entries = DotfilesLayerEntries::default();
            add_source_files(&req, "root", &mut entries)?;
            assert!(!entries.files.contains_key("root/.pipe"));
            assert!(entries.files.contains_key("root/.bashrc"));

            std::fs::remove_file(source.join("dot-pipe"))?;
            std::os::unix::fs::symlink(source.join("missing"), source.join("dot-dangling"))?;
            assert!(add_source_files(&req, "root", &mut DotfilesLayerEntries::default()).is_err());
            std::fs::remove_file(source.join("dot-dangling"))?;
        }

        req.exclude.clear();
        let err = add_source_files(&req, "root", &mut DotfilesLayerEntries::default())
            .unwrap_err()
            .to_string();
        assert!(err.contains("both deploy to"), "{err}");
        Ok(())
    }

    #[tokio::test]
    async fn dot_prefix_names_reach_the_built_layer() -> Result<()> {
        let config = Config::get().await?;
        let dir = tempfile::tempdir()?;
        let req = dot_prefix_req(dir.path())?;
        let blob = build_dotfiles_layer(&config, &[req], LayerOwner::default())?;
        let mut archive =
            jdx_tar::Archive::new(flate2::read::GzDecoder::new(blob.bytes.as_slice()));
        let mut files = vec![];
        for entry in archive.entries()? {
            let mut entry = entry?;
            if entry.entry_type() == jdx_tar::EntryType::File {
                let path = entry.path()?.to_string_lossy().into_owned();
                let mut contents = String::new();
                std::io::Read::read_to_string(&mut entry, &mut contents)?;
                files.push((path, contents));
            }
        }
        files.sort();
        assert_eq!(
            files,
            vec![
                ("root/.bashrc".to_string(), "dotted".to_string()),
                (
                    "root/.config/app/config.toml".to_string(),
                    "config".to_string()
                ),
            ]
        );
        Ok(())
    }

    #[test]
    fn missing_install_preflight_waits_for_reinstall() {
        use crate::toolset::{ToolRequest, ToolSource};
        use std::sync::mpsc;
        use std::time::Duration;

        let td = tempfile::tempdir().unwrap();
        let version = td
            .path()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let backend =
            crate::args::BackendArg::new("node".to_string(), Some("core:node".to_string()));
        let request =
            ToolRequest::new_version_for_test(backend.into(), &version, ToolSource::Unknown);
        let mut tv = ToolVersion::new(request, version);
        tv.install_path = Some(td.path().join("install"));
        let held = lock_tool_install(&tv).unwrap();
        let (started_tx, started_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        std::thread::scope(|scope| {
            scope.spawn(|| {
                started_tx.send(()).unwrap();
                done_tx.send(preflight_tool_install(&tv)).unwrap();
            });
            started_rx.recv().unwrap();
            let before_unlock = done_rx.recv_timeout(Duration::from_millis(100));
            // Simulate the installer completing while holding its transaction lock.
            std::fs::create_dir(tv.install_path()).unwrap();
            drop(held);
            assert!(matches!(
                before_unlock,
                Err(mpsc::RecvTimeoutError::Timeout)
            ));
            done_rx
                .recv_timeout(Duration::from_secs(10))
                .unwrap()
                .unwrap();
        });
    }

    fn layer(annotations: &[(&str, &str)], digest: &str) -> Descriptor {
        Descriptor {
            media_type: manifest::MEDIA_TYPE_OCI_LAYER_GZIP.to_string(),
            size: 100,
            digest: digest.to_string(),
            annotations: annotations
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            platform: None,
        }
    }

    #[test]
    fn recognizes_an_alias_resolved_to_core_python() {
        let alias = crate::args::BackendArg::new("py".to_string(), Some("core:python".to_string()));
        let backend = crate::backend::arg_to_backend(alias).unwrap();
        assert!(is_python_backend(backend.as_ref()));
    }

    #[test]
    fn python_relocation_fingerprint_covers_exact_inputs_and_not_order() {
        let python_313 = PythonRelocation {
            version: "3.13.9".into(),
            host: "/host/python/3.13.9".into(),
            image: "/mise/installs/python/3.13.9".into(),
        };
        let python_314 = PythonRelocation {
            version: "3.14.3".into(),
            host: "/host/python/3.14.3".into(),
            image: "/mise/installs/python/3.14.3".into(),
        };
        let fingerprint = python_relocation_fingerprint(&[python_313.clone(), python_314.clone()]);
        assert_eq!(
            fingerprint,
            python_relocation_fingerprint(&[python_314.clone(), python_313.clone()])
        );
        assert_ne!(fingerprint, python_relocation_fingerprint(&[python_314]));

        let python_313_fingerprint =
            python_relocation_fingerprint(std::slice::from_ref(&python_313));
        let mut changed_target = python_313;
        changed_target.image = "/opt/python/3.13.9".into();
        assert_ne!(
            python_313_fingerprint,
            python_relocation_fingerprint(&[changed_target])
        );
    }

    #[test]
    fn reuse_index_keys_on_all_relocation_annotations() {
        let full = [
            (ANNOTATION_TOOL_SHORT, "jq"),
            (ANNOTATION_TOOL_VERSION, "1.8.1"),
            (ANNOTATION_LAYER_PREFIX, "mise/installs/jq/1.8.1"),
            (ANNOTATION_LAYER_OWNER, "0:0"),
            (ANNOTATION_LAYER_RELOCATION, TOOL_LAYER_RELOCATION_VERSION),
        ];
        // Second layer lacks prefix/owner/relocation (pushed by an older mise) —
        // not reusable.
        let partial = [
            (ANNOTATION_TOOL_SHORT, "node"),
            (ANNOTATION_TOOL_VERSION, "20.0.0"),
        ];
        let remote = registry::RemoteImage {
            manifest: ImageManifest {
                schema_version: 2,
                media_type: manifest::MEDIA_TYPE_OCI_MANIFEST.to_string(),
                config: layer(&[], "sha256:cfg"),
                layers: vec![layer(&full, "sha256:aaa"), layer(&partial, "sha256:bbb")],
                annotations: Default::default(),
            },
            diff_ids: vec!["sha256:diff-a".into(), "sha256:diff-b".into()],
            config: serde_json::json!({}),
        };

        let index = build_reuse_index(&remote);
        assert_eq!(index.len(), 1);
        let hit = index
            .get(&ReuseKey {
                short: "jq".into(),
                version: "1.8.1".into(),
                prefix: "mise/installs/jq/1.8.1".into(),
                owner: "0:0".into(),
                relocation: TOOL_LAYER_RELOCATION_VERSION.into(),
                plugin: String::new(),
            })
            .unwrap();
        assert_eq!(hit.digest, "sha256:aaa");
        assert_eq!(hit.diff_id, "sha256:diff-a");
        // Different owner -> miss.
        assert!(
            index
                .get(&ReuseKey {
                    short: "jq".into(),
                    version: "1.8.1".into(),
                    prefix: "mise/installs/jq/1.8.1".into(),
                    owner: "1000:1000".into(),
                    relocation: TOOL_LAYER_RELOCATION_VERSION.into(),
                    plugin: String::new(),
                })
                .is_none()
        );
    }

    #[test]
    fn reuse_index_keys_plugin_tools_on_the_plugin_fingerprint() {
        let base = [
            (ANNOTATION_TOOL_SHORT, "demo:hello"),
            (ANNOTATION_TOOL_VERSION, "1.0.0"),
            (ANNOTATION_LAYER_PREFIX, "mise/installs/demo-hello/1.0.0"),
            (ANNOTATION_LAYER_OWNER, "0:0"),
            (ANNOTATION_LAYER_RELOCATION, TOOL_LAYER_RELOCATION_VERSION),
        ];
        let mut with_plugin = base.to_vec();
        with_plugin.push((ANNOTATION_TOOL_PLUGIN, "sha256:plugin-a"));
        let remote = registry::RemoteImage {
            manifest: ImageManifest {
                schema_version: 2,
                media_type: manifest::MEDIA_TYPE_OCI_MANIFEST.to_string(),
                config: layer(&[], "sha256:cfg"),
                layers: vec![layer(&with_plugin, "sha256:aaa")],
                annotations: Default::default(),
            },
            diff_ids: vec!["sha256:diff-a".into()],
            config: serde_json::json!({}),
        };
        let index = build_reuse_index(&remote);
        let key = |plugin: &str| ReuseKey {
            short: "demo:hello".into(),
            version: "1.0.0".into(),
            prefix: "mise/installs/demo-hello/1.0.0".into(),
            owner: "0:0".into(),
            relocation: TOOL_LAYER_RELOCATION_VERSION.into(),
            plugin: plugin.into(),
        };
        assert!(index.contains_key(&key("sha256:plugin-a")));
        // An updated plugin, or no plugin at all, must not reuse the layer.
        assert!(!index.contains_key(&key("sha256:plugin-b")));
        assert!(!index.contains_key(&key("")));
    }

    #[test]
    fn embedded_plugin_fingerprint_hashes_plugin_sources() {
        assert_eq!(embedded_plugin_fingerprint("not-an-embedded-plugin"), None);
        let mut seen = std::collections::HashSet::new();
        for name in vfox::embedded_plugins::list_embedded_plugins() {
            let fingerprint = embedded_plugin_fingerprint(name).unwrap();
            assert!(fingerprint.starts_with("embedded:sha256:"));
            assert_eq!(embedded_plugin_fingerprint(name).unwrap(), fingerprint);
            // Distinct plugins have distinct sources, so distinct fingerprints.
            assert!(seen.insert(fingerprint), "duplicate fingerprint for {name}");
        }
    }

    #[test]
    fn host_home_detection_matches_whole_path_components() {
        let home = std::path::Path::new("/home/runner");
        assert!(value_refers_to_dir("/home/runner/go", home));
        assert!(value_refers_to_dir("/home/runner", home));
        // PATH-like values use the host's separator (`;` on Windows).
        let list = std::env::join_paths(["/mise/installs/x/bin", "/home/runner/.cargo/bin"])
            .unwrap()
            .to_string_lossy()
            .to_string();
        assert!(value_refers_to_dir(&list, home));
        assert!(!value_refers_to_dir("/home/runner2/go", home));
        assert!(!value_refers_to_dir("/mise/installs/x/1.0.0", home));
        assert!(!value_refers_to_dir("1", home));
        assert!(!value_refers_to_dir("/anything", std::path::Path::new("/")));
    }

    #[test]
    fn cached_path_entries_are_scoped_to_the_reused_tool() {
        let remote = registry::RemoteImage {
            manifest: ImageManifest {
                schema_version: 2,
                media_type: manifest::MEDIA_TYPE_OCI_MANIFEST.to_string(),
                config: layer(&[], "sha256:cfg"),
                layers: vec![],
                annotations: Default::default(),
            },
            diff_ids: vec![],
            config: serde_json::json!({
                "config": {
                    "Env": [
                        "PATH=/mise/installs/pnpm/9.15.9:/mise/installs/deno/2.0.0/bin:/mise/installs/pnpm/9.15.9/../../other/bin:/usr/bin"
                    ]
                }
            }),
        };

        assert_eq!(
            cached_tool_path_entries(&remote, "/mise/installs/pnpm/9.15.9"),
            vec!["/mise/installs/pnpm/9.15.9"]
        );
        assert_eq!(
            cached_tool_path_entries(&remote, "/mise/installs/deno/2.0.0"),
            vec!["/mise/installs/deno/2.0.0/bin"]
        );
    }

    #[test]
    fn resolve_layer_owner_defaults_to_root() {
        assert_eq!(
            resolve_layer_owner(None, &OciConfig::default()),
            LayerOwner::new(0, 0)
        );
    }

    #[test]
    fn resolve_layer_owner_uses_config_with_uid_as_gid_default() {
        let oci = OciConfig {
            user_id: Some(1000),
            ..Default::default()
        };

        assert_eq!(resolve_layer_owner(None, &oci), LayerOwner::new(1000, 1000));
    }

    #[test]
    fn resolve_layer_owner_uses_config_group_id_when_present() {
        let oci = OciConfig {
            user_id: Some(1000),
            group_id: Some(1001),
            ..Default::default()
        };

        assert_eq!(resolve_layer_owner(None, &oci), LayerOwner::new(1000, 1001));
    }

    #[test]
    fn resolve_layer_owner_uses_merged_config_group_id_from_same_layer() {
        let mut oci = OciConfig::default();
        oci.fill_defaults_from(OciConfig {
            user_id: Some(1000),
            group_id: Some(1001),
            ..Default::default()
        });

        assert_eq!(resolve_layer_owner(None, &oci), LayerOwner::new(1000, 1001));
    }

    #[test]
    fn resolve_layer_owner_does_not_inherit_less_specific_group_after_user_override() {
        let mut oci = OciConfig {
            user_id: Some(1000),
            ..Default::default()
        };
        oci.fill_defaults_from(OciConfig {
            group_id: Some(2000),
            ..Default::default()
        });

        assert_eq!(resolve_layer_owner(None, &oci), LayerOwner::new(1000, 1000));
    }

    #[test]
    fn resolve_layer_owner_cli_value_wins_over_config() {
        let oci = OciConfig {
            user_id: Some(1000),
            group_id: Some(1001),
            ..Default::default()
        };

        assert_eq!(
            resolve_layer_owner(Some(LayerOwner::new(2000, 2001)), &oci),
            LayerOwner::new(2000, 2001)
        );
    }
}
