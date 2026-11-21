//! `schema/mise.json` describes `[daemons]` and `[daemon_providers]` from the `Daemon*Decl`
//! types in mise-config. Nothing parses into those types: this crate reads a declaration key
//! by key. These tests fail when the two disagree about a key or an enumerated value.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;

use indexmap::IndexMap;
use mise_config::daemons::{
    DaemonCommandDecl, DaemonCommandNamedPortAuto, DaemonContainerDecl, DaemonContainerPortTable,
    DaemonContainerRuntime, DaemonNamedPortAuto, DaemonPortAuto, DaemonPresetDecl,
    DaemonProjectDecl, DaemonProviderDecl, DaemonProviderRefDecl, DaemonProxyTls, DaemonTaskDecl,
};
use mise_config::de::struct_fields;
use mise_config::schema::{property_enum, type_enum};
use strum::IntoEnumIterator;

use crate::host::{ConfigFileView, ConfigMap};
use crate::{Daemon, container, presets, providers, urls};

/// The non-test source of every module that reads a daemon declaration.
const SOURCES: [&str; 6] = [
    include_str!("lib.rs"),
    include_str!("container.rs"),
    include_str!("ports.rs"),
    include_str!("presets.rs"),
    include_str!("providers.rs"),
    include_str!("urls.rs"),
];

/// Keys this crate reads from a declaration that belong to pitchfork, which receives them with
/// every other key the declaration types leave out. `id` is the key of a `depends` entry.
const PITCHFORK_KEYS: [&str; 4] = ["depends", "env", "id", "stop_signal"];

/// A key this crate reads on a form whose declaration type leaves it out.
// docs/daemons.md lists `mise` for custom, task and container daemons only. A preset reads it
// too: `mise = false` leaves its probe commands unwrapped.
const UNDECLARED: [(&str, &str); 1] = [("preset", "mise")];

fn fields<T: serde::de::DeserializeOwned>() -> BTreeSet<&'static str> {
    struct_fields::<T>().iter().copied().collect()
}

fn strings(values: &[&str]) -> BTreeSet<String> {
    values.iter().map(|value| value.to_string()).collect()
}

/// Every key a declaration type names.
fn declared_keys() -> BTreeSet<&'static str> {
    let mut keys = fields::<DaemonCommandDecl>();
    keys.extend(fields::<DaemonTaskDecl>());
    keys.extend(fields::<DaemonPresetDecl>());
    keys.extend(fields::<DaemonContainerDecl>());
    keys.extend(fields::<DaemonProjectDecl>());
    keys.extend(fields::<DaemonProviderRefDecl>());
    keys.extend(fields::<DaemonProviderDecl>());
    keys
}

/// Every key the declaration types name inside a `port` or `ports` table.
fn port_table_keys() -> BTreeSet<&'static str> {
    let mut keys = fields::<DaemonPortAuto>();
    keys.extend(fields::<DaemonNamedPortAuto>());
    keys.extend(fields::<DaemonCommandNamedPortAuto>());
    keys.extend(fields::<DaemonContainerPortTable>());
    keys
}

/// The keys the source reads from a declaration by a literal name: `table.get("port")`,
/// `take_string(&mut overrides, "tool")` and the like.
fn keys_read_by_name() -> BTreeSet<String> {
    let table = r"\b(?:table|overrides|declaration)";
    let patterns = [
        format!(r#"{table}\s*\.\s*(?:get|get_mut|remove|contains_key|entry)\(\s*"([a-z_]+)""#),
        format!(r#"\b(?:take_string|list)\((?:name, |&mut )?{table}, "([a-z_]+)"\)"#),
    ];
    let mut keys = BTreeSet::new();
    for source in SOURCES {
        let source = source
            .split_once("#[cfg(test)]\nmod tests")
            .map_or(source, |(code, _)| code);
        for pattern in &patterns {
            let pattern = regex::Regex::new(pattern).unwrap();
            keys.extend(
                pattern
                    .captures_iter(source)
                    .map(|found| found[1].to_string()),
            );
        }
    }
    keys
}

/// A value this crate accepts for `key` on a daemon of `form`.
fn sample(form: &str, key: &str) -> toml::Value {
    let text = match (form, key) {
        // a preset's named ports are the ones the preset defines
        ("preset", "ports") => "v = { http_port = 8090 }",
        (_, "args") => "v = ['--verbose']",
        (_, "command") => "v = 'redis-server'",
        (_, "container_port") => "v = 6379",
        (_, "data_dir") => "v = 'data'",
        (_, "data_mount") => "v = '/data'",
        (_, "image") => "v = 'redis:8'",
        (_, "init") => "v = ['mise run migrate']",
        (_, "mise") => "v = false",
        (_, "name") => "v = 'db'",
        (_, "options") => "v = { database = 'app' }",
        (_, "port") => "v = { auto = true, base = 5000 }",
        (_, "ports") => "v = { metrics = 9100 }",
        (_, "preset") => "v = 'redis'",
        (_, "project") => "v = '../other'",
        (_, "provider") => "v = 'shared'",
        (_, "proxy") => "v = 'web'",
        (_, "proxy_idle_timeout") => "v = '30m'",
        (_, "proxy_tls") => "v = 'passthrough'",
        (_, "resource") => "v = 'app_db'",
        (_, "run") => "v = 'exec other-server'",
        (_, "runtime") => "v = 'podman'",
        (_, "runtime_args") => "v = ['--init']",
        (_, "task") => "v = 'serve'",
        (_, "tool") => "v = 'github:example/server'",
        (_, "version") => "v = '8'",
        (_, "volumes") => "v = ['cache:/cache']",
        _ => panic!("add a sample value for the daemon key {key:?}"),
    };
    toml::from_str::<toml::Table>(text)
        .unwrap()
        .remove("v")
        .unwrap()
}

/// The keys `key` needs beside it on a daemon of `form`.
fn companions(form: &str, key: &str) -> &'static str {
    match (form, key) {
        // a preset is not proxied unless it says so
        ("preset", "proxy_tls") => "proxy = 'db'",
        ("container", "data_dir") => "data_mount = '/data'",
        _ => "",
    }
}

fn load_one(table: &toml::Table) -> eyre::Result<Daemon> {
    let path = PathBuf::from("/project/mise.toml");
    let body = toml::to_string(&toml::Table::from_iter([(
        "daemons".to_string(),
        toml::Value::Table(toml::Table::from_iter([(
            "app".to_string(),
            toml::Value::Table(table.clone()),
        )])),
    )]))?;
    let file: Arc<dyn ConfigFileView> =
        Arc::new(crate::test_host::DeclaredFile::parse(&body, &path)?);
    let config: ConfigMap = [(path, file)].into_iter().collect();
    let mut set = crate::load(&config)?;
    set.daemons
        .shift_remove("app")
        .ok_or_else(|| eyre::eyre!("the daemon did not load"))
}

#[derive(Debug, PartialEq)]
enum Outcome {
    /// Loading fails with this error.
    Rejected(String),
    /// The value reaches pitchfork as written and changes nothing else.
    Forwarded,
    /// mise consumes the key, rewrites it, or builds the daemon differently because of it.
    Read,
}

/// What this crate does with `key` on the daemon `base` describes.
fn outcome(form: &str, base: &toml::Table, key: &str) -> Outcome {
    let value = sample(form, key);
    let mut base = base.clone();
    base.extend(toml::from_str::<toml::Table>(companions(form, key)).unwrap());
    let mut with = base.clone();
    with.insert(key.to_string(), value.clone());
    let mut with = match load_one(&with) {
        Ok(daemon) => daemon,
        Err(err) => return Outcome::Rejected(format!("{err:#}")),
    };
    let mut without = base.clone();
    without.remove(key);
    // a daemon that does not load without the key needs it
    let Ok(mut without) = load_one(&without) else {
        return Outcome::Read;
    };
    let forwarded = with.table.remove(key);
    without.table.remove(key);
    if forwarded == Some(value) && format!("{with:?}") == format!("{without:?}") {
        Outcome::Forwarded
    } else {
        Outcome::Read
    }
}

/// A custom, task, preset or container table is open: a key mise does not read goes to
/// pitchfork. So a key mise reads on a form must be declared for that form, and a key declared
/// for a form must load on it.
#[test]
fn open_forms_declare_the_keys_mise_reads() {
    let forms: [(&str, &str, BTreeSet<&str>); 4] = [
        (
            "custom",
            "run = 'exec server'\nport = 4000\nready_cmd = 'true'",
            fields::<DaemonCommandDecl>(),
        ),
        (
            "task",
            "task = 'serve'\nport = 4000\nready_cmd = 'true'",
            fields::<DaemonTaskDecl>(),
        ),
        (
            "preset",
            "preset = 'cockroachdb'\nversion = '25'",
            fields::<DaemonPresetDecl>(),
        ),
        (
            "container",
            "image = 'redis:8'\nport = 4000\nready_cmd = 'true'",
            fields::<DaemonContainerDecl>(),
        ),
    ];
    let keys: BTreeSet<String> = declared_keys()
        .into_iter()
        .map(str::to_string)
        .chain(keys_read_by_name())
        .filter(|key| !PITCHFORK_KEYS.contains(&key.as_str()))
        .filter(|key| !port_table_keys().contains(key.as_str()))
        .collect();
    let mut problems = vec![];
    for (form, base, declared) in forms {
        let base: toml::Table = toml::from_str(base).unwrap();
        load_one(&base).unwrap_or_else(|err| panic!("the {form} daemon must load: {err:#}"));
        for key in &keys {
            let outcome = outcome(form, &base, key);
            let declared = declared.contains(key.as_str());
            let undeclared = UNDECLARED.contains(&(form, key.as_str()));
            match outcome {
                Outcome::Read if !declared && !undeclared => problems.push(format!(
                    "mise reads `{key}` on a {form} daemon, but its declaration type has no such field"
                )),
                Outcome::Rejected(err) if declared => problems.push(format!(
                    "the declaration type of a {form} daemon has `{key}`, but mise rejects it: {err}"
                )),
                Outcome::Forwarded | Outcome::Rejected(_) if undeclared => problems.push(format!(
                    "UNDECLARED lists `{key}` on a {form} daemon, but mise does not read it"
                )),
                _ => {}
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// A `port` or `ports.<name>` table is closed, and each form has its own declaration type for
/// it. Every key any of them names loads where its type declares it and is rejected elsewhere.
#[test]
fn port_tables_take_the_declared_keys() {
    let custom = "run = 'exec server'\nready_cmd = 'true'";
    let task = "task = 'serve'\nready_cmd = 'true'";
    let preset = "preset = 'cockroachdb'\nversion = '25'";
    let container = "image = 'redis:8'\nready_cmd = 'true'";
    let per_worktree = "auto = true\nbase = 27000";
    // (where the table is, the daemon around it, the keys a valid table starts from, its type)
    let shapes: [(&str, &str, &str, &str, BTreeSet<&str>); 8] = [
        (
            "custom port",
            custom,
            "port",
            per_worktree,
            fields::<DaemonPortAuto>(),
        ),
        (
            "task port",
            task,
            "port",
            per_worktree,
            fields::<DaemonPortAuto>(),
        ),
        (
            "preset port",
            preset,
            "port",
            per_worktree,
            fields::<DaemonPortAuto>(),
        ),
        (
            "container port",
            container,
            "port",
            per_worktree,
            fields::<DaemonPortAuto>(),
        ),
        (
            "custom ports.<name>",
            "run = 'exec server'\nport = 4000\nready_cmd = 'true'",
            "ports.metrics",
            per_worktree,
            fields::<DaemonCommandNamedPortAuto>(),
        ),
        (
            "task ports.<name>",
            "task = 'serve'\nport = 4000\nready_cmd = 'true'",
            "ports.metrics",
            per_worktree,
            fields::<DaemonCommandNamedPortAuto>(),
        ),
        (
            "preset ports.<name>",
            preset,
            "ports.http_port",
            "auto = true",
            fields::<DaemonNamedPortAuto>(),
        ),
        (
            "container ports.<name>",
            "image = 'redis:8'\nport = 4000\nready_cmd = 'true'",
            "ports.metrics",
            "container = 8080",
            fields::<DaemonContainerPortTable>(),
        ),
    ];
    let mut problems = vec![];
    for (shape, daemon, at, start, declared) in shapes {
        for key in port_table_keys() {
            let mut table: toml::Table = toml::from_str(start).unwrap();
            // a per-worktree host port is asked for with `auto`
            if shape.starts_with("container ports") && matches!(key, "base" | "stride") {
                table.insert("auto".into(), true.into());
            }
            let value = match key {
                "auto" => true.into(),
                "base" => 27000.into(),
                "container" => 8080.into(),
                "host" => 8081.into(),
                "stride" => 2.into(),
                _ => panic!("add a sample value for the port key {key:?}"),
            };
            table.insert(key.into(), value);
            let mut daemon: toml::Table = toml::from_str(daemon).unwrap();
            let table = toml::Value::Table(table);
            match at.split_once('.') {
                Some((ports, name)) => {
                    let named = toml::Table::from_iter([(name.to_string(), table)]);
                    daemon.insert(ports.into(), toml::Value::Table(named));
                }
                None => {
                    daemon.insert(at.into(), table);
                }
            }
            match (load_one(&daemon), declared.contains(key)) {
                (Err(err), true) => problems.push(format!(
                    "the declaration type of a {shape} table has `{key}`, but mise rejects it: {err:#}"
                )),
                (Ok(_), false) => problems.push(format!(
                    "mise reads `{key}` in a {shape} table, but its declaration type has no such field"
                )),
                _ => {}
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// A key the source reads by name is declared for some form, or is one of pitchfork's.
#[test]
fn keys_read_by_name_are_declared() {
    let read = keys_read_by_name();
    // the scan finds the reads: these three are on every path a declaration takes
    for key in ["port", "image", "tool"] {
        assert!(read.contains(key), "the scan no longer finds `{key}`");
    }
    let mut declared = declared_keys();
    declared.extend(port_table_keys());
    let unknown: Vec<&String> = read
        .iter()
        .filter(|key| !declared.contains(key.as_str()) && !PITCHFORK_KEYS.contains(&key.as_str()))
        .collect();
    assert!(
        unknown.is_empty(),
        "mise-daemons reads {unknown:?} from a declaration, but no Daemon*Decl type declares them"
    );
    let stale: Vec<&str> = PITCHFORK_KEYS
        .into_iter()
        .filter(|key| !read.contains(*key) || declared.contains(key))
        .collect();
    assert!(
        stale.is_empty(),
        "PITCHFORK_KEYS lists {stale:?} needlessly"
    );
}

/// A reference to another project's daemon takes the declared keys and no others.
#[test]
fn project_keys_match_the_declaration() {
    let declared = fields::<DaemonProjectDecl>();
    for key in declared_keys() {
        let mut table = toml::Table::new();
        table.insert("project".into(), sample("project", "project"));
        table.insert(key.into(), sample("project", key));
        assert_eq!(
            crate::parse_import("app", table).is_ok(),
            declared.contains(key),
            "{key}"
        );
    }
}

/// A reference to a shared provider takes the declared keys and no others.
#[test]
fn provider_reference_keys_match_the_declaration() {
    let declared = fields::<DaemonProviderRefDecl>();
    for key in declared_keys() {
        let mut table = toml::Table::new();
        table.insert("provider".into(), sample("provider", "provider"));
        table.insert(key.into(), sample("provider", key));
        // no provider is defined, so a table with only declared keys fails on the lookup
        let err = providers::binding(
            &IndexMap::new(),
            "app",
            table,
            PathBuf::from("/project/mise.toml"),
            PathBuf::from("/project"),
        )
        .unwrap_err()
        .to_string();
        assert_eq!(
            err.contains("unknown daemon provider"),
            declared.contains(key),
            "{key}: {err}"
        );
    }
}

#[test]
fn provider_keys_match_the_declaration() {
    let read: BTreeSet<&str> = providers::PROVIDER_KEYS
        .into_iter()
        .chain(["preset", "version"])
        .collect();
    assert_eq!(read, fields::<DaemonProviderDecl>());
}

#[test]
fn enumerated_values_match_the_declaration() {
    let presets: BTreeSet<String> = presets::PRESETS
        .iter()
        .map(|(name, _)| name.to_string())
        .collect();
    assert_eq!(property_enum::<DaemonPresetDecl>("preset"), presets);

    let provider_presets = strings(&providers::PROVIDER_PRESETS);
    assert_eq!(
        property_enum::<DaemonProviderDecl>("preset"),
        provider_presets
    );
    assert!(provider_presets.is_subset(&presets));

    assert_eq!(
        type_enum::<DaemonProxyTls>(),
        strings(&urls::PROXY_TLS_MODES)
    );

    // every declared runtime parses, and every runtime the parser knows is declared once
    let runtimes: Vec<container::Runtime> = type_enum::<DaemonContainerRuntime>()
        .iter()
        .map(|name| container::Runtime::parse("app", &name.as_str().into()).unwrap())
        .collect();
    for runtime in container::Runtime::iter() {
        assert_eq!(
            runtimes.iter().filter(|parsed| **parsed == runtime).count(),
            1,
            "{runtime:?}"
        );
    }
}
