//! Embeds the daemon presets in `presets/` after checking each one is well formed.

use std::path::{Path, PathBuf};
use std::{env, fs};

use eyre::{Result, eyre};

fn main() -> Result<()> {
    codegen_daemon_presets()
}

fn codegen_daemon_presets() -> Result<()> {
    let dir = Path::new("presets");
    println!("cargo:rerun-if-changed={}", dir.display());
    let mut paths = fs::read_dir(dir)?
        .map(|e| e.map(|e| e.path()))
        .collect::<std::io::Result<Vec<_>>>()?;
    paths.sort();
    let mut code = String::from("&[\n");
    for path in paths {
        if path.extension().and_then(|e| e.to_str()) != Some("toml") {
            continue;
        }
        println!("cargo:rerun-if-changed={}", path.display());
        let content = fs::read_to_string(&path)?;
        let value: toml::Value = toml::from_str(&content)?;
        for key in ["tool", "binary", "description"] {
            if value.get(key).and_then(toml::Value::as_str).is_none() {
                return Err(eyre!("{}: missing {key}", path.display()));
            }
        }
        if value
            .get("port")
            .and_then(toml::Value::as_integer)
            .is_none()
            || value
                .get("daemon")
                .and_then(|v| v.get("run"))
                .and_then(toml::Value::as_str)
                .is_none()
        {
            return Err(eyre!("{}: missing port or daemon.run", path.display()));
        }
        // Match the runtime Preset shape: a successful registry build must not
        // embed a preset that fails deserialization when first selected.
        let valid_port = value
            .get("port")
            .and_then(toml::Value::as_integer)
            .is_some_and(|port| (1..=65535).contains(&port));
        let valid_options = value.get("options").is_some_and(toml::Value::is_table);
        let valid_exports = value
            .get("exports")
            .and_then(toml::Value::as_table)
            .is_some_and(|exports| exports.values().all(toml::Value::is_str));
        if !valid_port || !valid_options || !valid_exports {
            return Err(eyre!(
                "{}: invalid preset port, options, or exports",
                path.display()
            ));
        }
        validate_daemon_preset(&path, &value)?;
        code.push_str(&format!(
            "({:?}, {}),\n",
            path.file_stem().unwrap().to_string_lossy(),
            raw_string_literal(&content)
        ));
    }
    code.push(']');
    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("daemon_presets.rs"),
        code,
    )?;
    Ok(())
}

/// Validate the declarative parts of a daemon preset: named ports, version
/// detection, typed options, and initialization steps. A preset that passes here
/// deserializes into the runtime `Preset` struct.
fn validate_daemon_preset(path: &Path, value: &toml::Value) -> Result<()> {
    let bad = |msg: &str| eyre!("{}: {msg}", path.display());
    let version = value
        .get("version")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| bad("missing [version]"))?;
    let pattern = version
        .get("pattern")
        .and_then(toml::Value::as_str)
        .ok_or_else(|| bad("missing version.pattern"))?;
    let re =
        regex::Regex::new(pattern).map_err(|e| bad(&format!("invalid version.pattern: {e}")))?;
    if re.captures_len() != 2 {
        return Err(bad("version.pattern needs exactly one capture group"));
    }
    if !is_string_list(version.get("args").unwrap_or(&toml::Value::Array(vec![]))) {
        return Err(bad("version.args must be a list of strings"));
    }
    if let Some(ports) = value.get("ports") {
        let ports = ports
            .as_table()
            .ok_or_else(|| bad("[ports] must be a table"))?;
        for (name, port) in ports {
            if RESERVED_TEMPLATE_NAMES.contains(&name.as_str()) {
                return Err(bad(&format!(
                    "named port {name:?} shadows a reserved template name"
                )));
            }
            if !port.as_integer().is_some_and(|p| (1..=65535).contains(&p)) {
                return Err(bad(&format!("invalid named port {name:?}")));
            }
        }
    }
    if let Some(file) = value.get("data_version_file")
        && !file.is_str()
    {
        return Err(bad("data_version_file must be a string"));
    }
    for (name, option) in value
        .get("options")
        .and_then(toml::Value::as_table)
        .into_iter()
        .flatten()
    {
        // Options share one template context with the ports and the names the
        // engine binds, and are inserted last, so a collision would silently
        // shadow the value a template expects.
        if RESERVED_TEMPLATE_NAMES.contains(&name.as_str())
            || value
                .get("ports")
                .and_then(|p| p.get(name.as_str()))
                .is_some()
        {
            return Err(bad(&format!(
                "option {name:?} shadows a reserved template name"
            )));
        }
        match option {
            toml::Value::Table(spec) => {
                let kind = spec
                    .get("type")
                    .and_then(toml::Value::as_str)
                    .ok_or_else(|| bad(&format!("option {name:?} needs a type")))?;
                if !["string", "path", "list", "int", "bool"].contains(&kind) {
                    return Err(bad(&format!("option {name:?} has unknown type {kind:?}")));
                }
                let default = spec
                    .get("default")
                    .ok_or_else(|| bad(&format!("option {name:?} needs a default")))?;
                let matches = match kind {
                    "string" | "path" => default.is_str(),
                    "list" => is_string_list(default),
                    "int" => default.is_integer(),
                    _ => default.is_bool(),
                };
                if !matches {
                    return Err(bad(&format!("option {name:?} default is not a {kind}")));
                }
                if let Some(pattern) = spec.get("pattern") {
                    let pattern = pattern
                        .as_str()
                        .ok_or_else(|| bad(&format!("option {name:?} pattern must be a string")))?;
                    regex::Regex::new(pattern).map_err(|e| {
                        bad(&format!("option {name:?} has an invalid pattern: {e}"))
                    })?;
                }
                if let Some(entry) = spec.get("entry_value_in") {
                    let other = entry.get("option").and_then(toml::Value::as_str);
                    let tier = entry.get("key").and_then(toml::Value::as_str);
                    let (Some(other), Some(_)) = (other, tier) else {
                        return Err(bad(&format!(
                            "option {name:?} entry_value_in needs an option and a key"
                        )));
                    };
                    if value.get("options").and_then(|o| o.get(other)).is_none() {
                        return Err(bad(&format!(
                            "option {name:?} entry_value_in names unknown option {other:?}"
                        )));
                    }
                }
                for key in ["requires", "ignored_with"] {
                    if let Some(names) = spec.get(key) {
                        if !is_string_list(names) {
                            return Err(bad(&format!("option {name:?} {key} must be strings")));
                        }
                        for other in names.as_array().into_iter().flatten() {
                            let other = other.as_str().unwrap_or_default();
                            if value.get("options").and_then(|o| o.get(other)).is_none() {
                                return Err(bad(&format!(
                                    "option {name:?} {key} names unknown option {other:?}"
                                )));
                            }
                        }
                    }
                }
            }
            toml::Value::String(_) | toml::Value::Integer(_) | toml::Value::Boolean(_) => {}
            toml::Value::Array(_) if is_string_list(option) => {}
            _ => return Err(bad(&format!("option {name:?} has an unsupported default"))),
        }
    }
    let init = match value.get("init") {
        Some(init) => init
            .as_table()
            .ok_or_else(|| bad("[init] must be a table"))?,
        None => return Ok(()),
    };
    if let Some(server) = init.get("server") {
        let server = server
            .as_table()
            .ok_or_else(|| bad("[init.server] must be a table"))?;
        for key in ["run", "ready"] {
            if !server.get(key).is_some_and(is_argv) {
                return Err(bad(&format!(
                    "init.server.{key} must be a non-empty list of strings"
                )));
            }
        }
    }
    let steps = match init.get("steps") {
        Some(steps) => steps
            .as_array()
            .ok_or_else(|| bad("init.steps must be an array of tables"))?
            .as_slice(),
        None => &[],
    };
    for step in steps {
        let step = step
            .as_table()
            .ok_or_else(|| bad("init steps must be tables"))?;
        if !step.get("run").is_some_and(is_argv) {
            return Err(bad("each init step needs a non-empty run list of strings"));
        }
        match step.get("always") {
            None => {}
            Some(toml::Value::Boolean(false)) => {}
            // A repeating step runs against already-published data, where the
            // ephemeral init server is not started and its ports do not exist.
            Some(toml::Value::Boolean(true)) if init.contains_key("server") => {
                return Err(bad(
                    "an init step cannot be always when the preset declares init.server",
                ));
            }
            Some(toml::Value::Boolean(true)) => {}
            Some(_) => return Err(bad("init step always must be a boolean")),
        }
        if let Some(key) = step.get("for_each") {
            let key = key
                .as_str()
                .ok_or_else(|| bad("init step for_each must be a string"))?;
            let list = value
                .get("options")
                .and_then(|o| o.get(key))
                .ok_or_else(|| bad(&format!("init step for_each {key:?} is not an option")))?;
            let kind = list
                .get("type")
                .and_then(toml::Value::as_str)
                .unwrap_or(if list.is_array() { "list" } else { "other" });
            if kind != "list" {
                return Err(bad(&format!(
                    "init step for_each {key:?} is not a list option"
                )));
            }
        }
    }
    Ok(())
}

fn is_string_list(value: &toml::Value) -> bool {
    value
        .as_array()
        .is_some_and(|items| items.iter().all(toml::Value::is_str))
}

/// Names the preset engine binds in the template context itself. Ports and
/// options are inserted after them, so either could silently shadow one.
const RESERVED_TEMPLATE_NAMES: &[&str] = &[
    "data",
    "port",
    "item",
    "item_key",
    "item_value",
    "init_port",
    "init_http_port",
    // Set from the daemon's proxy hostname when it has one.
    "host",
    "url",
];

/// A command the runtime will execute, so it must name a program.
fn is_argv(value: &toml::Value) -> bool {
    is_string_list(value) && value.as_array().is_some_and(|items| !items.is_empty())
}

fn raw_string_literal(s: &str) -> String {
    // Find the longest sequence of '#' characters following a '"' in the string
    let mut max_hashes = 0;
    let mut current_hashes = 0;
    let mut after_quote = false;

    for c in s.chars() {
        if after_quote {
            if c == '#' {
                current_hashes += 1;
                max_hashes = max_hashes.max(current_hashes);
            } else {
                after_quote = false;
                current_hashes = 0;
            }
        }
        if c == '"' {
            after_quote = true;
            current_hashes = 0;
        }
    }

    // Use one more '#' than the longest sequence found
    let hashes = "#".repeat(max_hashes + 1);
    format!("r{hashes}\"{s}\"{hashes}")
}
