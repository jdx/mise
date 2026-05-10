use std::path::{Path, PathBuf};
use std::process::Command;

use clap::ValueHint;
use eyre::{Context, Result, bail};
use tempfile::TempDir;

use crate::cli::oci::common::perform_build;
use crate::config::Settings;
use crate::file;
use crate::oci::BuildOptions;

/// [experimental] Build an OCI image from the current mise.toml and run a command in it
///
/// Equivalent to `mise oci build` followed by `docker run` / `podman run`.
/// The built image is loaded into the local container engine (podman is
/// preferred; docker works via skopeo) and the given command is executed
/// inside it with stdin/stdout/stderr inherited.
///
/// Requires `mise settings experimental=true` (or `MISE_EXPERIMENTAL=1`) and
/// one of: `podman`, `docker+skopeo`.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Run {
    // Long-only flags, kept alphabetical (asserted by
    // `cli::tests::test_subcommands_are_sorted`).
    /// Container engine to use (`auto`, `podman`, or `docker`)
    #[clap(long, default_value = "auto")]
    engine: Engine,

    /// Base image reference for the build (ignored with --image-dir)
    #[clap(long)]
    from: Option<String>,

    /// Use an already-built OCI image layout instead of building fresh
    #[clap(long, value_hint = ValueHint::DirPath, conflicts_with_all = &["from", "mount_point", "no_mise", "include_global"])]
    image_dir: Option<PathBuf>,

    /// Also include tools from the global / system config (default: project-only)
    ///
    /// See `mise oci build --help` for details.
    #[clap(long)]
    include_global: bool,

    /// Keep the loaded image in the engine's storage after the run
    ///
    /// By default, both the container (`--rm`) and the loaded image are
    /// removed when the command exits, so repeated `mise oci run` calls
    /// don't accumulate images in podman / docker storage. Pass `--keep`
    /// to retain the image under the tag mise used (`mise-oci:run-*` for
    /// docker; the pulled image ID for podman).
    #[clap(long)]
    keep: bool,

    /// Override in-image mount point (ignored with --image-dir)
    #[clap(long)]
    mount_point: Option<String>,

    /// Don't embed the mise binary (ignored with --image-dir)
    #[clap(long)]
    no_mise: bool,

    /// Bind-mount a host path (repeatable, `HOST:CONTAINER[:MODE]`)
    ///
    /// Note: unlike `docker run -v`, there's no `-v` short flag here because
    /// mise reserves `-v` for --verbose. Use `--volume` or `--mount`.
    #[clap(long = "volume", alias = "mount", value_name = "HOST:CONTAINER")]
    volume: Vec<String>,

    // Flags that have both a short and a long form.
    /// Set environment variable in the container (repeatable, `KEY=VAL`)
    #[clap(short = 'e', long = "env", value_name = "KEY=VAL")]
    env: Vec<String>,

    /// Run interactively (pass `-i` to the engine)
    #[clap(short, long)]
    interactive: bool,

    /// Allocate a TTY (pass `-t` to the engine)
    #[clap(short, long)]
    tty: bool,

    /// Working directory inside the container
    #[clap(short = 'w', long = "workdir")]
    workdir: Option<String>,

    /// Command and arguments to run inside the container (after `--`)
    #[clap(last = true)]
    cmd: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, clap::ValueEnum)]
enum Engine {
    Auto,
    Podman,
    Docker,
}

impl Run {
    pub async fn run(self) -> Result<()> {
        Settings::get().ensure_experimental("mise oci run")?;

        // 1. Validate arguments first so bad args win over "engine missing".
        if let Some(d) = &self.image_dir
            && !d.join("index.json").is_file()
        {
            bail!(
                "{}: does not look like an OCI image layout (missing index.json)",
                d.display()
            );
        }

        // 2. Locate a container engine.
        let engine = select_engine(self.engine)?;

        // 3. Build (or reuse an existing layout). When building, keep the
        // `TempDir` alive for the duration of the command — it removes the
        // directory on drop, so partial-gigabyte tool layers don't pile up
        // in /tmp across invocations.
        let (image_dir, _tempdir_guard): (PathBuf, Option<TempDir>) =
            if let Some(d) = &self.image_dir {
                (d.clone(), None)
            } else {
                let td = TempDir::with_prefix("mise-oci-run-")
                    .wrap_err("creating temp dir for oci build output")?;
                let out_dir = td.path().join("image");
                let opts = BuildOptions {
                    out_dir: out_dir.clone(),
                    from: self.from.clone(),
                    tag: Some("mise-oci:run".to_string()),
                    mount_point: self.mount_point.clone(),
                    include_mise: !self.no_mise,
                };
                let built = perform_build(opts, self.include_global).await?;
                info!("built image: {}", built.manifest_digest);
                (out_dir, Some(td))
            };

        // 4. Load into the engine. Returns the image reference to actually
        // pass to `podman run` / `docker run` (podman uses an image ID;
        // docker uses the tag skopeo copied under).
        let image_ref = load_image(engine, &image_dir)?;

        // 5. docker/podman run <flags> <image-ref> <cmd...>
        let engine_bin = engine_name(engine);
        let mut args: Vec<String> = vec!["run".into()];
        if !self.keep {
            args.push("--rm".into());
        }
        if self.interactive {
            args.push("-i".into());
        }
        if self.tty {
            args.push("-t".into());
        }
        for e in &self.env {
            args.push("-e".into());
            args.push(e.clone());
        }
        for v in &self.volume {
            args.push("-v".into());
            args.push(v.clone());
        }
        if let Some(w) = &self.workdir {
            args.push("-w".into());
            args.push(w.clone());
        }
        args.push(image_ref.clone());
        args.extend(self.cmd.clone());

        let run_result = Command::new(engine_bin)
            .args(&args)
            .status()
            .wrap_err_with(|| format!("exec {engine_bin} {args:?}"));

        // Clean up the loaded image unless the user passed `--keep`.
        // `docker run --rm` / `podman run --rm` only removes the
        // *container*; the image the engine loaded for us stays in local
        // storage and would otherwise accumulate across invocations.
        if !self.keep {
            let rmi = Command::new(engine_bin)
                .args(["rmi", "--force", &image_ref])
                .output();
            match rmi {
                Ok(out) if out.status.success() => {}
                Ok(out) => {
                    // Don't fail the overall command — just note it; a
                    // failing rmi usually means the image is still in use
                    // (e.g. another concurrent run) or was already deleted.
                    let stderr = String::from_utf8_lossy(&out.stderr);
                    debug!("{engine_bin} rmi {image_ref} failed: {stderr}");
                }
                Err(e) => debug!("failed to spawn {engine_bin} rmi: {e}"),
            }
        }

        let status = run_result?;
        if let Some(code) = status.code() {
            if code != 0 {
                std::process::exit(code);
            }
        } else if !status.success() {
            bail!("{engine_bin} exited abnormally: {status:?}");
        }
        Ok(())
    }
}

fn select_engine(requested: Engine) -> Result<Engine> {
    match requested {
        Engine::Podman => {
            if file::which("podman").is_some() {
                Ok(Engine::Podman)
            } else {
                bail!("--engine podman requested but `podman` was not found on PATH")
            }
        }
        Engine::Docker => {
            if file::which("docker").is_none() {
                bail!("--engine docker requested but `docker` was not found on PATH");
            }
            if file::which("skopeo").is_none() {
                bail!(
                    "--engine docker requires `skopeo` (to load the OCI layout into the \
                     docker daemon). Install skopeo or use `--engine podman`."
                );
            }
            Ok(Engine::Docker)
        }
        Engine::Auto => {
            if file::which("podman").is_some() {
                Ok(Engine::Podman)
            } else if file::which("docker").is_some() && file::which("skopeo").is_some() {
                Ok(Engine::Docker)
            } else {
                bail!(
                    "no supported container engine found. Install one of:\n  \
                       - podman (native OCI-layout support)\n  \
                       - docker + skopeo (to load the OCI layout into the docker daemon)"
                )
            }
        }
    }
}

fn engine_name(engine: Engine) -> &'static str {
    match engine {
        Engine::Podman => "podman",
        Engine::Docker => "docker",
        Engine::Auto => unreachable!("select_engine resolves Auto to a concrete engine"),
    }
}

/// Load the OCI layout at `image_dir` into the given engine and return the
/// image reference that should be passed to the engine's `run` subcommand.
///
/// We don't rely on `podman tag` here because `podman tag` takes an image
/// name/ID (not a transport reference), and the image name that `podman
/// pull oci:<dir>` assigns depends on the layout's `ref.name` annotation
/// and the podman version. Capturing the image ID printed by
/// `podman pull --quiet` is deterministic across versions.
fn load_image(engine: Engine, image_dir: &Path) -> Result<String> {
    match engine {
        Engine::Podman => {
            let src = format!("oci:{}", image_dir.display());
            let out = Command::new("podman")
                .args(["pull", "--quiet", &src])
                .output()
                .wrap_err("running `podman pull`")?;
            if !out.status.success() {
                let stderr = String::from_utf8_lossy(&out.stderr);
                bail!("podman pull failed: {}: {stderr}", out.status);
            }
            // `podman pull --quiet` prints just the image ID on stdout.
            let id = String::from_utf8(out.stdout)
                .wrap_err("podman pull produced non-utf8 output")?
                .trim()
                .to_string();
            if id.is_empty() {
                bail!("podman pull succeeded but printed no image ID");
            }
            Ok(id)
        }
        Engine::Docker => {
            // skopeo is the portable way to get an OCI layout into a docker
            // daemon. Pick a per-invocation tag so concurrent `mise oci run`
            // calls don't clobber each other — a shared `mise-oci:run` tag
            // would otherwise race: the second skopeo copy would overwrite
            // the first image before the first container started.
            let tag = format!(
                "mise-oci:run-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or(0)
            );
            let src = format!("oci:{}", image_dir.display());
            let dst = format!("docker-daemon:{tag}");
            let status = Command::new("skopeo")
                .args(["copy", &src, &dst])
                .status()
                .wrap_err("running `skopeo copy`")?;
            if !status.success() {
                bail!(
                    "skopeo copy failed ({status:?}). Ensure the docker daemon is running and \
                     your user has access to the socket."
                );
            }
            Ok(tag)
        }
        Engine::Auto => unreachable!(),
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    Build the current mise.toml and drop into bash:
    $ <bold>mise oci run -it -- bash</bold>

    Run a one-shot command with env + volume (note: `-v` is reserved
    for --verbose, so use `--volume`):
    $ <bold>mise oci run -e DEBUG=1 --volume $PWD:/work -w /work -- npm test</bold>

    Re-use a previously built layout (skip the build step):
    $ <bold>mise oci build -o ./img && mise oci run --image-dir ./img -- node -e 'console.log(process.version)'</bold>

<bold><underline>Engines:</underline></bold>

    Prefers <bold>podman</bold> (loads OCI layouts natively). Falls back to
    <bold>docker + skopeo</bold>. Pass <bold>--engine podman</bold> or <bold>--engine docker</bold> to override.
"#
);
