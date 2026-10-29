//! Daemons that run a container image with Docker or Podman.
//!
//! A daemon with `image` is an ordinary custom daemon whose `run` command mise
//! writes: a short POSIX shell script that removes a stale container of the same
//! name, starts the container in the foreground with its ports published on
//! 127.0.0.1 only, and runs `<runtime> stop` when pitchfork stops the daemon.
//! Everything else (ports, exports, proxy, readiness, supervision, logs) is the
//! custom-daemon path, so a container gets the same per-worktree ports and
//! exported variables as any other daemon.
use eyre::{Result, bail};
use indexmap::IndexMap;
use std::path::{Path, PathBuf};

use crate::ports::{self, PortClaim};
use crate::presets::quote;

/// Seconds the runtime waits for the container to exit after SIGTERM before it
/// sends SIGKILL. Shorter than pitchfork's own stop timeout so the container is
/// gone by the time the supervisor gives up on the process.
const STOP_TIMEOUT_SECS: u32 = 10;

/// Set in the environment to run another docker-compatible CLI (`nerdctl`, a path)
/// or to pick Docker or Podman without editing the project's `mise.toml`.
pub const RUNTIME_ENV: &str = "MISE_CONTAINER_RUNTIME";

/// Keys only a container daemon understands. Without `image` they would reach
/// pitchfork as unknown keys and fail there, far from the cause.
const CONTAINER_KEYS: &[&str] = &[
    "command",
    "container_port",
    "data_mount",
    "runtime",
    "runtime_args",
    "volumes",
];

/// Names that would shadow a template variable a probe can use.
const RESERVED_PORT_NAMES: &[&str] = &["port", "container", "runtime"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Runtime {
    /// Docker if it is on PATH, otherwise Podman.
    Auto,
    Docker,
    Podman,
}

impl Runtime {
    fn parse(name: &str, value: &toml::Value) -> Result<Self> {
        match value.as_str() {
            Some("auto") => Ok(Self::Auto),
            Some("docker") => Ok(Self::Docker),
            Some("podman") => Ok(Self::Podman),
            _ => bail!(
                "[daemons.{name}].runtime must be \"auto\", \"docker\" or \"podman\"; to use another docker-compatible CLI, set {RUNTIME_ENV}"
            ),
        }
    }

    /// A shell expression for the runtime's executable, valid wherever the
    /// daemon's own command or a readiness probe runs: both go through `sh`.
    fn shell_expr(self) -> &'static str {
        match self {
            Self::Auto => {
                "${MISE_CONTAINER_RUNTIME:-$(command -v docker >/dev/null 2>&1 && echo docker || echo podman)}"
            }
            Self::Docker => "${MISE_CONTAINER_RUNTIME:-docker}",
            Self::Podman => "${MISE_CONTAINER_RUNTIME:-podman}",
        }
    }

    /// Why the runtime is missing, for the script to print.
    fn missing(self) -> &'static str {
        match self {
            Self::Auto => "neither docker nor podman was found on PATH",
            Self::Docker | Self::Podman => "$rt was not found on PATH",
        }
    }
}

/// One published port: where the container listens, and the host port it is
/// published on (127.0.0.1 only).
#[derive(Debug, Clone, PartialEq)]
enum PortDecl {
    Fixed {
        host: u16,
        container: u16,
    },
    /// The host port is `base + slot * stride`; `base` defaults to `container`.
    Auto {
        container: u16,
        base: Option<u16>,
        stride: u16,
    },
}

#[derive(Debug, Clone, PartialEq)]
enum Command {
    /// Shell text, as `run` is: the user quotes it.
    Line(String),
    Argv(Vec<String>),
}

/// The container keys of a daemon declaration, removed from its table.
#[derive(Debug)]
pub struct Decl {
    image: String,
    command: Option<Command>,
    runtime: Runtime,
    container_port: Option<u16>,
    ports: IndexMap<String, PortDecl>,
    volumes: Vec<String>,
    data_mount: Option<String>,
    data_dir: Option<String>,
    runtime_args: Vec<String>,
}

/// What a declaration becomes once the primary port is resolved.
#[derive(Debug)]
pub struct Built {
    /// The long-running command for pitchfork.
    pub run: String,
    /// Host ports of the named `ports`, in declaration order.
    pub ports: IndexMap<String, u16>,
    /// Host directory mounted at `data_mount`.
    pub data_dir: Option<PathBuf>,
    /// Template variables for `ready_cmd` and `health_cmd`.
    pub probe_ctx: tera::Context,
}

/// Remove the container keys from `table`. `None` means the daemon is not a
/// container; it is an error to set a container-only key without `image`.
pub fn take(name: &str, table: &mut toml::Table) -> Result<Option<Decl>> {
    let Some(image) = table.remove("image") else {
        if let Some(key) = CONTAINER_KEYS.iter().find(|key| table.contains_key(**key)) {
            bail!("[daemons.{name}].{key} requires image");
        }
        return Ok(None);
    };
    let image = match image.as_str().map(str::trim) {
        Some(image) if !image.is_empty() && !image.starts_with('-') => image.to_string(),
        _ => bail!("[daemons.{name}].image must be a non-empty image reference"),
    };
    let runtime = table
        .remove("runtime")
        .map(|value| Runtime::parse(name, &value))
        .transpose()?
        .unwrap_or(Runtime::Auto);
    let command = table
        .remove("command")
        .map(|value| match value {
            toml::Value::String(line) => Ok(Command::Line(line)),
            toml::Value::Array(items) => strings(name, "command", items).map(Command::Argv),
            _ => bail!("[daemons.{name}].command must be a string or an array of strings"),
        })
        .transpose()?;
    let container_port = table
        .remove("container_port")
        .map(|value| port_number(name, "container_port", &value))
        .transpose()?;
    let mut ports = IndexMap::new();
    if let Some(value) = table.remove("ports") {
        let Some(declared) = value.as_table() else {
            bail!("[daemons.{name}].ports must be a table");
        };
        for (key, value) in declared {
            validate_port_name(name, key)?;
            ports.insert(key.clone(), parse_port(name, key, value)?);
        }
    }
    let volumes = list(name, table, "volumes")?;
    let runtime_args = list(name, table, "runtime_args")?;
    let data_mount = table
        .remove("data_mount")
        .map(|value| match value.as_str() {
            Some(path) if path.starts_with('/') => Ok(path.to_string()),
            _ => bail!("[daemons.{name}].data_mount must be an absolute path inside the container"),
        })
        .transpose()?;
    let data_dir = table
        .remove("data_dir")
        .map(|value| match value.as_str() {
            Some(path) if !path.trim().is_empty() => Ok(path.to_string()),
            _ => bail!("[daemons.{name}].data_dir must not be empty"),
        })
        .transpose()?;
    if data_dir.is_some() && data_mount.is_none() {
        bail!("[daemons.{name}].data_dir requires data_mount, the path inside the container");
    }
    Ok(Some(Decl {
        image,
        command,
        runtime,
        container_port,
        ports,
        volumes,
        data_mount,
        data_dir,
        runtime_args,
    }))
}

fn strings(name: &str, key: &str, items: Vec<toml::Value>) -> Result<Vec<String>> {
    items
        .into_iter()
        .map(|item| match item {
            toml::Value::String(item) => Ok(item),
            _ => bail!("[daemons.{name}].{key} entries must be strings"),
        })
        .collect()
}

fn list(name: &str, table: &mut toml::Table, key: &str) -> Result<Vec<String>> {
    match table.remove(key) {
        None => Ok(Vec::new()),
        Some(toml::Value::Array(items)) => strings(name, key, items),
        Some(_) => bail!("[daemons.{name}].{key} must be an array of strings"),
    }
}

fn port_number(name: &str, key: &str, value: &toml::Value) -> Result<u16> {
    value
        .as_integer()
        .and_then(|n| u16::try_from(n).ok())
        .filter(|n| *n > 0)
        .ok_or_else(|| eyre::eyre!("[daemons.{name}].{key} must be an integer from 1 to 65535"))
}

/// A named port becomes a template variable and the middle of an exported
/// variable name.
fn validate_port_name(name: &str, key: &str) -> Result<()> {
    let mut chars = key.chars();
    let valid = chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
    if !valid || RESERVED_PORT_NAMES.contains(&key) {
        bail!(
            "[daemons.{name}].ports.{key}: port names use letters, numbers and '_', and cannot be {}",
            RESERVED_PORT_NAMES.join(", ")
        );
    }
    Ok(())
}

fn parse_port(name: &str, key: &str, value: &toml::Value) -> Result<PortDecl> {
    let at = format!("ports.{key}");
    match value {
        toml::Value::Integer(_) => {
            let port = port_number(name, &at, value)?;
            Ok(PortDecl::Fixed {
                host: port,
                container: port,
            })
        }
        toml::Value::Table(fields) => {
            for field in fields.keys() {
                if !matches!(
                    field.as_str(),
                    "container" | "host" | "auto" | "base" | "stride"
                ) {
                    bail!("unknown [daemons.{name}].{at} key {field:?}");
                }
            }
            let container = fields
                .get("container")
                .map(|v| port_number(name, &format!("{at}.container"), v))
                .transpose()?
                .ok_or_else(|| {
                    eyre::eyre!(
                        "[daemons.{name}].{at} needs container, the port inside the container"
                    )
                })?;
            let field = |field: &str| {
                fields
                    .get(field)
                    .map(|v| port_number(name, &format!("{at}.{field}"), v))
                    .transpose()
            };
            if fields.contains_key("auto") {
                if fields.get("auto").and_then(toml::Value::as_bool) != Some(true) {
                    bail!("[daemons.{name}].{at}.auto must be true; use host for a fixed port");
                }
                if fields.contains_key("host") {
                    bail!("[daemons.{name}].{at} cannot set both host and auto");
                }
                return Ok(PortDecl::Auto {
                    container,
                    base: field("base")?,
                    stride: field("stride")?.unwrap_or(ports::DEFAULT_STRIDE),
                });
            }
            if fields.contains_key("base") || fields.contains_key("stride") {
                bail!("[daemons.{name}].{at}: base and stride need auto = true");
            }
            Ok(PortDecl::Fixed {
                host: field("host")?.unwrap_or(container),
                container,
            })
        }
        _ => bail!(
            "[daemons.{name}].{at} must be an integer from 1 to 65535, or a table with container and host or auto"
        ),
    }
}

/// Everything resolved outside the declaration that the command needs.
pub struct Context<'a> {
    pub name: &'a str,
    pub root: &'a Path,
    /// The daemon's pitchfork namespace, which carries the project and worktree.
    pub namespace: &'a str,
    /// The primary port, already resolved.
    pub claim: Option<PortClaim>,
    /// Names of the variables the daemon's `env` sets; they are passed through
    /// to the container by name so values stay out of the command line.
    pub env: Vec<String>,
    /// Where `data_mount` is stored when `data_dir` is not set.
    pub default_data_dir: PathBuf,
}

/// The container's name: unique per project checkout and daemon, and stable, so
/// a leftover container from an earlier run can be found and removed.
pub fn container_name(namespace: &str, name: &str) -> String {
    // A namespace can be set in a project's own pitchfork config, where nothing
    // limits its characters; a container name allows letters, digits, `_`, `.`, `-`.
    let namespace: String = namespace
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-') {
                c
            } else {
                '-'
            }
        })
        .collect();
    format!("mise-{namespace}-{name}")
}

impl Decl {
    pub fn build(self, cx: Context<'_>) -> Result<Built> {
        let Context {
            name,
            root,
            namespace,
            claim,
            env,
            default_data_dir,
        } = cx;
        let container = container_name(namespace, name);
        let mut publish = Vec::new();
        let mut taken: Vec<u16> = Vec::new();
        let mut claim_host = |port: u16, what: &str| -> Result<()> {
            if taken.contains(&port) {
                bail!("[daemons.{name}] publishes host port {port} more than once ({what})");
            }
            taken.push(port);
            Ok(())
        };
        if let Some(claim) = claim {
            // The port the container listens on defaults to the one the primary
            // checkout publishes, so `port = 5432` and `port = { auto, base = 5432 }`
            // both mean "5432 inside, 5432 on the primary checkout".
            let container_port = self.container_port.unwrap_or(claim.base);
            claim_host(claim.port, "port")?;
            publish.push((claim.port, container_port));
        } else if self.container_port.is_some() {
            bail!("[daemons.{name}].container_port needs port, the host port to publish it on");
        }
        // A named auto port moves with the same slot as the primary one, so a
        // worktree's ports never collide with the primary checkout's.
        let slot = match claim {
            Some(claim) if claim.is_auto() => (claim.port - claim.base) / claim.stride,
            _ => ports::slot(root),
        };
        let mut named = IndexMap::new();
        for (key, port) in &self.ports {
            let (host, container_port) = match *port {
                PortDecl::Fixed { host, container } => (host, container),
                PortDecl::Auto {
                    container,
                    base,
                    stride,
                } => {
                    let base = base.unwrap_or(container);
                    let host = slot
                        .checked_mul(stride)
                        .and_then(|offset| base.checked_add(offset))
                        .ok_or_else(|| {
                            eyre::eyre!(
                                "[daemons.{name}].ports.{key} base {base} with stride {stride} exceeds 65535 for this worktree; choose a lower base or stride"
                            )
                        })?;
                    (host, container)
                }
            };
            claim_host(host, &format!("ports.{key}"))?;
            publish.push((host, container_port));
            named.insert(key.clone(), host);
        }

        let mut mounts = Vec::new();
        let mut create = Vec::new();
        let data_dir = self.data_mount.as_ref().map(|mount| {
            let dir = match &self.data_dir {
                Some(dir) => root.join(mise_util::file::replace_path(dir)),
                None => default_data_dir.clone(),
            };
            mounts.push(format!("{}:{mount}", dir.to_string_lossy()));
            create.push(dir.clone());
            dir
        });
        for volume in &self.volumes {
            mounts.push(resolve_volume(name, root, volume, &mut create)?);
        }

        let mut script = String::from("{\n");
        script.push_str(&format!("rt=\"{}\"\n", self.runtime.shell_expr()));
        script.push_str(&format!(
            "command -v \"$rt\" >/dev/null 2>&1 || {{ echo {} >&2; exit 127; }}\n",
            quote(format!(
                "mise: daemon {name} needs a container runtime, but {}. Install Docker or Podman, set runtime on the daemon, or set {RUNTIME_ENV}.",
                self.runtime.missing()
            ))
            // `$rt` is expanded by the shell inside the double quotes the message sits in.
            .replace("$rt", "'\"$rt\"'")
        ));
        script.push_str(&format!("name={}\n", quote(&container)));
        // A previous run that was killed rather than stopped leaves its container
        // behind; the name is ours, so it is safe to take.
        script.push_str("\"$rt\" rm -f \"$name\" >/dev/null 2>&1\n");
        script.push_str(&format!(
            "trap 'stopping=1; \"$rt\" stop -t {STOP_TIMEOUT_SECS} \"$name\" >/dev/null 2>&1' TERM INT HUP\n"
        ));
        for dir in &create {
            let dir = quote(dir.to_string_lossy());
            script.push_str(&format!("[ -e {dir} ] || mkdir -p {dir}\n"));
        }
        script.push_str("[ -n \"$stopping\" ] && exit 0\n");
        let mut run = vec![
            "\"$rt\" run --rm --name \"$name\"".to_string(),
            format!(
                "--label {}",
                quote(format!("dev.mise.daemon={namespace}/{name}"))
            ),
            format!(
                "--label {}",
                quote(format!("dev.mise.project={}", root.to_string_lossy()))
            ),
        ];
        for (host, container_port) in &publish {
            run.push(format!("-p 127.0.0.1:{host}:{container_port}"));
        }
        for var in &env {
            if !is_env_name(var) {
                bail!("[daemons.{name}].env: {var:?} is not a valid variable name");
            }
            run.push(format!("-e {var}"));
        }
        for mount in &mounts {
            run.push(format!("-v {}", quote(mount)));
        }
        run.extend(self.runtime_args.iter().map(quote));
        run.push("--".to_string());
        run.push(quote(&self.image));
        match &self.command {
            Some(Command::Line(line)) => run.push(line.clone()),
            Some(Command::Argv(argv)) => run.extend(argv.iter().map(quote)),
            None => {}
        }
        script.push_str(&run.join(" "));
        script.push_str(" &\npid=$!\nwait \"$pid\"\nstatus=$?\n");
        // A trapped signal ends `wait` early; the container is stopping, and the
        // runtime client exits once it has, so collect it before leaving.
        script.push_str("if [ -n \"$stopping\" ]; then wait \"$pid\" 2>/dev/null; exit 0; fi\n");
        script.push_str("exit \"$status\"\n}");

        // Loading must still succeed on Windows, where `mise env` reads this
        // project too; only starting the daemon fails, and says why.
        if cfg!(windows) {
            script = format!(
                "echo mise: daemon {name} cannot start: container daemons are not supported on Windows 1>&2 & exit /b 1"
            );
        }

        let mut probe_ctx = tera::Context::new();
        probe_ctx.insert("container", &container);
        probe_ctx.insert("runtime", self.runtime.shell_expr());
        if let Some(claim) = claim {
            probe_ctx.insert("port", &claim.port);
        }
        for (key, host) in &named {
            probe_ctx.insert(key.clone(), host);
        }
        Ok(Built {
            run: script,
            ports: named,
            data_dir,
            probe_ctx,
        })
    }
}

fn is_env_name(var: &str) -> bool {
    let mut chars = var.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// A `-v` argument. A source that is a path (starts with `.`, `/` or `~`) is
/// resolved against the project root and created when missing, because Podman
/// refuses a missing source and Docker would create it owned by root. Anything
/// else is a named volume, or an anonymous one, and passes through.
fn resolve_volume(
    name: &str,
    root: &Path,
    volume: &str,
    create: &mut Vec<PathBuf>,
) -> Result<String> {
    let (source, rest) = match volume.split_once(':') {
        Some((source, rest)) => (source, Some(rest)),
        None => (volume, None),
    };
    if volume.trim().is_empty() || source.is_empty() {
        bail!("[daemons.{name}].volumes entry {volume:?} needs a source");
    }
    let Some(rest) = rest else {
        return Ok(volume.to_string());
    };
    if !matches!(source.chars().next(), Some('.' | '/' | '~')) {
        return Ok(volume.to_string());
    }
    let path = root.join(mise_util::file::replace_path(source));
    create.push(path.clone());
    Ok(format!("{}:{rest}", path.to_string_lossy()))
}

/// Render `ready_cmd` and `health_cmd` with the container's variables
/// (`container`, `runtime`, `port` and the named ports). These are the only
/// probes a container daemon needs to template: the container name and the
/// runtime are not known when the configuration is written.
pub fn render_probes(root: &Path, table: &mut toml::Table, ctx: &tera::Context) -> Result<()> {
    let mut renderer = mise_util::tera::get_tera(Some(root));
    for key in ["ready_cmd", "health_cmd"] {
        match table.get_mut(key) {
            Some(toml::Value::Table(fields)) => {
                if let Some(command) = fields.get_mut("run") {
                    crate::presets::render_value(&mut renderer, command, ctx)?;
                }
            }
            Some(command) => crate::presets::render_value(&mut renderer, command, ctx)?,
            None => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decl(source: &str) -> Decl {
        let mut table: toml::Table = toml::from_str(source).unwrap();
        take("db", &mut table).unwrap().unwrap()
    }

    fn cx<'a>(root: &'a Path, claim: Option<PortClaim>) -> Context<'a> {
        Context {
            name: "db",
            root,
            namespace: "proj-abc",
            claim,
            env: vec![],
            default_data_dir: PathBuf::from("/state/data/db"),
        }
    }

    #[test]
    fn a_table_without_image_is_not_a_container() {
        let mut table: toml::Table = toml::from_str("run = \"exec sleep 1\"").unwrap();
        assert!(take("db", &mut table).unwrap().is_none());
        assert!(table.contains_key("run"));
    }

    #[test]
    fn container_only_keys_need_image() {
        for key in [
            "command = \"x\"",
            "volumes = []",
            "runtime = \"podman\"",
            "container_port = 80",
            "data_mount = \"/data\"",
            "runtime_args = []",
        ] {
            let mut table: toml::Table = toml::from_str(&format!("run = \"x\"\n{key}")).unwrap();
            let err = take("db", &mut table).unwrap_err().to_string();
            assert!(err.contains("requires image"), "{key}: {err}");
        }
    }

    #[test]
    fn rejects_bad_declarations() {
        let bad = |source: &str| {
            let mut table: toml::Table = toml::from_str(source).unwrap();
            take("db", &mut table).unwrap_err().to_string()
        };
        assert!(bad("image = \"\"").contains("image"));
        assert!(bad("image = \"-v\"").contains("image"));
        assert!(bad("image = \"x\"\nruntime = \"lxc\"").contains("runtime"));
        assert!(bad("image = \"x\"\ndata_dir = \"d\"").contains("data_mount"));
        assert!(bad("image = \"x\"\ndata_mount = \"rel\"").contains("absolute"));
        assert!(bad("image = \"x\"\nports.port = 1").contains("cannot be"));
        assert!(bad("image = \"x\"\nports.a-b = 1").contains("port names"));
        assert!(bad("image = \"x\"\nports.http = { host = 1 }").contains("container"));
        assert!(
            bad("image = \"x\"\nports.http = { container = 1, host = 2, auto = true }")
                .contains("both host and auto")
        );
        assert!(bad("image = \"x\"\nports.http = { container = 1, base = 2 }").contains("auto"));
        assert!(
            bad("image = \"x\"\nports.http = { container = 1, bogus = 2 }").contains("unknown")
        );
        assert!(bad("image = \"x\"\nports.http = \"80\"").contains("must be an integer"));
    }

    #[test]
    fn publishes_on_loopback_only() {
        let root = Path::new("/work/proj");
        let built = decl("image = \"postgres:18\"")
            .build(cx(root, Some(PortClaim::fixed(5432))))
            .unwrap();
        assert!(
            built.run.contains("-p 127.0.0.1:5432:5432"),
            "{}",
            built.run
        );
        assert!(!built.run.contains("-p 5432"));
        assert!(built.run.contains("-- 'postgres:18'"));
    }

    #[test]
    fn container_port_defaults_to_the_base_of_an_auto_port() {
        let root = Path::new("/work/proj");
        let claim = PortClaim {
            port: 5440,
            base: 5432,
            stride: 1,
        };
        let built = decl("image = \"postgres:18\"")
            .build(cx(root, Some(claim)))
            .unwrap();
        assert!(
            built.run.contains("-p 127.0.0.1:5440:5432"),
            "{}",
            built.run
        );
        let built = decl("image = \"x\"\ncontainer_port = 80")
            .build(cx(root, Some(claim)))
            .unwrap();
        assert!(built.run.contains("-p 127.0.0.1:5440:80"), "{}", built.run);
    }

    #[test]
    fn named_ports_follow_the_primary_slot() {
        let root = Path::new("/work/proj");
        let claim = PortClaim {
            port: 26264,
            base: 26257,
            stride: 1,
        };
        let built = decl(
            "image = \"x\"\nports.http = { container = 8080, auto = true, base = 27000, stride = 2 }\nports.fixed = { container = 9, host = 4000 }\nports.same = 7000",
        )
        .build(cx(Path::new("/work/proj"), Some(claim)))
        .unwrap();
        assert_eq!(built.ports["http"], 27014);
        assert_eq!(built.ports["fixed"], 4000);
        assert_eq!(built.ports["same"], 7000);
        assert!(built.run.contains("-p 127.0.0.1:27014:8080"));
        assert!(built.run.contains("-p 127.0.0.1:4000:9"));
        // The base defaults to the container port.
        let built = decl("image = \"x\"\nports.http = { container = 8080, auto = true }")
            .build(cx(root, Some(claim)))
            .unwrap();
        assert_eq!(built.ports["http"], 8087);
        let _ = root;
    }

    #[test]
    fn a_named_port_that_overflows_is_an_error() {
        let root = Path::new("/work/proj");
        let claim = PortClaim {
            port: 5439,
            base: 5432,
            stride: 1,
        };
        let err = decl("image = \"x\"\nports.http = { container = 80, auto = true, base = 65535 }")
            .build(cx(root, Some(claim)))
            .unwrap_err()
            .to_string();
        assert!(err.contains("exceeds 65535"), "{err}");
    }

    #[test]
    fn duplicate_host_ports_are_rejected() {
        let root = Path::new("/work/proj");
        let err = decl("image = \"x\"\nports.http = 5432")
            .build(cx(root, Some(PortClaim::fixed(5432))))
            .unwrap_err()
            .to_string();
        assert!(err.contains("more than once"), "{err}");
    }

    #[test]
    fn container_port_needs_a_published_port() {
        let err = decl("image = \"x\"\ncontainer_port = 80")
            .build(cx(Path::new("/work/proj"), None))
            .unwrap_err()
            .to_string();
        assert!(err.contains("needs port"), "{err}");
    }

    #[test]
    fn the_name_carries_the_checkout_identity() {
        assert_eq!(container_name("proj-abc", "db"), "mise-proj-abc-db");
        assert_eq!(container_name("my proj/x", "db"), "mise-my-proj-x-db");
        let built = decl("image = \"x\"")
            .build(cx(Path::new("/work/proj"), None))
            .unwrap();
        assert!(built.run.contains("name='mise-proj-abc-db'"));
        assert!(built.run.contains("--label 'dev.mise.daemon=proj-abc/db'"));
        assert!(built.run.contains("--label 'dev.mise.project=/work/proj'"));
    }

    #[test]
    fn stale_containers_are_removed_and_stop_is_forwarded() {
        let built = decl("image = \"x\"")
            .build(cx(Path::new("/work/proj"), None))
            .unwrap();
        let rm = built.run.find("\"$rt\" rm -f \"$name\"").unwrap();
        let run = built.run.find("\"$rt\" run --rm").unwrap();
        assert!(rm < run);
        assert!(built.run.contains("trap 'stopping=1; \"$rt\" stop -t 10"));
    }

    #[test]
    fn volumes_resolve_against_the_project_root() {
        let built = decl(
            "image = \"x\"\nvolumes = [\"./data:/data\", \"/abs:/abs:ro\", \"named:/n\", \"/anon\"]",
        )
        .build(cx(Path::new("/work/proj"), None))
        .unwrap();
        assert!(built.run.contains("-v '/work/proj/./data:/data'"));
        assert!(built.run.contains("-v '/abs:/abs:ro'"));
        assert!(built.run.contains("-v 'named:/n'"));
        assert!(built.run.contains("-v '/anon'"));
        // Path sources are created before the container starts; named volumes are not.
        assert!(
            built
                .run
                .contains("[ -e '/work/proj/./data' ] || mkdir -p '/work/proj/./data'")
        );
        assert!(!built.run.contains("mkdir -p 'named"));
    }

    #[test]
    fn data_mount_stores_data_per_checkout_by_default() {
        let built = decl("image = \"x\"\ndata_mount = \"/var/lib/data\"")
            .build(cx(Path::new("/work/proj"), None))
            .unwrap();
        assert_eq!(built.data_dir, Some(PathBuf::from("/state/data/db")));
        assert!(built.run.contains("-v '/state/data/db:/var/lib/data'"));
        let built = decl("image = \"x\"\ndata_mount = \"/d\"\ndata_dir = \".data/db\"")
            .build(cx(Path::new("/work/proj"), None))
            .unwrap();
        assert_eq!(built.data_dir, Some(PathBuf::from("/work/proj/.data/db")));
    }

    #[test]
    fn env_is_passed_by_name_so_values_stay_out_of_the_command() {
        let mut context = cx(Path::new("/work/proj"), None);
        context.env = vec!["POSTGRES_PASSWORD".into()];
        let built = decl("image = \"x\"").build(context).unwrap();
        assert!(built.run.contains(" -e POSTGRES_PASSWORD "));
        let mut context = cx(Path::new("/work/proj"), None);
        context.env = vec!["BAD NAME".into()];
        assert!(decl("image = \"x\"").build(context).is_err());
    }

    #[test]
    fn command_is_shell_text_or_an_argv() {
        let line = decl("image = \"x\"\ncommand = \"start --insecure\"")
            .build(cx(Path::new("/work/proj"), None))
            .unwrap();
        assert!(line.run.contains("-- 'x' start --insecure &"));
        let argv = decl("image = \"x\"\ncommand = [\"sh\", \"-c\", \"a b\"]")
            .build(cx(Path::new("/work/proj"), None))
            .unwrap();
        assert!(argv.run.contains("-- 'x' 'sh' '-c' 'a b' &"));
    }

    #[test]
    fn runtime_args_come_before_the_image() {
        let built = decl("image = \"x\"\nruntime_args = [\"--shm-size=1g\"]")
            .build(cx(Path::new("/work/proj"), None))
            .unwrap();
        let args = built.run.find("'--shm-size=1g'").unwrap();
        assert!(args < built.run.find("-- 'x'").unwrap());
    }

    #[test]
    fn the_runtime_can_be_picked_or_overridden() {
        let auto = decl("image = \"x\"")
            .build(cx(Path::new("/work/proj"), None))
            .unwrap();
        assert!(auto.run.contains("command -v docker"));
        assert!(auto.run.contains("neither docker nor podman"));
        let podman = decl("image = \"x\"\nruntime = \"podman\"")
            .build(cx(Path::new("/work/proj"), None))
            .unwrap();
        assert!(podman.run.contains("${MISE_CONTAINER_RUNTIME:-podman}"));
        assert!(!podman.run.contains("command -v docker"));
    }

    #[test]
    fn probes_see_the_container_runtime_and_ports() {
        let built = decl("image = \"x\"\nports.http = 8080")
            .build(cx(Path::new("/work/proj"), Some(PortClaim::fixed(5432))))
            .unwrap();
        let mut table: toml::Table = toml::from_str(
            "ready_cmd = \"{{ runtime }} exec {{ container }} true {{ port }} {{ http }}\"\nhealth_cmd = [\"c\", \"{{ container }}\"]",
        )
        .unwrap();
        render_probes(Path::new("/work/proj"), &mut table, &built.probe_ctx).unwrap();
        let ready = table["ready_cmd"].as_str().unwrap();
        assert!(
            ready.ends_with(" exec mise-proj-abc-db true 5432 8080"),
            "{ready}"
        );
        assert!(ready.starts_with("${MISE_CONTAINER_RUNTIME:-"), "{ready}");
        assert_eq!(table["health_cmd"][1].as_str(), Some("mise-proj-abc-db"));
    }
}
