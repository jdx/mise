use super::ports::PortClaim;
use super::{Daemon, state_dir};
use eyre::{Context, Result, bail};
use indexmap::IndexMap;
use serde::Deserialize;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const PRESETS: &[(&str, &str)] = include!(concat!(env!("OUT_DIR"), "/daemon_presets.rs"));

/// Daemon table keys whose values are shell command lines. Only these render with
/// shell-quoted paths and option values; everything else (`depends`, `dir`, …) is a
/// literal that must not gain quotes.
const SHELL_KEYS: &[&str] = &["run", "ready_cmd"];

const MARKER: &str = ".mise-daemon-version";

/// How long an ephemeral init server has to accept its first request, across every
/// attempt, and how many times a lost port race is retried.
const READY_TIMEOUT: Duration = Duration::from_secs(120);
const INIT_SERVER_ATTEMPTS: usize = 3;
/// How long a server that finished its work has to shut down cleanly before it is
/// killed, so a store is flushed before the data directory is published.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(30);
/// How long to reap a killed process before abandoning it. SIGKILL cannot be
/// caught, but a process wedged in uninterruptible I/O outlives it, and waiting
/// on that would break the very deadline the kill is enforcing. An abandoned
/// child is reparented when this short-lived process exits.
const REAP_GRACE: Duration = Duration::from_secs(2);
/// How long one init step may run. Generous, because creating a cluster is slow
/// on a slow machine, but finite: a step whose server died mid-run would
/// otherwise block the daemon start for ever with nothing to show for it.
const STEP_TIMEOUT: Duration = Duration::from_secs(600);

#[derive(Deserialize)]
struct Preset {
    tool: String,
    port: u16,
    binary: String,
    /// Additional named ports (`http_port`, `metrics_port`, …) with their defaults.
    #[serde(default)]
    ports: IndexMap<String, u16>,
    version: VersionSpec,
    /// A file the tool itself writes with its own major version, cross-checked
    /// against the mise marker (for example Postgres's `PG_VERSION`).
    #[serde(default)]
    data_version_file: Option<String>,
    #[serde(default)]
    options: IndexMap<String, OptionSpec>,
    #[serde(default)]
    init: InitSpec,
    daemon: toml::Table,
    exports: IndexMap<String, String>,
}

/// How to ask the binary for its version and where the major version sits in the
/// output. Tool version strings are not assumed to be orderable; only the major
/// component is captured, for data-compatibility checks.
#[derive(Deserialize)]
struct VersionSpec {
    #[serde(default = "version_args")]
    args: Vec<String>,
    pattern: String,
}

fn version_args() -> Vec<String> {
    vec!["--version".into()]
}

#[derive(Deserialize, Default)]
struct InitSpec {
    /// An ephemeral server started for the duration of the init steps, for tools
    /// whose administrative commands require a running instance.
    #[serde(default)]
    server: Option<ServerSpec>,
    #[serde(default)]
    steps: Vec<StepSpec>,
}

#[derive(Deserialize)]
struct ServerSpec {
    run: Vec<String>,
    ready: Vec<String>,
    #[serde(default)]
    when: Option<String>,
}

#[derive(Deserialize)]
struct StepSpec {
    run: Vec<String>,
    #[serde(default)]
    when: Option<String>,
    /// Name of a list option; the step runs once per element with `item` bound.
    #[serde(default)]
    for_each: Option<String>,
    #[serde(default)]
    stdin: Option<String>,
    /// Run on every start, not only the first. For state the daemon keeps outside
    /// its data directory, such as a schema in a separate database, where the
    /// local marker says nothing about whether the step still needs to run.
    #[serde(default)]
    always: bool,
}

#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
enum OptionKind {
    String,
    Path,
    List,
    Int,
    Bool,
}

impl OptionKind {
    fn name(self) -> &'static str {
        match self {
            Self::String => "a string",
            Self::Path => "a path string",
            Self::List => "a list of strings",
            Self::Int => "an integer",
            Self::Bool => "a boolean",
        }
    }
}

/// Either a bare default whose type is inferred, or an explicit declaration.
#[derive(Deserialize, Clone)]
#[serde(untagged)]
enum OptionSpec {
    Declared {
        #[serde(rename = "type")]
        kind: OptionKind,
        default: toml::Value,
        #[serde(default)]
        pattern: Option<String>,
        /// Options that must also be changed from their own `default` whenever this
        /// one is changed from its `default`.
        #[serde(default)]
        requires: Vec<String>,
        /// Options whose presence makes this one a no-op, warned about when a user
        /// sets both rather than silently ignoring what they asked for.
        #[serde(default)]
        ignored_with: Vec<String>,
        /// For a list of `key=value` entries, where each entry's value has to be
        /// declared. A region a node never declared does not exist.
        #[serde(default)]
        entry_value_in: Option<EntryValueIn>,
    },
    Inferred(toml::Value),
}

struct OptionDef {
    kind: OptionKind,
    default: toml::Value,
    pattern: Option<String>,
    requires: Vec<String>,
    ignored_with: Vec<String>,
    entry_value_in: Option<EntryValueIn>,
}

/// Names the option holding `key=value` tiers, and which tier declares the value.
/// Matching any tier would accept `az=us-east-2` as declaring a region.
#[derive(Deserialize, Clone)]
struct EntryValueIn {
    option: String,
    key: String,
}

#[derive(Clone, Debug)]
enum OptionValue {
    Text(String),
    Path(String),
    List(Vec<String>),
    Int(i64),
    Bool(bool),
}

impl OptionSpec {
    fn define(&self, name: &str) -> Result<OptionDef> {
        match self {
            Self::Declared {
                kind,
                default,
                pattern,
                requires,
                ignored_with,
                entry_value_in,
            } => Ok(OptionDef {
                kind: *kind,
                default: default.clone(),
                pattern: pattern.clone(),
                requires: requires.clone(),
                ignored_with: ignored_with.clone(),
                entry_value_in: entry_value_in.clone(),
            }),
            Self::Inferred(value) => {
                let kind = match value {
                    toml::Value::String(_) => OptionKind::String,
                    toml::Value::Integer(_) => OptionKind::Int,
                    toml::Value::Boolean(_) => OptionKind::Bool,
                    toml::Value::Array(_) => OptionKind::List,
                    _ => bail!("preset option {name:?} has an unsupported default"),
                };
                Ok(OptionDef {
                    kind,
                    default: value.clone(),
                    pattern: None,
                    requires: Vec::new(),
                    ignored_with: Vec::new(),
                    entry_value_in: None,
                })
            }
        }
    }
}

impl OptionDef {
    /// Text values may be empty only when the preset's own default is empty, which
    /// keeps "unset" distinguishable from a required name that was blanked out.
    fn check_text(&self, name: &str, text: &str) -> Result<()> {
        if text.is_empty() {
            if self.default.as_str().is_some_and(|d| !d.is_empty()) {
                bail!("daemon option {name:?} must not be empty");
            }
            return Ok(());
        }
        if let Some(pattern) = &self.pattern {
            let re = regex::Regex::new(pattern)
                .wrap_err_with(|| format!("invalid pattern for daemon option {name:?}"))?;
            if !re.is_match(text) {
                bail!("daemon option {name:?} must match {pattern}");
            }
        }
        Ok(())
    }

    fn coerce(&self, name: &str, value: &toml::Value, root: &Path) -> Result<OptionValue> {
        let mismatch = || eyre::eyre!("daemon option {name:?} must be {}", self.kind.name());
        Ok(match self.kind {
            OptionKind::String => {
                let text = value.as_str().ok_or_else(mismatch)?;
                self.check_text(name, text)?;
                OptionValue::Text(text.into())
            }
            OptionKind::Path => {
                let text = value.as_str().ok_or_else(mismatch)?;
                self.check_text(name, text)?;
                if text.is_empty() {
                    OptionValue::Path(String::new())
                } else {
                    // `~/certs/tls.crt` is a path a user reasonably writes, and
                    // joining it to the project root would silently point at a
                    // directory named `~`.
                    let path = crate::file::replace_path(text);
                    let path = if path.is_absolute() {
                        path
                    } else {
                        root.join(path)
                    };
                    OptionValue::Path(path.to_string_lossy().into_owned())
                }
            }
            OptionKind::List => {
                let items = value.as_array().ok_or_else(mismatch)?;
                let mut out = Vec::with_capacity(items.len());
                for item in items {
                    let item = item.as_str().ok_or_else(mismatch)?;
                    if item.is_empty() {
                        bail!("daemon option {name:?} must not contain empty entries");
                    }
                    self.check_text(name, item)?;
                    out.push(item.to_string());
                }
                OptionValue::List(out)
            }
            OptionKind::Int => OptionValue::Int(value.as_integer().ok_or_else(mismatch)?),
            OptionKind::Bool => OptionValue::Bool(value.as_bool().ok_or_else(mismatch)?),
        })
    }
}

fn preset(name: &str) -> Result<Preset> {
    let content = PRESETS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, s)| s)
        .ok_or_else(|| {
            eyre::eyre!(
                "unknown daemon preset {name:?}; available presets: {}",
                PRESETS
                    .iter()
                    .map(|(n, _)| *n)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })?;
    Ok(toml::from_str(content)?)
}

/// The well-known port a preset binds when nothing overrides it, and the base
/// that `port = "auto"` offsets per worktree.
pub(crate) fn default_port(name: &str) -> Result<u16> {
    Ok(preset(name)?.port)
}

pub(crate) fn quote(value: impl AsRef<str>) -> String {
    format!("'{}'", value.as_ref().replace('\'', "'\\''"))
}

fn port_value(name: &str, key: &str, value: &toml::Value) -> Result<u16> {
    value
        .as_integer()
        .and_then(|n| u16::try_from(n).ok())
        .filter(|n| *n > 0)
        .ok_or_else(|| eyre::eyre!("[daemons.{name}].{key} must be an integer from 1 to 65535"))
}

/// Resolve declared options against user overrides. The result is ordered by the
/// preset's declaration order so rendered contexts are stable.
fn resolve_options(
    preset: &Preset,
    preset_name: &str,
    overrides: Option<&toml::Value>,
    root: &Path,
) -> Result<IndexMap<String, OptionValue>> {
    let mut values: IndexMap<String, toml::Value> = preset
        .options
        .iter()
        .map(|(k, spec)| Ok((k.clone(), spec.define(k)?.default)))
        .collect::<Result<_>>()?;
    if let Some(overrides) = overrides {
        let overrides = overrides
            .as_table()
            .ok_or_else(|| eyre::eyre!("daemon options must be a table"))?;
        for (key, value) in overrides {
            if !preset.options.contains_key(key) {
                bail!("unknown {preset_name} option {key:?}");
            }
            values.insert(key.clone(), value.clone());
        }
    }
    let resolved: IndexMap<String, OptionValue> = values
        .iter()
        .map(|(key, value)| {
            let def = preset.options[key].define(key)?;
            Ok((key.clone(), def.coerce(key, value, root)?))
        })
        .collect::<Result<_>>()?;
    // Options that only work as a set, such as a TLS certificate and its key, or a
    // SpiceDB datastore engine and its connection URI, fail here rather than
    // producing a command the tool quietly ignores. Both sides use one rule: an
    // option changed away from the preset's default requires its companions to be
    // changed too. Testing for a non-empty value instead would pass an untouched
    // option whose own default is meaningful, such as SpiceDB's memory engine.
    for (key, value) in &values {
        let def = preset.options[key].define(key)?;
        if def.requires.is_empty() || *value == def.default {
            continue;
        }
        for other in &def.requires {
            let other_default = preset
                .options
                .get(other)
                .ok_or_else(|| eyre::eyre!("preset option {key:?} requires unknown {other:?}"))?
                .define(other)?
                .default;
            if values.get(other) == Some(&other_default) {
                bail!("daemon option {key:?} also requires {other:?}");
            }
        }
    }
    // An entry may name something another option has to declare, as a database's
    // primary region must be a region the node was started with. Checking it here
    // turns a failed statement on first start into a configuration error.
    for (key, value) in &resolved {
        let def = preset.options[key].define(key)?;
        let (Some(spec), OptionValue::List(items)) = (&def.entry_value_in, value) else {
            continue;
        };
        let other = &spec.option;
        let declared = match resolved.get(other) {
            Some(OptionValue::Text(text)) => text.clone(),
            _ => bail!("preset option {key:?} names unknown or non-text {other:?}"),
        };
        for item in items {
            let Some((_, wanted)) = item.split_once('=') else {
                continue;
            };
            // Only the named tier declares it: in `region=us-east-2,zone=a` the
            // region is `us-east-2`, while `az=us-east-2` declares no region.
            let found = declared
                .split(',')
                .filter_map(|part| part.split_once('='))
                .any(|(tier, value)| tier.trim() == spec.key && value == wanted);
            if !found {
                bail!(
                    "daemon option {key:?} names {wanted:?}, which {other:?} does not declare as {:?}",
                    spec.key
                );
            }
        }
    }
    Ok(resolved)
}

/// Options the user set that the preset declares a no-op, given what else is set.
/// Only a declaration the user wrote counts: `mise daemons __init` reconstructs a
/// full table from the resolved context, where every option would look deliberate.
fn ignored_options(
    preset: &Preset,
    overrides: Option<&toml::Value>,
) -> Result<Vec<(String, String)>> {
    let Some(overrides) = overrides.and_then(toml::Value::as_table) else {
        return Ok(Vec::new());
    };
    let mut ignored = Vec::new();
    for key in overrides.keys() {
        let Some(spec) = preset.options.get(key) else {
            continue;
        };
        for other in spec.define(key)?.ignored_with {
            let other_default = preset
                .options
                .get(&other)
                .ok_or_else(|| eyre::eyre!("preset option {key:?} names unknown {other:?}"))?
                .define(&other)?
                .default;
            if overrides.get(&other).is_some_and(|v| *v != other_default) {
                ignored.push((key.clone(), other.clone()));
            }
        }
    }
    Ok(ignored)
}

/// Build the template context. `shell` quotes text and path values so they can be
/// interpolated into a command line; the unquoted form feeds exports, literal
/// daemon fields, and init argv.
fn context(
    data: &str,
    port: u16,
    ports: &IndexMap<String, u16>,
    options: &IndexMap<String, OptionValue>,
    shell: bool,
) -> tera::Context {
    let mut ctx = tera::Context::new();
    ctx.insert("data", &if shell { quote(data) } else { data.to_string() });
    ctx.insert("port", &port);
    for (key, value) in ports {
        ctx.insert(key.clone(), value);
    }
    for (key, value) in options {
        let key = key.clone();
        match value {
            OptionValue::Text(text) | OptionValue::Path(text) => {
                // An empty value stays empty so `{% if opt %}` blocks stay false.
                if text.is_empty() || !shell {
                    ctx.insert(key, text);
                } else {
                    ctx.insert(key, &quote(text));
                }
            }
            OptionValue::List(items) => {
                // Lists reach argv steps today, but the shell context must hold
                // shell-safe values for any preset that interpolates one.
                if shell {
                    ctx.insert(key, &items.iter().map(quote).collect::<Vec<_>>());
                } else {
                    ctx.insert(key, items);
                }
            }
            OptionValue::Int(n) => ctx.insert(key, n),
            OptionValue::Bool(b) => ctx.insert(key, b),
        }
    }
    ctx
}

/// The option values `mise daemons __init` needs, as JSON embedded in the run command.
fn init_values(
    port: u16,
    ports: &IndexMap<String, u16>,
    options: &IndexMap<String, OptionValue>,
) -> serde_json::Map<String, serde_json::Value> {
    let mut map = serde_json::Map::new();
    map.insert("port".into(), port.into());
    for (key, value) in ports {
        map.insert(key.clone(), (*value).into());
    }
    for (key, value) in options {
        let value = match value {
            OptionValue::Text(text) | OptionValue::Path(text) => text.clone().into(),
            OptionValue::List(items) => items.clone().into(),
            OptionValue::Int(n) => (*n).into(),
            OptionValue::Bool(b) => (*b).into(),
        };
        map.insert(key.clone(), value);
    }
    map
}

fn render_value(
    renderer: &mut crate::tera::TeraEngine,
    value: &mut toml::Value,
    ctx: &tera::Context,
) -> Result<()> {
    match value {
        toml::Value::String(text) => *text = crate::tera::render_str(renderer, text, ctx)?,
        toml::Value::Array(items) => {
            for item in items.iter_mut() {
                render_value(renderer, item, ctx)?;
            }
            // A template that renders to nothing means "not configured"; drop it so
            // optional fields such as `depends` disappear instead of holding a blank.
            items.retain(|item| item.as_str() != Some(""));
        }
        _ => {}
    }
    Ok(())
}

/// Wrap a command so it runs inside the project's tool environment. Needed
/// wherever pitchfork is told not to wrap the daemon itself in `mise x`.
pub(crate) fn in_tool_env(command: &str) -> String {
    format!(
        "{} x -- sh -c {}",
        quote(crate::env::MISE_BIN.to_string_lossy()),
        quote(command)
    )
}

/// Chain idempotent setup steps in front of the long-running command, using
/// pitchfork's shell-command semantics: every step must succeed before the
/// process that keeps running is reached. Returns `run` unchanged when there is
/// nothing to set up.
pub(crate) fn with_init(steps: &[String], run: &str) -> String {
    if steps.is_empty() {
        return run.to_string();
    }
    format!("{} && {run}", steps.join(" && "))
}

/// The declaration fields mise interprets itself rather than forwarding to
/// pitchfork: setup steps to run before the daemon, the port already resolved
/// for this project root, and the hostname components that root contributes.
pub(crate) struct Extras<'a> {
    pub init: &'a [String],
    pub port: Option<PortClaim>,
    pub labels: &'a super::urls::RootLabels,
    /// True when this daemon belongs to a project referenced with `project =`.
    /// Its exports go to that project's environment, not the one loading it.
    pub imported: bool,
}

pub(crate) fn expand(
    name: &str,
    preset_name: &str,
    version: &str,
    mut overrides: toml::Table,
    extras: Extras<'_>,
    source: &Path,
    root: &Path,
) -> Result<Daemon> {
    let preset = preset(preset_name)?;
    let tool = super::take_string(&mut overrides, "tool")?.unwrap_or_else(|| preset.tool.clone());
    // `port` is already parsed and resolved by the caller, which knows the
    // persisted allocation for this project root.
    let claim = extras.port.unwrap_or_else(|| PortClaim::fixed(preset.port));
    let port = claim.port;
    // An auto allocation moves every port this daemon binds by the same slot, so
    // a worktree's named ports stay with its primary one instead of colliding
    // with the primary checkout's.
    let slot = if claim.is_auto() {
        (claim.port - claim.base) / claim.stride
    } else {
        0
    };
    let mut ports = preset.ports.clone();
    if slot > 0 {
        let offset = slot.checked_mul(claim.stride).ok_or_else(|| {
            eyre::eyre!(
                "[daemons.{name}].port stride {} overflows the port range",
                claim.stride
            )
        })?;
        for (key, value) in ports.iter_mut() {
            // Clamping here would let two worktrees resolve the same listener to
            // 65535 while their primary ports stayed distinct.
            *value = value.checked_add(offset).filter(|p| *p > 0).ok_or_else(|| {
                eyre::eyre!(
                    "[daemons.{name}].ports.{key} {value} with stride {} exceeds 65535 for this worktree",
                    claim.stride
                )
            })?;
        }
    }
    if let Some(value) = overrides.remove("ports") {
        let table = value
            .as_table()
            .ok_or_else(|| eyre::eyre!("[daemons.{name}].ports must be a table"))?;
        for (key, value) in table {
            if !ports.contains_key(key) {
                bail!("unknown {preset_name} port {key:?}");
            }
            ports.insert(
                key.clone(),
                port_value(name, &format!("ports.{key}"), value)?,
            );
        }
    }
    let option_overrides = overrides.remove("options");
    let options = resolve_options(&preset, preset_name, option_overrides.as_ref(), root)?;
    for (key, other) in ignored_options(&preset, option_overrides.as_ref())? {
        warn!("[daemons.{name}] {preset_name} option {key:?} has no effect with {other:?} set");
    }

    let data = state_dir(root).join("data").join(name);
    // Resolve the proxy before rendering, so a preset's own default label and a
    // user override both reach `{{ url }}`. `proxy` and `proxy_tls` are the two
    // override keys that have to be applied early; everything else in
    // `overrides` still wins at the end, where it cannot change the URL.
    let mut table = preset.daemon;
    for key in ["proxy", "proxy_tls"] {
        if let Some(value) = overrides.get(key) {
            table.insert(key.into(), value.clone());
        }
    }
    // The resolved port has to be on the table before the proxy is read: a
    // daemon without one is never routed, and a preset always has one.
    table.insert("port".into(), super::expected_port(port));
    let proxy = super::urls::proxy_settings();
    let super::urls::Applied { host, .. } =
        super::urls::apply(name, &mut table, extras.labels, &proxy.tld)?;
    let data = data.to_string_lossy().into_owned();
    let mut shell_ctx = context(&data, port, &ports, &options, true);
    let mut plain_ctx = context(&data, port, &ports, &options, false);
    if let Some(host) = &host {
        let url = proxy.url(host);
        shell_ctx.insert("url", &quote(&url));
        shell_ctx.insert("host", &quote(host));
        plain_ctx.insert("url", &url);
        plain_ctx.insert("host", host);
    }
    let mut renderer = crate::tera::get_tera(Some(root));

    for (key, value) in table.iter_mut() {
        let ctx = if SHELL_KEYS.contains(&key.as_str()) {
            &shell_ctx
        } else {
            &plain_ctx
        };
        render_value(&mut renderer, value, ctx)?;
    }
    table.retain(|_, value| match value {
        toml::Value::String(text) => !text.is_empty(),
        toml::Value::Array(items) => !items.is_empty(),
        _ => true,
    });
    if let Some(toml::Value::String(command)) = table.get_mut("ready_cmd") {
        *command = in_tool_env(command.as_str());
    }
    let mut exports = preset.exports;
    for (key, value) in exports.iter_mut() {
        *value = crate::tera::render_str(&mut renderer, value, &plain_ctx).map_err(|err| {
            // `url` and `host` are the only context values a declaration can
            // take away, so name that cause rather than reporting a bare
            // template error about an undefined variable.
            if host.is_none() {
                eyre::eyre!(
                    "[daemons.{name}] sets proxy = false, but the {preset_name} preset derives {key} from the proxy hostname; drop proxy = false, or set {key} yourself in [env]"
                )
            } else {
                eyre::Report::from(err)
            }
        })?;
    }
    // Overrides use pitchfork's shell-command semantics verbatim. In particular,
    // callers can use `setup && exec server` without an extra shell or an `exec`
    // prefix that would terminate the shell before the second command.
    let run = super::take_string(&mut overrides, "run")?.unwrap_or_else(|| {
        format!(
            "exec {}",
            table.get("run").and_then(toml::Value::as_str).unwrap()
        )
    });
    // Data initialization always comes first; user `init` steps run after it,
    // once the data directory exists.
    let values = serde_json::to_string(&init_values(port, &ports, &options))?;
    let mut steps = vec![format!(
        "{} daemons __init {} {} --context {}",
        quote(crate::env::MISE_BIN.to_string_lossy()),
        quote(preset_name),
        quote(&data),
        quote(values),
    )];
    steps.extend(extras.init.iter().cloned());
    table.insert("run".into(), toml::Value::String(with_init(&steps, &run)));
    let mut expect = vec![port];
    for value in ports.values() {
        // Each port is a separate listener; a duplicate would be accepted here and
        // then fail to bind when the daemon starts.
        if expect.contains(value) {
            bail!("[daemons.{name}] uses port {value} more than once");
        }
        expect.push(*value);
    }
    table.insert("port".into(), super::expected_ports(&expect));
    table.insert("mise".into(), toml::Value::Boolean(true));
    // `proxy` and `proxy_tls` were already normalized above; re-applying the raw
    // override here would undo that.
    overrides.remove("proxy");
    overrides.remove("proxy_tls");
    table.extend(overrides);
    Ok(Daemon {
        name: name.into(),
        source: source.into(),
        root: root.into(),
        table,
        preset: Some(preset_name.into()),
        task: None,
        tool: Some((tool, version.into())),
        exports,
        imported: extras.imported,
        port: Some(claim),
        host,
    })
}

/// Database compatibility is preset-specific; tool request strings remain opaque.
fn major(preset: &Preset, preset_name: &str, output: &str) -> Result<String> {
    let re = regex::Regex::new(&preset.version.pattern)
        .wrap_err_with(|| format!("invalid version pattern for {preset_name}"))?;
    let version = re
        .captures(output)
        .and_then(|caps| caps.get(1))
        .map(|m| m.as_str())
        .ok_or_else(|| eyre::eyre!("unrecognized {preset_name} version output: {output}"))?;
    if version.is_empty() || !version.chars().all(|c| c.is_ascii_digit()) {
        bail!("unrecognized {preset_name} version: {version}");
    }
    Ok(version.into())
}

fn json_to_toml(name: &str, value: &serde_json::Value) -> Result<toml::Value> {
    Ok(match value {
        serde_json::Value::String(s) => toml::Value::String(s.clone()),
        serde_json::Value::Bool(b) => toml::Value::Boolean(*b),
        serde_json::Value::Number(n) => n
            .as_i64()
            .map(toml::Value::Integer)
            .ok_or_else(|| eyre::eyre!("daemon option {name:?} must be an integer"))?,
        serde_json::Value::Array(items) => toml::Value::Array(
            items
                .iter()
                .map(|item| json_to_toml(name, item))
                .collect::<Result<_>>()?,
        ),
        _ => bail!("daemon option {name:?} has an unsupported value"),
    })
}

/// Re-validate the values embedded in the generated run command. `__init` is a
/// separate process invocation, so its input is checked again rather than trusted.
/// The ports and options a generated run command passes to `mise daemons __init`.
struct InitContext {
    port: u16,
    ports: IndexMap<String, u16>,
    options: IndexMap<String, OptionValue>,
}

fn init_context(preset: &Preset, preset_name: &str, values: &str) -> Result<InitContext> {
    let values: serde_json::Map<String, serde_json::Value> = serde_json::from_str(values)
        .wrap_err("daemon initialization context must be a JSON object")?;
    let mut ports = IndexMap::new();
    let mut port = preset.port;
    let mut overrides = toml::Table::new();
    for (key, value) in &values {
        if key == "port" || preset.ports.contains_key(key) {
            let value = port_value(preset_name, key, &json_to_toml(key, value)?)?;
            if key == "port" {
                port = value;
            } else {
                ports.insert(key.clone(), value);
            }
            continue;
        }
        if !preset.options.contains_key(key) {
            bail!("unknown {preset_name} option {key:?}");
        }
        overrides.insert(key.clone(), json_to_toml(key, value)?);
    }
    for (key, value) in &preset.ports {
        ports.entry(key.clone()).or_insert(*value);
    }
    let options = resolve_options(
        preset,
        preset_name,
        Some(&toml::Value::Table(overrides)),
        Path::new("/"),
    )?;
    Ok(InitContext {
        port,
        ports,
        options,
    })
}

/// Two distinct free loopback ports. Both listeners stay open until the pair is
/// chosen, so the kernel cannot hand out the same port twice. They are released
/// before the server starts, which the readiness wait then confirms.
fn free_ports() -> Result<(u16, u16)> {
    let first = std::net::TcpListener::bind(("127.0.0.1", 0))?;
    let second = std::net::TcpListener::bind(("127.0.0.1", 0))?;
    Ok((first.local_addr()?.port(), second.local_addr()?.port()))
}

fn truthy(rendered: &str) -> bool {
    !matches!(rendered.trim(), "" | "false" | "0")
}

fn render_argv(
    renderer: &mut crate::tera::TeraEngine,
    argv: &[String],
    ctx: &tera::Context,
) -> Result<Vec<String>> {
    if argv.is_empty() {
        bail!("daemon initialization command must not be empty");
    }
    let rendered: Vec<String> = argv
        .iter()
        .map(|arg| Ok(crate::tera::render_str(renderer, arg, ctx)?))
        .collect::<Result<Vec<_>>>()?;
    // An element that renders to nothing is an unset optional flag, not an empty
    // argument; passing "" would be a different command.
    let rendered: Vec<String> = rendered.into_iter().filter(|arg| !arg.is_empty()).collect();
    if rendered.is_empty() {
        bail!("daemon initialization command rendered to nothing");
    }
    Ok(rendered)
}

/// Waits for a process, killing it at `deadline`. `None` means it was killed.
fn wait_until(child: &mut Child, deadline: Instant) -> Result<Option<std::process::ExitStatus>> {
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(Some(status));
        }
        if Instant::now() >= deadline {
            kill_and_reap(child);
            return Ok(None);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Kills a process and reaps it, giving up rather than waiting without bound.
fn kill_and_reap(child: &mut Child) {
    let _ = child.kill();
    let abandon_at = Instant::now() + REAP_GRACE;
    loop {
        match child.try_wait() {
            Ok(Some(_)) | Err(_) => return,
            Ok(None) => {}
        }
        if Instant::now() >= abandon_at {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Terminates the ephemeral init server on every exit path, including errors.
struct ServerGuard {
    child: Child,
    /// When a polite shutdown gives up and the process is killed. A server that is
    /// being abandoned carries the caller's remaining budget, so its shutdown cannot
    /// extend a wait the caller already bounded.
    kill_at: Option<Instant>,
}

impl ServerGuard {
    fn new(child: Child) -> Self {
        Self {
            child,
            kill_at: None,
        }
    }

    /// Stops the server now, killing it no later than `deadline`.
    fn stop_by(mut self, deadline: Instant) {
        self.kill_at = Some(deadline);
    }
}

impl Drop for ServerGuard {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            let pid = nix::unistd::Pid::from_raw(self.child.id() as i32);
            let _ = nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGTERM);
        }
        let deadline = self
            .kill_at
            .unwrap_or_else(|| Instant::now() + SHUTDOWN_GRACE);
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) | Err(_) => return,
                Ok(None) => {}
            }
            if Instant::now() >= deadline {
                kill_and_reap(&mut self.child);
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

/// Runs one readiness probe, giving up at `deadline`. A probe that blocks, such as
/// a client dialing a half-open socket, must not outlive the wait it belongs to.
fn probe(argv: &[String], deadline: Instant) -> bool {
    let Ok(mut child) = Command::new(&argv[0])
        .args(&argv[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Err(_) => return false,
            Ok(None) => {}
        }
        if Instant::now() >= deadline {
            kill_and_reap(&mut child);
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn wait_ready(argv: &[String], server: &mut ServerGuard, deadline: Instant) -> Result<()> {
    loop {
        if let Some(status) = server.child.try_wait()? {
            bail!("daemon initialization server exited before it was ready ({status})");
        }
        if probe(argv, deadline) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            bail!("daemon initialization server did not become ready");
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// Runs a preset's init steps against `data`. `only_always` restricts the run to
/// steps that repeat on every start, which is what an already-initialized data
/// directory needs.
fn run_steps(preset: &Preset, init: &InitContext, data: &Path, only_always: bool) -> Result<()> {
    let InitContext {
        port,
        ports,
        options,
    } = init;
    let steps: Vec<&StepSpec> = preset
        .init
        .steps
        .iter()
        .filter(|step| !only_always || step.always)
        .collect();
    if only_always && steps.is_empty() {
        return Ok(());
    }
    let mut ctx = context(&data.to_string_lossy(), *port, ports, options, false);
    let mut renderer = crate::tera::get_tera(None);
    let mut server = None;
    if let Some(spec) = &preset.init.server
        && !only_always
    {
        let wanted = match &spec.when {
            Some(when) => truthy(&crate::tera::render_str(&mut renderer, when, &ctx)?),
            None => true,
        };
        if wanted {
            // The chosen ports are released before the server binds them, so another
            // process can take one. A server that loses the race exits immediately,
            // which the readiness wait reports, so try again on a fresh pair. One
            // deadline covers every attempt and every abandoned server's shutdown,
            // so retrying cannot extend how long initialization can block.
            let deadline = Instant::now() + READY_TIMEOUT;
            let mut failure = None;
            for _ in 0..INIT_SERVER_ATTEMPTS {
                let (init_port, init_http_port) = free_ports()?;
                ctx.insert("init_port", &init_port);
                ctx.insert("init_http_port", &init_http_port);
                let argv = render_argv(&mut renderer, &spec.run, &ctx)?;
                let child = Command::new(&argv[0])
                    .args(&argv[1..])
                    .stdin(Stdio::null())
                    .spawn()
                    .wrap_err_with(|| format!("failed to start {}", argv[0]))?;
                let mut guard = ServerGuard::new(child);
                let ready = render_argv(&mut renderer, &spec.ready, &ctx)?;
                match wait_ready(&ready, &mut guard, deadline) {
                    Ok(()) => {
                        server = Some(guard);
                        break;
                    }
                    Err(err) => {
                        // Abandoning this attempt must not outlast the shared budget,
                        // however slowly the server responds to a stop signal.
                        guard.stop_by(deadline);
                        failure = Some(err);
                    }
                }
                if Instant::now() >= deadline {
                    break;
                }
            }
            if server.is_none() {
                return Err(failure
                    .unwrap_or_else(|| eyre::eyre!("daemon initialization server did not start")));
            }
        }
    }
    for step in steps {
        let items: Vec<Option<String>> = match &step.for_each {
            Some(key) => match options.get(key) {
                Some(OptionValue::List(items)) => items.iter().cloned().map(Some).collect(),
                _ => bail!("daemon initialization for_each {key:?} is not a list option"),
            },
            None => vec![None],
        };
        for item in items {
            let mut ctx = ctx.clone();
            if let Some(item) = &item {
                ctx.insert("item", item);
                // An entry may carry a value, as `name=region` does, so a step can
                // treat the halves separately without splitting in a template.
                let (key, value) = item.split_once('=').unwrap_or((item.as_str(), ""));
                ctx.insert("item_key", key);
                ctx.insert("item_value", value);
            }
            // Checked per entry so a step can act on some of a list and not others.
            if let Some(when) = &step.when
                && !truthy(&crate::tera::render_str(&mut renderer, when, &ctx)?)
            {
                continue;
            }
            let argv = render_argv(&mut renderer, &step.run, &ctx)?;
            let stdin = step
                .stdin
                .as_ref()
                .map(|text| crate::tera::render_str(&mut renderer, text, &ctx))
                .transpose()?;
            let mut command = Command::new(&argv[0]);
            command.args(&argv[1..]);
            command.stdin(if stdin.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            });
            let mut child = command
                .spawn()
                .wrap_err_with(|| format!("failed to run {}", argv[0]))?;
            if let Some(text) = stdin {
                child.stdin.take().unwrap().write_all(text.as_bytes())?;
            }
            let Some(status) = wait_until(&mut child, Instant::now() + STEP_TIMEOUT)? else {
                bail!(
                    "daemon initialization gave up on {} after {}s; existing data was preserved",
                    argv[0],
                    STEP_TIMEOUT.as_secs()
                );
            };
            if !status.success() {
                bail!(
                    "daemon initialization failed running {}; existing data was preserved",
                    argv[0]
                );
            }
        }
    }
    drop(server);
    Ok(())
}

/// Configurations generated before `--context` pass the database positionally, and
/// passed one to every preset, including those with no `database` option at all.
/// Only a preset that declares the option can be given it back.
fn legacy_values(preset: &Preset, database: Option<&str>) -> String {
    match database {
        Some(database) if preset.options.contains_key("database") => {
            serde_json::json!({ "database": database }).to_string()
        }
        _ => String::from("{}"),
    }
}

pub(crate) fn initialize(
    preset_name: &str,
    data: &Path,
    values: Option<&str>,
    legacy_database: Option<&str>,
) -> Result<()> {
    crate::config::Settings::get().ensure_experimental("daemon presets")?;
    crate::config::Settings::ensure_not_safe("initializing daemon data")?;
    if cfg!(windows) {
        bail!("daemon presets are not supported on Windows yet");
    }
    let preset = preset(preset_name)?;
    let owned;
    let values = match values {
        Some(values) => values,
        None => {
            owned = legacy_values(&preset, legacy_database);
            &owned
        }
    };
    let parent = data
        .parent()
        .ok_or_else(|| eyre::eyre!("daemon data must have a parent directory"))?;
    std::fs::create_dir_all(parent)?;
    let _lock = crate::lock_file::LockFile::new(data).lock()?;
    let output = Command::new(&preset.binary)
        .args(&preset.version.args)
        .output()?;
    if !output.status.success() {
        bail!("unable to determine {} version", preset.binary);
    }
    let major = major(&preset, preset_name, &String::from_utf8(output.stdout)?)?;
    let marker = format!("{preset_name}:{major}\n");
    if data.exists() {
        let existing = std::fs::read_to_string(data.join(MARKER))
            .wrap_err("daemon data was not initialized completely; inspect it and migrate or reset it explicitly")?;
        if existing != marker {
            bail!(
                "incompatible {preset_name} data in {} (stored {}, current {}); migrate or reset it explicitly",
                data.display(),
                existing.trim(),
                marker.trim()
            );
        }
        if let Some(file) = &preset.data_version_file {
            let recorded = std::fs::read_to_string(data.join(file)).wrap_err_with(|| {
                format!("daemon data was not initialized completely; missing {file}")
            })?;
            if recorded.trim() != major {
                bail!("{preset_name} data version does not match its initialization marker");
            }
        }
        let init = init_context(&preset, preset_name, values)?;
        return run_steps(&preset, &init, data, true);
    }
    let init = init_context(&preset, preset_name, values)?;
    let staging = tempfile::Builder::new()
        .prefix(".mise-init-")
        .tempdir_in(parent)?;
    let staged_data: PathBuf = staging.path().join("data");
    std::fs::create_dir(&staged_data)?;
    #[cfg(unix)]
    std::fs::set_permissions(
        &staged_data,
        std::os::unix::fs::PermissionsExt::from_mode(0o700),
    )?;
    run_steps(&preset, &init, &staged_data, false)?;
    if let Some(file) = &preset.data_version_file {
        let recorded = std::fs::read_to_string(staged_data.join(file)).wrap_err_with(|| {
            format!(
                "{} did not write {file} during initialization",
                preset.binary
            )
        })?;
        if recorded.trim() != major {
            bail!("{preset_name} initialization produced data for an incompatible version");
        }
    }
    std::fs::write(staged_data.join(MARKER), marker)?;
    std::fs::rename(staged_data, data)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels() -> super::super::urls::RootLabels {
        super::super::urls::RootLabels {
            project: Some("shop".into()),
            worktree: Some("main".into()),
        }
    }

    fn render(name: &str, overrides: toml::Table) -> Daemon {
        expand(
            name,
            name,
            "latest",
            overrides,
            Extras {
                init: &[],
                port: None,
                labels: &labels(),
                imported: false,
            },
            Path::new("/project/mise.toml"),
            Path::new("/project"),
        )
        .unwrap()
    }

    /// The JSON context embedded in the generated run command.
    fn init_json(daemon: &Daemon) -> serde_json::Value {
        let run = daemon.table["run"].as_str().unwrap();
        let start = run.find("--context '").unwrap() + "--context '".len();
        let end = run[start..].find("' && ").unwrap() + start;
        serde_json::from_str(&run[start..end].replace("'\\''", "'")).unwrap()
    }

    #[test]
    fn presets_render_local_persistent_daemons() {
        for (name, _) in PRESETS {
            let daemon = render(name, toml::Table::new());
            let run = daemon.table["run"].as_str().unwrap();
            assert!(run.contains(" daemons __init "));
            assert!(run.contains("127.0.0.1"));
            assert!(!run.contains("/latest"));
            assert_eq!(daemon.table["port"]["bump"].as_bool(), Some(false));
            // No preset's primary port speaks HTTP, so none may be routed through
            // the HTTP reverse proxy or advertise a proxy URL.
            assert!(daemon.host.is_none(), "{name} is proxied");
            assert_eq!(daemon.table["proxy"].as_bool(), Some(false), "{name}");
            // Every declared port is reserved, and nothing is left unrendered.
            let preset = preset(name).unwrap();
            assert_eq!(
                daemon.table["port"]["expect"].as_array().unwrap().len(),
                1 + preset.ports.len()
            );
            for value in daemon.table.values() {
                if let Some(text) = value.as_str() {
                    assert!(
                        !text.contains("{{"),
                        "unrendered template in {name}: {text}"
                    );
                }
            }
            for value in daemon.exports.values() {
                assert!(
                    !value.contains("{{"),
                    "unrendered export in {name}: {value}"
                );
            }
        }
    }

    #[test]
    fn named_instances_override_ports_and_keep_templates() {
        let overrides = toml::toml! { ready_cmd = "echo {{ env.FOO }}" };
        let daemon = expand(
            "analytics",
            "postgres",
            "18",
            overrides,
            Extras {
                init: &[],
                port: Some(PortClaim::fixed(5433)),
                labels: &labels(),
                imported: false,
            },
            Path::new("/p/mise.toml"),
            Path::new("/p"),
        )
        .unwrap();
        assert_eq!(daemon.exports["PGPORT"], "5433");
        assert_eq!(
            daemon.table["ready_cmd"].as_str(),
            Some("echo {{ env.FOO }}")
        );
        assert!(daemon.table["run"].as_str().unwrap().contains("analytics"));
    }

    #[test]
    fn compatibility_uses_actual_database_versions() {
        let postgres = preset("postgres").unwrap();
        let redis = preset("redis").unwrap();
        assert_eq!(
            major(&postgres, "postgres", "postgres (PostgreSQL) 18.6\n").unwrap(),
            "18"
        );
        assert_eq!(
            major(&redis, "redis", "Redis server v=8.10.1 sha=abc").unwrap(),
            "8"
        );
        assert!(major(&postgres, "postgres", "nightly").is_err());
        assert_eq!(quote("a'b"), "'a'\\''b'");
        assert_eq!(
            major(
                &preset("cockroachdb").unwrap(),
                "cockroachdb",
                "cockroach version v26.2.6 (x86_64-linux)\n"
            )
            .unwrap(),
            "26"
        );
        assert_eq!(
            major(&preset("nats").unwrap(), "nats", "nats-server: v2.14.7\n").unwrap(),
            "2"
        );
        assert_eq!(
            major(&preset("spicedb").unwrap(), "spicedb", "spicedb v1.56.2\n").unwrap(),
            "1"
        );
    }

    #[test]
    fn an_auto_port_moves_every_port_the_daemon_binds() {
        // A worktree's slot has to move the named ports too, or two checkouts
        // separate their primary ports and still collide on the HTTP listener.
        let slotted = PortClaim {
            port: 26257 + 7,
            base: 26257,
            stride: 1,
        };
        let daemon = expand(
            "crdb",
            "cockroachdb",
            "26",
            toml::Table::new(),
            Extras {
                init: &[],
                port: Some(slotted),
                labels: &labels(),
                imported: false,
            },
            Path::new("/p/mise.toml"),
            Path::new("/p"),
        )
        .unwrap();
        let run = daemon.table["run"].as_str().unwrap();
        assert!(run.contains("--listen-addr=127.0.0.1:26264"), "{run}");
        assert!(run.contains("--http-addr=127.0.0.1:8087"), "{run}");
        assert_eq!(
            daemon.table["port"]["expect"].as_array().unwrap(),
            &vec![toml::Value::Integer(26264), toml::Value::Integer(8087)]
        );
        // A named port above the primary one runs out of range first, so a far
        // enough slot must be rejected rather than clamped to a 65535 that another
        // slot could also land on. NATS monitors on 8222 above a base of 4222.
        let far = PortClaim {
            port: u16::MAX,
            base: 4222,
            stride: 1,
        };
        let overflow = expand(
            "events",
            "nats",
            "2",
            toml::Table::new(),
            Extras {
                init: &[],
                port: Some(far),
                labels: &labels(),
                imported: false,
            },
            Path::new("/p/mise.toml"),
            Path::new("/p"),
        );
        assert!(
            overflow.unwrap_err().to_string().contains("exceeds 65535"),
            "a named port past the range must be rejected"
        );
        // The primary checkout keeps the well-known ports.
        let primary = render("cockroachdb", toml::Table::new());
        let run = primary.table["run"].as_str().unwrap();
        assert!(run.contains("--http-addr=127.0.0.1:8080"), "{run}");
        // An explicitly chosen named port is exactly that, slot or no slot.
        let pinned = expand(
            "crdb",
            "cockroachdb",
            "26",
            toml::toml! { [ports] http_port = 9999 },
            Extras {
                init: &[],
                port: Some(slotted),
                labels: &labels(),
                imported: false,
            },
            Path::new("/p/mise.toml"),
            Path::new("/p"),
        )
        .unwrap();
        assert!(
            pinned.table["run"]
                .as_str()
                .unwrap()
                .contains("--http-addr=127.0.0.1:9999")
        );
    }

    #[test]
    fn named_ports_default_and_override_together() {
        let daemon = render("cockroachdb", toml::Table::new());
        let run = daemon.table["run"].as_str().unwrap();
        assert!(run.contains("--listen-addr=127.0.0.1:26257"));
        assert!(run.contains("--http-addr=127.0.0.1:8080"));
        let daemon = expand(
            "crdb",
            "cockroachdb",
            "26",
            toml::toml! { [ports] http_port = 8081 },
            Extras {
                init: &[],
                port: Some(PortClaim::fixed(26258)),
                labels: &labels(),
                imported: false,
            },
            Path::new("/p/mise.toml"),
            Path::new("/p"),
        )
        .unwrap();
        let run = daemon.table["run"].as_str().unwrap();
        assert!(run.contains("--listen-addr=127.0.0.1:26258"));
        assert!(run.contains("--http-addr=127.0.0.1:8081"));
        assert_eq!(
            daemon.table["port"]["expect"].as_array().unwrap(),
            &vec![toml::Value::Integer(26258), toml::Value::Integer(8081)]
        );
        assert!(daemon.exports["DATABASE_URL"].contains("127.0.0.1:26258"));
        let unknown = expand(
            "crdb",
            "cockroachdb",
            "26",
            toml::toml! { [ports] grpc_port = 1 },
            Extras {
                init: &[],
                port: None,
                labels: &labels(),
                imported: false,
            },
            Path::new("/p/mise.toml"),
            Path::new("/p"),
        );
        assert!(
            unknown
                .unwrap_err()
                .to_string()
                .contains("unknown cockroachdb port")
        );
    }

    #[test]
    fn list_options_reach_initialization_and_are_validated() {
        let daemon = expand(
            "crdb",
            "cockroachdb",
            "26",
            toml::toml! { [options] databases = ["entirecore", "spicedb"] database = "entirecore" },
            Extras {
                init: &[],
                port: None,
                labels: &labels(),
                imported: false,
            },
            Path::new("/p/mise.toml"),
            Path::new("/p"),
        )
        .unwrap();
        assert_eq!(
            init_json(&daemon)["databases"],
            serde_json::json!(["entirecore", "spicedb"])
        );
        assert!(daemon.exports["COCKROACH_URL"].ends_with("/entirecore?sslmode=disable"));
        let invalid = expand(
            "crdb",
            "cockroachdb",
            "26",
            toml::toml! { [options] databases = ["drop; table"] },
            Extras {
                init: &[],
                port: None,
                labels: &labels(),
                imported: false,
            },
            Path::new("/p/mise.toml"),
            Path::new("/p"),
        );
        assert!(invalid.unwrap_err().to_string().contains("must match"));
        let wrong_type = expand(
            "crdb",
            "cockroachdb",
            "26",
            toml::toml! { [options] databases = "entirecore" },
            Extras {
                init: &[],
                port: None,
                labels: &labels(),
                imported: false,
            },
            Path::new("/p/mise.toml"),
            Path::new("/p"),
        );
        assert!(
            wrong_type
                .unwrap_err()
                .to_string()
                .contains("must be a list of strings")
        );
    }

    #[test]
    fn path_options_resolve_against_the_project_root_and_are_quoted() {
        let daemon = expand(
            "events",
            "nats",
            "2",
            toml::toml! { [options] config = "conf/nats.conf" tls_cert = "/etc/nats/tls.crt" tls_key = "/etc/nats/tls.key" },
            Extras {
                init: &[],
                port: None,
                labels: &labels(),
                imported: false,
            },
            Path::new("/p/mise.toml"),
            Path::new("/p"),
        )
        .unwrap();
        let run = daemon.table["run"].as_str().unwrap();
        // A relative option resolves against the project root, spelled the way the
        // host spells paths.
        let resolved = Path::new("/p").join("conf/nats.conf");
        let resolved = resolved.to_string_lossy();
        assert_ne!(resolved, "conf/nats.conf");
        assert!(
            run.contains(&format!("--config {}", quote(&resolved))),
            "{run}"
        );
        assert_eq!(init_json(&daemon)["config"], serde_json::json!(resolved));
        assert!(run.contains("--tls --tlscert '"), "{run}");
        // A TLS-only listener needs clients to select TLS from the URL.
        assert_eq!(daemon.exports["NATS_URL"], "tls://127.0.0.1:4222");
        // Only Unix treats a leading slash as absolute, so only there is the root
        // guaranteed not to be prepended.
        #[cfg(unix)]
        assert!(run.contains("--tlscert '/etc/nats/tls.crt'"), "{run}");
        // A CA is only meaningful with verification turned on: nats-server loads
        // `--tlscacert` but checks a client against it only under `--tlsverify`.
        let verified = expand(
            "events",
            "nats",
            "2",
            toml::toml! {
                [options]
                tls_cert = "/tls/a.crt"
                tls_key = "/tls/a.key"
                tls_ca = "/tls/ca.crt"
            },
            Extras {
                init: &[],
                port: None,
                labels: &labels(),
                imported: false,
            },
            Path::new("/p/mise.toml"),
            Path::new("/p"),
        )
        .unwrap();
        let run = verified.table["run"].as_str().unwrap();
        assert!(
            run.contains("--tlsverify --tlscacert '/tls/ca.crt'"),
            "{run}"
        );
        // Unset paths drop their flags entirely.
        let daemon = render("nats", toml::Table::new());
        let run = daemon.table["run"].as_str().unwrap();
        assert!(!run.contains("--config"), "{run}");
        assert!(!run.contains("--tls"), "{run}");
        assert_eq!(daemon.exports["NATS_URL"], "nats://127.0.0.1:4222");
    }

    #[test]
    fn spicedb_depends_is_present_only_when_configured() {
        let daemon = render("spicedb", toml::Table::new());
        assert!(!daemon.table.contains_key("depends"));
        let run = daemon.table["run"].as_str().unwrap();
        assert!(run.contains("--datastore-engine 'memory'"), "{run}");
        assert!(!run.contains("--datastore-conn-uri"), "{run}");
        assert_eq!(daemon.exports["SPICEDB_ENDPOINT"], "127.0.0.1:50051");
        let daemon = expand(
            "spicedb",
            "spicedb",
            "1",
            toml::toml! {
                [options]
                datastore_engine = "cockroachdb"
                datastore_uri = "postgresql://root@127.0.0.1:26257/spicedb?sslmode=disable"
                datastore_daemon = "crdb"
            },
            Extras {
                init: &[],
                port: None,
                labels: &labels(),
                imported: false,
            },
            Path::new("/p/mise.toml"),
            Path::new("/p"),
        )
        .unwrap();
        assert_eq!(
            daemon.table["depends"].as_array().unwrap(),
            &vec![toml::Value::String("crdb".into())]
        );
        let run = daemon.table["run"].as_str().unwrap();
        // Connection URIs carry shell metacharacters and must reach the tool intact.
        assert!(
            run.contains(
                "--datastore-conn-uri 'postgresql://root@127.0.0.1:26257/spicedb?sslmode=disable'"
            ),
            "{run}"
        );
    }

    #[test]
    fn cluster_settings_accept_the_quoted_literals_sql_needs() {
        // Durations and strings are only valid as quoted SQL literals, so a pattern
        // that admits bare words alone can express no duration setting at all.
        let daemon = expand(
            "crdb",
            "cockroachdb",
            "26",
            toml::toml! {
                [options]
                databases = ["app"]
                settings = ["jobs.retention_time = '1h'", "kv.rangefeed.enabled = true"]
            },
            Extras {
                init: &[],
                port: None,
                labels: &labels(),
                imported: false,
            },
            Path::new("/p/mise.toml"),
            Path::new("/p"),
        )
        .unwrap();
        assert_eq!(
            init_json(&daemon)["settings"],
            serde_json::json!(["jobs.retention_time = '1h'", "kv.rangefeed.enabled = true"])
        );
        // A quoted literal must not be able to close its statement and open another.
        for injection in [
            "a.b = 'x'; DROP DATABASE app",
            "a.b = \"x\"",
            "a.b = 'x' -- comment",
        ] {
            let rejected = expand(
                "crdb",
                "cockroachdb",
                "26",
                toml::toml! { [options] settings = [injection] },
                Extras {
                    init: &[],
                    port: None,
                    labels: &labels(),
                    imported: false,
                },
                Path::new("/p/mise.toml"),
                Path::new("/p"),
            );
            assert!(rejected.is_err(), "accepted {injection}");
        }
    }

    #[test]
    fn duplicate_ports_are_rejected_before_the_daemon_starts() {
        let clash = expand(
            "crdb",
            "cockroachdb",
            "26",
            toml::Table::new(),
            Extras {
                init: &[],
                port: Some(PortClaim::fixed(8080)),
                labels: &labels(),
                imported: false,
            },
            Path::new("/p/mise.toml"),
            Path::new("/p"),
        );
        assert!(
            clash
                .unwrap_err()
                .to_string()
                .contains("uses port 8080 more than once")
        );
    }

    #[test]
    fn options_that_only_work_together_are_rejected_apart() {
        for options in [
            toml::toml! { [options] tls_cert = "/tls/a.crt" },
            toml::toml! { [options] tls_key = "/tls/a.key" },
        ] {
            let partial = expand(
                "events",
                "nats",
                "2",
                options,
                Extras {
                    init: &[],
                    port: None,
                    labels: &labels(),
                    imported: false,
                },
                Path::new("/p/mise.toml"),
                Path::new("/p"),
            );
            assert!(partial.unwrap_err().to_string().contains("also requires"));
        }
        let both = expand(
            "events",
            "nats",
            "2",
            toml::toml! { [options] tls_cert = "/tls/a.crt" tls_key = "/tls/a.key" },
            Extras {
                init: &[],
                port: None,
                labels: &labels(),
                imported: false,
            },
            Path::new("/p/mise.toml"),
            Path::new("/p"),
        )
        .unwrap();
        assert!(both.table["run"].as_str().unwrap().contains("--tlskey"));
    }

    #[test]
    fn a_persistent_datastore_requires_its_connection_uri() {
        let missing = expand(
            "authz",
            "spicedb",
            "1",
            toml::toml! { [options] datastore_engine = "cockroachdb" },
            Extras {
                init: &[],
                port: None,
                labels: &labels(),
                imported: false,
            },
            Path::new("/p/mise.toml"),
            Path::new("/p"),
        );
        assert!(
            missing
                .unwrap_err()
                .to_string()
                .contains("also requires \"datastore_uri\"")
        );
        // The reverse is just as wrong: the memory engine ignores a connection URI,
        // so the daemon would quietly serve from an ephemeral store instead.
        let engineless = expand(
            "authz",
            "spicedb",
            "1",
            toml::toml! { [options] datastore_uri = "postgresql://root@127.0.0.1:26257/spicedb?sslmode=disable" },
            Extras {
                init: &[],
                port: None,
                labels: &labels(),
                imported: false,
            },
            Path::new("/p/mise.toml"),
            Path::new("/p"),
        );
        assert!(
            engineless
                .unwrap_err()
                .to_string()
                .contains("also requires \"datastore_engine\"")
        );
        // A certificate authority without a certificate enables no TLS at all.
        let ca_only = expand(
            "events",
            "nats",
            "2",
            toml::toml! { [options] tls_ca = "/tls/ca.crt" },
            Extras {
                init: &[],
                port: None,
                labels: &labels(),
                imported: false,
            },
            Path::new("/p/mise.toml"),
            Path::new("/p"),
        );
        assert!(
            ca_only
                .unwrap_err()
                .to_string()
                .contains("also requires \"tls_cert\"")
        );
        // The in-memory default needs no URI.
        assert!(
            !render("spicedb", toml::Table::new()).table["run"]
                .as_str()
                .unwrap()
                .contains("--datastore-conn-uri")
        );
    }

    #[test]
    fn steps_touching_external_state_repeat_on_every_start() {
        // SpiceDB keeps its schema in the datastore, so its migration cannot be a
        // one-time step keyed on a local marker.
        let spicedb = preset("spicedb").unwrap();
        assert!(spicedb.init.steps.iter().all(|step| step.always));
        // Steps that build local data must not repeat over published data.
        for name in ["postgres", "cockroachdb"] {
            let preset = preset(name).unwrap();
            assert!(
                preset.init.steps.iter().all(|step| !step.always),
                "{name} repeats a data-initialization step"
            );
        }
        // A repeating step runs without the ephemeral server, so the two cannot mix.
        for (name, _) in PRESETS {
            let preset = preset(name).unwrap();
            if preset.init.server.is_some() {
                assert!(preset.init.steps.iter().all(|step| !step.always));
            }
        }
    }

    #[test]
    fn initialization_ports_differ_from_each_other() {
        let (first, second) = free_ports().unwrap();
        assert_ne!(first, second);
    }

    #[test]
    #[cfg(unix)]
    fn abandoning_a_server_respects_the_callers_deadline() {
        // A server that ignores SIGTERM would otherwise hold its own grace period,
        // outside the budget the retry loop is trying to keep.
        let child = Command::new("sh")
            .args(["-c", "trap '' TERM; sleep 60"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let guard = ServerGuard::new(child);
        let start = Instant::now();
        guard.stop_by(Instant::now());
        assert!(start.elapsed() < SHUTDOWN_GRACE, "{:?}", start.elapsed());
    }

    /// A preset whose init steps only touch files, so the whole step machinery can
    /// run in a test without installing a database.
    #[cfg(unix)]
    fn stepping_preset() -> Preset {
        toml::from_str(
            r#"
binary = "sh"
description = "test double"
port = 1234
tool = "sh"

[version]
pattern = 'v(\d+)\.'

[options]
databases = { type = "list", default = [], pattern = '^[a-z]+$' }
quiet = false

[[init.steps]]
for_each = "databases"
run = ["sh", "-c", "echo {{ item }} >> {{ data }}/created"]

[[init.steps]]
run = ["sh", "-c", "cat >> {{ data }}/piped"]
stdin = "hello {{ port }}"

[[init.steps]]
always = true
when = "{% if not quiet %}true{% endif %}"
run = ["sh", "-c", "echo tick >> {{ data }}/ticks"]

[daemon]
run = "sh"

[exports]
"#,
        )
        .unwrap()
    }

    #[cfg(unix)]
    fn stepping_context(preset: &Preset, overrides: toml::Table) -> InitContext {
        InitContext {
            port: preset.port,
            ports: preset.ports.clone(),
            options: resolve_options(
                preset,
                "test",
                Some(&toml::Value::Table(overrides)),
                Path::new("/p"),
            )
            .unwrap(),
        }
    }

    #[cfg(unix)]
    fn lines(dir: &Path, name: &str) -> Vec<String> {
        std::fs::read_to_string(dir.join(name))
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }

    #[test]
    #[cfg(unix)]
    fn init_steps_iterate_take_stdin_and_repeat_only_when_marked() {
        let preset = stepping_preset();
        let init = stepping_context(&preset, toml::toml! { databases = ["alpha", "beta"] });
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path();

        run_steps(&preset, &init, data, false).unwrap();
        // for_each runs the step once per entry, in order.
        assert_eq!(lines(data, "created"), ["alpha", "beta"]);
        // stdin reaches the process, rendered with the same context.
        assert_eq!(lines(data, "piped"), ["hello 1234"]);
        assert_eq!(lines(data, "ticks"), ["tick"]);

        // A rerun over published data repeats only the steps marked always.
        run_steps(&preset, &init, data, true).unwrap();
        assert_eq!(lines(data, "created"), ["alpha", "beta"]);
        assert_eq!(lines(data, "ticks"), ["tick", "tick"]);
    }

    #[test]
    #[cfg(unix)]
    fn a_repeating_step_still_honours_its_condition() {
        let preset = stepping_preset();
        let init = stepping_context(&preset, toml::toml! { quiet = true });
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path();
        run_steps(&preset, &init, data, true).unwrap();
        assert!(
            lines(data, "ticks").is_empty(),
            "a false `when` must skip an always step"
        );
    }

    #[test]
    fn an_ignored_option_is_reported_only_when_the_user_asked_for_it() {
        let nats = preset("nats").unwrap();
        let both = toml::Value::Table(toml::toml! { config = "n.conf" jetstream = true });
        assert_eq!(
            ignored_options(&nats, Some(&both)).unwrap(),
            [(String::from("jetstream"), String::from("config"))]
        );
        // A config alone says nothing about JetStream, so there is nothing to warn
        // about; `__init` rebuilds a full table and must not trip this either.
        let config_only = toml::Value::Table(toml::toml! { config = "n.conf" });
        assert!(
            ignored_options(&nats, Some(&config_only))
                .unwrap()
                .is_empty()
        );
        let jetstream_only = toml::Value::Table(toml::toml! { jetstream = true });
        assert!(
            ignored_options(&nats, Some(&jetstream_only))
                .unwrap()
                .is_empty()
        );
        assert!(ignored_options(&nats, None).unwrap().is_empty());
    }

    #[test]
    fn a_database_entry_can_carry_its_primary_region() {
        let daemon = expand(
            "crdb",
            "cockroachdb",
            "26",
            toml::toml! {
                [options]
                databases = ["entirecore=us-east-2", "spicedb"]
                locality = "region=us-east-2"
            },
            Extras {
                init: &[],
                port: None,
                labels: &labels(),
                imported: false,
            },
            Path::new("/p/mise.toml"),
            Path::new("/p"),
        )
        .unwrap();
        // The node must declare the region for a database to be able to use it.
        let run = daemon.table["run"].as_str().unwrap();
        assert!(run.contains("--locality='region=us-east-2'"), "{run}");
        assert_eq!(
            init_json(&daemon)["databases"],
            serde_json::json!(["entirecore=us-east-2", "spicedb"])
        );
        // A region the node never declared does not exist, so it fails here rather
        // than as a failed statement on the daemon's first start.
        for options in [
            toml::toml! { [options] databases = ["app=us-west-1"] locality = "region=us-east-2" },
            toml::toml! { [options] databases = ["app=us-east-2"] },
            // A tier that is not the region declares no region, however much its
            // value looks like one.
            toml::toml! { [options] databases = ["app=us-east-2"] locality = "az=us-east-2" },
        ] {
            let undeclared = expand(
                "crdb",
                "cockroachdb",
                "26",
                options,
                Extras {
                    init: &[],
                    port: None,
                    labels: &labels(),
                    imported: false,
                },
                Path::new("/p/mise.toml"),
                Path::new("/p"),
            );
            assert!(
                undeclared
                    .unwrap_err()
                    .to_string()
                    .contains("does not declare")
            );
        }
        // A zone alongside the region still resolves the region.
        assert!(
            expand(
                "crdb",
                "cockroachdb",
                "26",
                toml::toml! { [options] databases = ["app=us-east-2"] locality = "region=us-east-2,zone=a" },
                Extras {
                    init: &[],
                    port: None,
                    labels: &labels(),
                    imported: false,
                },
                Path::new("/p/mise.toml"),
                Path::new("/p"),
            )
            .is_ok()
        );
        // A region is optional per entry, and still validated.
        let bad = expand(
            "crdb",
            "cockroachdb",
            "26",
            toml::toml! { [options] databases = ["app=bad region"] },
            Extras {
                init: &[],
                port: None,
                labels: &labels(),
                imported: false,
            },
            Path::new("/p/mise.toml"),
            Path::new("/p"),
        );
        assert!(bad.unwrap_err().to_string().contains("must match"));
    }

    #[test]
    #[cfg(unix)]
    fn init_steps_split_entries_and_skip_the_ones_their_condition_excludes() {
        let preset: Preset = toml::from_str(
            r#"
binary = "sh"
port = 1234
tool = "sh"

[version]
pattern = 'v(\d+)\.'

[options]
databases = { type = "list", default = [], pattern = '^[a-z]+(=[a-z-]+)?$' }

[[init.steps]]
for_each = "databases"
run = ["sh", "-c", "echo {{ item_key }} >> {{ data }}/made"]

[[init.steps]]
for_each = "databases"
when = "{% if item_value %}true{% endif %}"
run = ["sh", "-c", "echo {{ item_key }}:{{ item_value }} >> {{ data }}/regions"]

[daemon]
run = "sh"

[exports]
"#,
        )
        .unwrap();
        let init = InitContext {
            port: preset.port,
            ports: preset.ports.clone(),
            options: resolve_options(
                &preset,
                "test",
                Some(&toml::Value::Table(
                    toml::toml! { databases = ["alpha=east", "beta"] },
                )),
                Path::new("/p"),
            )
            .unwrap(),
        };
        let dir = tempfile::tempdir().unwrap();
        run_steps(&preset, &init, dir.path(), false).unwrap();
        // Every entry is created by its name alone.
        assert_eq!(lines(dir.path(), "made"), ["alpha", "beta"]);
        // Only the entry carrying a value takes the second step.
        assert_eq!(lines(dir.path(), "regions"), ["alpha:east"]);
    }

    #[test]
    #[cfg(unix)]
    fn a_step_that_never_finishes_is_given_up_on() {
        let mut child = Command::new("sh")
            .args(["-c", "sleep 60"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let start = Instant::now();
        // `None` is the signal the caller turns into an error naming the command.
        assert!(
            wait_until(&mut child, Instant::now() + Duration::from_millis(200))
                .unwrap()
                .is_none()
        );
        assert!(start.elapsed() < Duration::from_secs(5));
        // A step that finishes on its own still reports its status.
        let mut ok = Command::new("true").spawn().unwrap();
        let status = wait_until(&mut ok, Instant::now() + Duration::from_secs(30))
            .unwrap()
            .expect("a finished process reports a status");
        assert!(status.success());
    }

    #[test]
    #[cfg(unix)]
    fn reaping_a_killed_process_is_bounded() {
        let mut child = Command::new("sh")
            .args(["-c", "trap '' TERM; sleep 60"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let start = Instant::now();
        kill_and_reap(&mut child);
        assert!(start.elapsed() < REAP_GRACE, "{:?}", start.elapsed());
        // A fixture that ignores SIGTERM still dies on SIGKILL, so this one must
        // have been reaped; `Ok(None)` would mean it was abandoned still running.
        assert!(matches!(child.try_wait(), Ok(Some(_))));
    }

    #[test]
    #[cfg(unix)]
    fn a_hung_readiness_probe_stops_at_the_deadline() {
        let argv = vec!["sleep".to_string(), "60".to_string()];
        let deadline = Instant::now() + Duration::from_millis(200);
        let start = Instant::now();
        assert!(!probe(&argv, deadline));
        assert!(start.elapsed() < Duration::from_secs(5));
        // A probe that exits on its own still reports what it found.
        let far = Instant::now() + Duration::from_secs(30);
        assert!(probe(&["true".to_string()], far));
        assert!(!probe(&["false".to_string()], far));
    }

    #[test]
    fn a_legacy_database_argument_is_used_only_where_it_means_something() {
        // Old configurations passed a database to every preset, including those
        // that never had the option; handing it back would fail as unknown.
        assert_eq!(
            legacy_values(&preset("postgres").unwrap(), Some("integration")),
            r#"{"database":"integration"}"#
        );
        assert_eq!(
            legacy_values(&preset("redis").unwrap(), Some("postgres")),
            "{}"
        );
        assert_eq!(legacy_values(&preset("postgres").unwrap(), None), "{}");
        // The value still has to survive the preset's own validation.
        let preset = preset("postgres").unwrap();
        let values = legacy_values(&preset, Some("integration"));
        assert!(init_context(&preset, "postgres", &values).is_ok());
    }

    #[test]
    fn initialization_context_rejects_values_the_preset_did_not_declare() {
        let preset = preset("postgres").unwrap();
        let init = init_context(&preset, "postgres", r#"{"port":5433,"database":"app"}"#).unwrap();
        assert_eq!(init.port, 5433);
        assert!(matches!(&init.options["database"], OptionValue::Text(t) if t == "app"));
        assert!(init_context(&preset, "postgres", r#"{"nope":1}"#).is_err());
        assert!(init_context(&preset, "postgres", r#"{"database":"a;b"}"#).is_err());
        assert!(init_context(&preset, "postgres", "[]").is_err());
    }
}
