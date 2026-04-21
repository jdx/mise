//! Orchestrates building an OCI image from a resolved mise Toolset.
//!
//! Produces an OCI image layout with one layer per tool version so that
//! bumping any single tool invalidates exactly one content-addressable blob.
//! See the README in this module for the design.

use std::path::PathBuf;
use std::sync::Arc;

use eyre::{Context, Result, bail};
use indexmap::IndexMap;

use crate::backend::backend_type::BackendType;
use crate::config::{Config, Settings};
use crate::file;
use crate::oci::OciConfig;
use crate::oci::layer::{self, LayerBlob};
use crate::oci::layout::ImageLayout;
use crate::oci::manifest::{self, Descriptor, ImageConfig, ImageManifest, Platform, RootFs};
use crate::oci::registry;
use crate::toolset::{ToolVersion, Toolset};

/// Options passed to the builder from the CLI.
#[derive(Debug, Clone)]
pub struct BuildOptions {
    /// Output directory for the OCI image layout.
    pub out_dir: PathBuf,
    /// Base image reference (overrides mise.toml and default setting).
    pub from: Option<String>,
    /// Tag to write into index.json (ref.name annotation).
    pub tag: Option<String>,
    /// Where mise tools get installed inside the image.
    pub mount_point: Option<String>,
    /// Embed the current mise binary at /usr/local/bin/mise.
    pub include_mise: bool,
}

pub struct Builder {
    pub cfg: Arc<Config>,
    pub ts: Toolset,
    pub oci: OciConfig,
    pub opts: BuildOptions,
}

/// Output summary returned to the CLI.
pub struct BuildOutput {
    pub out_dir: PathBuf,
    pub manifest_digest: String,
    pub tool_layers: Vec<ToolLayerInfo>,
}

pub struct ToolLayerInfo {
    pub short: String,
    pub version: String,
    pub digest: String,
    pub size: u64,
}

impl Builder {
    pub fn new(cfg: Arc<Config>, ts: Toolset, oci: OciConfig, opts: BuildOptions) -> Self {
        Self { cfg, ts, oci, opts }
    }

    /// Build the image and write it to the output directory.
    pub async fn build(self) -> Result<BuildOutput> {
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
            let pull = registry::pull_base_image(ref_, &layout, desired)
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

        // --- 2. Per-tool layers ---
        // Tool installs are host-native binaries. On non-linux hosts they'll
        // fail at runtime inside the linux container with `Exec format error`
        // — emit a single warning up front so the user isn't surprised after
        // the image appears to build successfully. (`--no-mise` silences the
        // mise-binary warning below but doesn't help with tool binaries; only
        // running the build on a linux host does.)
        if !versions.is_empty() && std::env::consts::OS != "linux" {
            warn!(
                "building on {host} host — the {n} tool layer(s) contain {host} binaries that \
                 will fail with `Exec format error` inside a linux container. Run \
                 `mise oci build` on a linux host (or in a linux container) for a working image.",
                host = std::env::consts::OS,
                n = versions.len()
            );
        }
        let mut tool_layers: Vec<(String, String, LayerBlob)> = Vec::new();
        for (_, tv) in &versions {
            let install_path = tv.install_path();
            if !install_path.is_dir() {
                bail!(
                    "{} install path does not exist: {}. Run `mise install` first.",
                    tv.style(),
                    install_path.display()
                );
            }
            let tv_prefix = tool_prefix(&mount_point, tv);
            let blob = layer::build_layer_from_dir(&install_path, &tv_prefix)
                .wrap_err_with(|| format!("building layer for {}", tv.style()))?;
            tool_layers.push((tv.ba().short.clone(), tv.version.clone(), blob));
        }

        // --- 3. mise binary layer (optional) ---
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
                    mise_layer = Some(layer::build_layer_from_files(&files)?);
                }
                Err(e) => {
                    warn!("could not locate mise binary to embed in image: {e}");
                }
            }
        }

        // --- 4. Config layer: /etc/mise/config.toml ---
        let config_layer = {
            let config_toml = synthesize_embedded_config_toml(&versions, &mount_point);
            let files = vec![(
                "etc/mise/config.toml".to_string(),
                config_toml.into_bytes(),
                0o644u32,
            )];
            layer::build_layer_from_files(&files)?
        };

        // --- 5. Write all layer blobs into the layout ---
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

        for (short, version, blob) in &tool_layers {
            layout.write_blob_with_digest(&blob.digest, &blob.bytes)?;
            let mut annotations = IndexMap::new();
            annotations.insert("dev.mise.tool.short".to_string(), short.clone());
            annotations.insert("dev.mise.tool.version".to_string(), version.clone());
            manifest_layers.push(Descriptor {
                media_type: manifest::MEDIA_TYPE_OCI_LAYER_GZIP.to_string(),
                size: blob.size,
                digest: blob.digest.clone(),
                annotations,
                platform: None,
            });
            all_diff_ids.push(blob.diff_id.clone());
            tool_layer_infos.push(ToolLayerInfo {
                short: short.clone(),
                version: version.clone(),
                digest: blob.digest.clone(),
                size: blob.size,
            });
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

        // --- 6. Image config ---
        let image_config = self
            .build_image_config(
                &versions,
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

        // --- 7. Manifest ---
        let image_manifest = ImageManifest {
            schema_version: 2,
            media_type: manifest::MEDIA_TYPE_OCI_MANIFEST.to_string(),
            config: config_descriptor,
            layers: manifest_layers,
            annotations: Default::default(),
        };
        let (manifest_digest, manifest_size) = layout.write_manifest(&image_manifest)?;

        // --- 8. index.json ---
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
            let in_image_root = format!("/{}", tool_prefix(mount_point, tv));
            match backend.exec_env(&self.cfg, &self.ts, tv).await {
                Ok(tool_env) => {
                    for (k, v) in tool_env {
                        let rebased = rebase_path_value(&v, &host_install, &in_image_root);
                        env_pairs.insert(k, rebased);
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
        for (backend, tv) in versions {
            let install_path = tv.install_path();
            let in_image_tool_root = format!("/{}", tool_prefix(mount_point, tv));
            let bin_paths = backend
                .list_bin_paths(&self.cfg, tv)
                .await
                .unwrap_or_default();
            let mut had_one = false;
            for p in bin_paths {
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
            crate::cli::version::VERSION_PLAIN.to_string(),
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

fn reject_unsupported_backends(
    versions: &[(Arc<dyn crate::backend::Backend>, ToolVersion)],
) -> Result<()> {
    // Ask the actual backend instance rather than parsing the short name.
    // `BackendType::guess` only matches literal "asdf" / "vfox" prefixes and
    // misses third-party vfox plugins whose tools use a custom plugin name
    // as the prefix (e.g. `my-plugin:tool`), even though they have the same
    // out-of-tree write behavior we're guarding against.
    let bad: Vec<String> = versions
        .iter()
        .filter_map(|(backend, tv)| match backend.get_type() {
            BackendType::Asdf | BackendType::Vfox | BackendType::VfoxBackend(_) => {
                Some(tv.ba().short.clone())
            }
            _ => None,
        })
        .collect();
    if !bad.is_empty() {
        bail!(
            "mise oci build does not support asdf/vfox plugins in v1 (their install scripts can \
             write outside the per-version directory, breaking the one-layer-per-tool invariant). \
             Affected tools: {}",
            bad.join(", ")
        );
    }
    Ok(())
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

fn tool_prefix(mount_point: &str, tv: &ToolVersion) -> String {
    // Use the canonical directory names that mise itself uses on the host
    // (via `BackendArg::tool_dir_name` / `ToolVersion::tv_pathname`). A
    // naive `short.replace([':', '/'], "-")` would diverge — the real
    // path name goes through `to_kebab_case()` which also strips
    // non-alphanumerics and splits camelCase boundaries. Mismatching
    // would mean files land at a path mise can't resolve inside the
    // container.
    let plugin_dir = tv.ba().tool_dir_name();
    let version_dir = tv.tv_pathname();
    format!("{mount_point}/installs/{plugin_dir}/{version_dir}")
        .trim_start_matches('/')
        .to_string()
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
