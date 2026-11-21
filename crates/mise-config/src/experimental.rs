//! The config that mise refuses to act on unless `experimental = true`.
//!
//! Each [`Experimental`] below is used twice: the schema labels a key with it
//! (`#[schemars(transform = experimental::SECRETS.labels())]`, or [`Experimental::label`] where
//! the schema is written by hand), and the code that acts on that key names it in its check
//! (`Settings::ensure_experimental(experimental::SECRETS.what)` or
//! `Settings::experimental_enabled(&experimental::SECRETS)`). The tests at the end of this file
//! fail when one of the two is missing, and when a description says "experimental" on its own.
//!
//! The `[settings]` schema is not covered here: it takes its labels from `settings.toml`.

use schemars::Schema;
use schemars::transform::Transform;

/// What the schema puts in front of the description of an experimental key.
pub const LABEL: &str = "[experimental]";

/// Config that needs `experimental = true`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Experimental {
    /// The name of the constant, which is how the checks refer to it.
    pub name: &'static str,
    /// What `Settings::ensure_experimental` says is experimental.
    pub what: &'static str,
}

macro_rules! experimental {
    ($($(#[$doc:meta])* $name:ident = $what:literal;)*) => {
        $(
            $(#[$doc])*
            pub const $name: Experimental = Experimental {
                name: stringify!($name),
                what: $what,
            };
        )*

        /// Every experimental part of the config.
        pub const ALL: &[Experimental] = &[$($name),*];
    };
}

experimental! {
    /// `age` values in `[env]` and `[vars]`.
    AGE = "age encryption";
    /// `[bootstrap.settings]`.
    BOOTSTRAP_SETTINGS = "bootstrap settings plugins";
    /// `[daemons]`, `[daemon_providers]`, `[daemons_settings]`, `[daemon_groups]` and a task's
    /// `daemons`.
    DAEMONS = "mise daemons";
    /// `[deps]`.
    DEPS = "deps";
    /// `[oci]`.
    OCI = "mise oci build";
    /// `[secrets]` and a task's `secrets`.
    SECRETS = "mise secrets";
    /// Tools of the `spinel:` backend.
    SPINEL = "the spinel backend";
    /// A task's `cache` and `[task_config].cache`.
    TASK_CACHE = "task artifact caching";
    /// `[task_config].global_env` and `global_pass_through_env`.
    TASK_GLOBAL_ENV = "global task environment configuration";
    /// `[task_config].global_inputs` and `input_groups`.
    TASK_INPUT_GROUPS = "task input groups";
    /// A task's `pass_through_env`.
    TASK_PASS_THROUGH_ENV = "task environment pass-through";
}

impl Experimental {
    /// `description` with the experimental label in front.
    pub fn label(&self, description: &str) -> String {
        #[cfg(test)]
        tests::LABELLED.lock().unwrap().insert(self.name);
        format!("{LABEL} {description}")
    }

    /// Labels the description a doc comment gave the schema.
    pub fn labels(self) -> impl Transform {
        move |schema: &mut Schema| {
            let description = schema
                .get("description")
                .and_then(|description| description.as_str())
                .unwrap_or_else(|| panic!("{} labels a schema without a description", self.name));
            let labelled = self.label(description);
            schema.insert("description".to_string(), labelled.into());
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::Path;
    use std::sync::Mutex;

    use serde_json::Value;

    use super::*;

    const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

    /// The names of the constants that labelled a description in this process.
    pub(super) static LABELLED: Mutex<BTreeSet<&'static str>> = Mutex::new(BTreeSet::new());

    /// What `ensure_experimental` is called with for something the config schema has no key
    /// for. A check that is not named here has to use a constant of this module.
    const NOT_IN_THE_SCHEMA: &[(&str, &str)] = &[
        ("OpenTelemetry export", "the `otel` settings"),
        ("daemon presets", "`mise daemons init`"),
        (
            "loading task `{}` from a `git::` file",
            "one form of a task's `file`, which is not experimental otherwise",
        ),
        ("mise installs", "the `mise installs` command"),
        ("mise oci push", "the `mise oci push` command"),
        ("mise oci run", "the `mise oci run` command"),
    ];

    fn render() -> Vec<(&'static str, Value)> {
        let settings = std::fs::read_to_string(format!("{ROOT}/settings.toml")).unwrap();
        crate::schema::render(&settings)
    }

    /// Every Rust source file of mise and its crates, except this one and vendored plugins.
    fn sources() -> Vec<(String, String)> {
        fn walk(dir: &Path, out: &mut Vec<(String, String)>) {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    if !path.ends_with("embedded-plugins") {
                        walk(&path, out);
                    }
                } else if path.extension().is_some_and(|ext| ext == "rs")
                    && !path.ends_with("mise-config/src/experimental.rs")
                {
                    let text = std::fs::read_to_string(&path).unwrap();
                    out.push((path.display().to_string(), text));
                }
            }
        }
        let mut out = vec![];
        walk(Path::new(&format!("{ROOT}/src")), &mut out);
        for entry in std::fs::read_dir(format!("{ROOT}/crates")).unwrap() {
            let src = entry.unwrap().path().join("src");
            if src.is_dir() {
                walk(&src, &mut out);
            }
        }
        assert!(out.len() > 100, "found only {} source files", out.len());
        out
    }

    /// Whether `text` holds `name` as a whole identifier after `experimental::`.
    fn refers_to(text: &str, name: &str) -> bool {
        let needle = format!("experimental::{name}");
        text.match_indices(&needle).any(|(at, _)| {
            !text[at + needle.len()..].starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_')
        })
    }

    fn descriptions<'a>(value: &'a Value, pointer: String, out: &mut Vec<(String, &'a str)>) {
        match value {
            Value::Object(object) => {
                for (key, value) in object {
                    let at = format!("{pointer}/{key}");
                    match value {
                        Value::String(text) if key == "description" => out.push((at, text)),
                        _ => descriptions(value, at, out),
                    }
                }
            }
            Value::Array(items) => {
                for (i, item) in items.iter().enumerate() {
                    descriptions(item, format!("{pointer}/{i}"), out);
                }
            }
            _ => {}
        }
    }

    #[test]
    fn every_constant_labels_the_schema_and_names_a_check() {
        render();
        let labelled = LABELLED.lock().unwrap().clone();
        let sources = sources();
        for feature in ALL {
            assert!(
                labelled.contains(feature.name),
                "experimental::{} labels nothing in the schema: mise checks for it, so label \
                 the key it gates with `#[schemars(transform = experimental::{}.labels())]`",
                feature.name,
                feature.name
            );
            let checked = sources.iter().any(|(path, text)| {
                !path.contains("/mise-config/src/")
                    && (text.contains("ensure_experimental(")
                        || text.contains("experimental_enabled("))
                    && refers_to(text, feature.name)
            });
            assert!(
                checked,
                "experimental::{} labels the schema but no check names it: pass it to \
                 `Settings::ensure_experimental` or `Settings::experimental_enabled` where mise \
                 acts on that config, or remove the label",
                feature.name
            );
        }
    }

    #[test]
    fn only_the_constants_label_the_schema() {
        for (path, schema) in render() {
            let mut found = vec![];
            descriptions(&schema, String::new(), &mut found);
            for (pointer, description) in found {
                // `[settings]` and `miserc.json` are labelled from `settings.toml`.
                if pointer.starts_with("/$defs/settings/") || path.ends_with("miserc.json") {
                    continue;
                }
                let rest = description
                    .strip_prefix(LABEL)
                    .unwrap_or(description)
                    .to_lowercase();
                assert!(
                    !rest.contains("experimental"),
                    "{path}: {pointer} says \"experimental\" in its own words; label it with \
                     a constant of `mise_config::experimental` instead: {description:?}"
                );
            }
        }
        // A label typed into a doc comment reads the same in the schema as one a constant
        // added, so the doc comments are checked where they are written.
        for (path, text) in sources() {
            if !path.contains("/mise-config/src/") {
                continue;
            }
            for (i, line) in text.lines().enumerate() {
                let line = line.trim_start();
                assert!(
                    !(line.starts_with("///") && line.contains(LABEL)),
                    "{path}:{}: a doc comment holds the label; label the key with a constant \
                     of `mise_config::experimental` instead",
                    i + 1
                );
            }
        }
    }

    #[test]
    fn every_other_check_is_listed() {
        let mut unknown = BTreeSet::new();
        let mut seen = BTreeSet::new();
        for (path, text) in sources() {
            for (at, call) in text.match_indices("ensure_experimental(") {
                let rest = &text[at + call.len()..];
                // The definition, not a call.
                if rest.starts_with("&self") {
                    continue;
                }
                let argument = rest
                    .trim_start()
                    .trim_start_matches("&format!(")
                    .trim_start();
                if let Some(literal) = argument.strip_prefix('"') {
                    let literal = &literal[..literal.find('"').unwrap()];
                    seen.insert(literal.to_string());
                    if !NOT_IN_THE_SCHEMA.iter().any(|(what, _)| *what == literal) {
                        unknown.insert(format!("{path}: {literal:?}"));
                    }
                } else if !ALL.iter().any(|feature| {
                    argument
                        .strip_prefix("experimental::")
                        .or_else(|| argument.strip_prefix("mise_config::experimental::"))
                        .or_else(|| argument.strip_prefix("crate::experimental::"))
                        .is_some_and(|rest| rest.starts_with(&format!("{}.what", feature.name)))
                }) {
                    let shown = argument.lines().next().unwrap_or_default();
                    unknown.insert(format!("{path}: {shown}"));
                }
            }
        }
        assert!(
            unknown.is_empty(),
            "these `ensure_experimental` calls name neither a constant of \
             `mise_config::experimental` (for config the schema describes) nor an entry of \
             NOT_IN_THE_SCHEMA (for everything else):\n{}",
            unknown.into_iter().collect::<Vec<_>>().join("\n")
        );
        for (what, _) in NOT_IN_THE_SCHEMA {
            assert!(
                seen.contains(*what),
                "NOT_IN_THE_SCHEMA lists {what:?}, which no `ensure_experimental` call uses"
            );
        }
    }
}
