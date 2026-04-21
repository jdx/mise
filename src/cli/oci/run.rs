use std::path::{Path, PathBuf};
use std::process::Command;

use clap::ValueHint;
use eyre::{Context, Result, bail};

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
    #[clap(long, value_hint = ValueHint::DirPath, conflicts_with_all = &["from", "mount_point", "no_mise"])]
    image_dir: Option<PathBuf>,

    /// Keep the image in the engine after the run (default: remove with `--rm`)
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

        // 3. Build (or reuse an existing layout).
        let image_dir: PathBuf = if let Some(d) = &self.image_dir {
            d.clone()
        } else {
            let out_dir = std::env::temp_dir().join(format!("mise-oci-{}", std::process::id()));
            if out_dir.exists() {
                std::fs::remove_dir_all(&out_dir).ok();
            }
            let opts = BuildOptions {
                out_dir: out_dir.clone(),
                from: self.from.clone(),
                tag: Some("mise-oci:run".to_string()),
                mount_point: self.mount_point.clone(),
                include_mise: !self.no_mise,
            };
            let built = perform_build(opts).await?;
            info!("built image: {}", built.manifest_digest);
            out_dir
        };

        // 3. Load into the engine under a known local tag.
        let tag = "mise-oci:run";
        load_image(engine, &image_dir, tag)?;

        // 4. docker/podman run <flags> <tag> <cmd...>
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
        args.push(tag.into());
        args.extend(self.cmd.clone());

        let status = Command::new(engine_bin)
            .args(&args)
            .status()
            .wrap_err_with(|| format!("exec {engine_bin} {args:?}"))?;

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

fn load_image(engine: Engine, image_dir: &Path, tag: &str) -> Result<()> {
    match engine {
        Engine::Podman => {
            // `podman pull oci:<dir>` loads an OCI layout into local storage.
            let arg = format!("oci:{}", image_dir.display());
            let status = Command::new("podman")
                .args(["pull", "--quiet", &arg])
                .status()
                .wrap_err("running `podman pull`")?;
            if !status.success() {
                bail!("podman pull failed: {status:?}");
            }
            // Tag it as `mise-oci:run` for convenience.
            let status = Command::new("podman")
                .args(["tag", &arg, tag])
                .status()
                .wrap_err("running `podman tag`")?;
            if !status.success() {
                // `podman tag` from oci: refs can be finicky; best-effort only.
                warn!("`podman tag {arg} {tag}` failed; will run via the oci: ref directly");
            }
            Ok(())
        }
        Engine::Docker => {
            // skopeo is the portable way to get an OCI layout into a docker daemon.
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
            Ok(())
        }
        Engine::Auto => unreachable!(),
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    Build the current mise.toml and drop into bash:
    $ <bold>mise oci run -it -- bash</bold>

    Run a one-shot command with env + volume:
    $ <bold>mise oci run -e DEBUG=1 -v $PWD:/work -w /work -- npm test</bold>

    Re-use a previously built layout (skip the build step):
    $ <bold>mise oci build -o ./img && mise oci run --image-dir ./img -- node -e 'console.log(process.version)'</bold>

<bold><underline>Engines:</underline></bold>

    Prefers <bold>podman</bold> (loads OCI layouts natively). Falls back to
    <bold>docker + skopeo</bold>. Pass <bold>--engine podman</bold> or <bold>--engine docker</bold> to override.
"#
);
