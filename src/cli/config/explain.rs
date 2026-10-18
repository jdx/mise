use crate::config::schema;
use crate::file::display_path;
use eyre::{Result, bail};
use serde_json::Value;
use std::path::PathBuf;
use toml_edit::{Item, TableLike};

/// Describe each key in a mise TOML file
///
/// Lists every key the file sets, with its line and what it does, from the same schema
/// editors use to check a config. A key mise does not read is marked, with the known key it
/// most likely meant, and so is a deprecated one. Reads the highest-precedence loaded TOML
/// file by default; select another with `--file`, `--global`, or `--system`.
#[derive(Debug, usage_rs::Args)]
#[usage(
    example(
        r###"mise config explain
~/src/app/mise.toml
   1  tools                  dev tools to use
   2  tools.node             a version of the tool, or a table with its version and options
   4  settings               mise settings
   5  settings.experimantal  not a key mise reads; did you mean experimental?"###,
        help = "Describe the keys of the project config"
    ),
    example(
        "mise config explain tasks",
        help = "Describe only the keys under [tasks]"
    ),
    verbatim_doc_comment
)]
pub(super) struct ConfigExplain {
    /// Dotted key path to describe, with the keys under it; omit for the whole file
    #[usage(complete = complete_key)]
    pub key: Option<String>,

    /// Config file to read, or a directory whose config file to read
    ///
    /// Defaults to the highest-precedence loaded TOML file other than the global override files.
    #[usage(short, long, visible_alias = "path", value_hint = usage_rs::ValueHint::AnyPath)]
    pub file: Option<PathBuf>,

    /// Read the global config file
    #[usage(long, short = 'g', conflicts = ["file", "system"])]
    pub global: bool,

    /// Read the system config file
    #[usage(long, conflicts = ["file", "global"])]
    pub system: bool,

    /// Read the global override file, `config.override.toml`
    #[usage(long = "override", conflicts = ["file", "global", "system"])]
    pub override_file: bool,

    /// Print the keys as JSON
    #[usage(long, short = 'J')]
    pub json: bool,
}

fn complete_key(
    partial: &<ConfigExplain as usage_rs::spec::CommandArgs>::Partial,
    ctx: &usage_rs::complete::CompleteCtx<'_>,
) -> Vec<usage_rs::complete::Candidate<'static>> {
    super::keys::complete(
        ctx,
        partial.file.as_deref(),
        partial.global,
        partial.system,
        partial.override_file,
    )
}

/// One key the file sets.
#[derive(Debug, serde::Serialize)]
struct Explained {
    key: String,
    line: usize,
    description: Option<String>,
    deprecated: bool,
    /// whether the schema names this key, or lets its table hold any key
    known: bool,
    suggestion: Option<String>,
    /// whether the schema describes this key, so the keys under it can be described too
    #[serde(skip)]
    described: bool,
}

impl ConfigExplain {
    /// `--file` naming a file, relative to `dir` (`--cd`) or the current directory: read it as
    /// is, without the config loading that resolving any other target needs.
    pub(super) fn reads_named_file(&self, dir: Option<&std::path::Path>) -> bool {
        self.file.as_ref().is_some_and(|file| match dir {
            Some(dir) => dir.join(file).is_file(),
            None => file.is_file(),
        })
    }

    pub(super) fn run(self) -> Result<()> {
        // By now any `--cd` is the current directory.
        let file = if self.reads_named_file(None) {
            self.file.clone()
        } else {
            super::target_file(self.file, self.global, self.system, self.override_file)?
        };
        let Some(file) = file else {
            bail!("No mise.toml file found");
        };
        if !file.exists() {
            bail!("config file not found: {}", display_path(&file));
        }
        let body = std::fs::read_to_string(&file)?;
        let doc = toml_edit::Document::parse(body.as_str())
            .map_err(|e| eyre::eyre!("{}: {e}", display_path(&file)))?;
        let mut keys = vec![];
        walk(&body, doc.as_table(), &mut vec![], &mut keys);
        // A table split across headers is walked as one, so put its keys back in file order.
        keys.sort_by_key(|k| k.line);
        if let Some(prefix) = &self.key {
            keys.retain(|k| k.key == *prefix || k.key.starts_with(&format!("{prefix}.")));
            if keys.is_empty() {
                bail!("Key not found: {prefix} in {}", display_path(&file));
            }
        }

        if self.json {
            miseprintln!("{}", serde_json::to_string_pretty(&keys)?);
            return Ok(());
        }
        miseprintln!("{}", display_path(&file));
        let width = keys.iter().map(|k| k.key.len()).max().unwrap_or(0);
        for k in &keys {
            miseprintln!("{:>4}  {:<width$}  {}", k.line, k.key, k.summary());
        }
        Ok(())
    }
}

impl Explained {
    fn summary(&self) -> String {
        if !self.known {
            return match &self.suggestion {
                Some(s) => format!("not a key mise reads; did you mean {s}?"),
                None => "not a key mise reads".to_string(),
            };
        }
        let description = self.description.as_deref().unwrap_or_default();
        if self.deprecated {
            format!("deprecated: {description}")
        } else {
            description.to_string()
        }
    }
}

/// Appends every key below `table`, whose path is `path`, in file order.
fn walk(body: &str, table: &dyn TableLike, path: &mut Vec<String>, out: &mut Vec<Explained>) {
    for (name, item) in table.iter() {
        path.push(name.to_string());
        let line = table
            .get_key_value(name)
            .and_then(|(key, _)| key.span())
            .map_or(0, |span| body[..span.start].matches('\n').count() + 1);
        let explained = explain(path, line, item);
        // Below an unknown key, or a key only an open table accepts, nothing more is known.
        let descend = explained.described;
        out.push(explained);
        if descend {
            match item {
                Item::Table(t) => walk(body, t, path, out),
                Item::Value(toml_edit::Value::InlineTable(t)) => walk(body, t, path, out),
                Item::ArrayOfTables(tables) => {
                    for (i, t) in tables.iter().enumerate() {
                        path.push(i.to_string());
                        walk(body, t, path, out);
                        path.pop();
                    }
                }
                Item::Value(toml_edit::Value::Array(values)) => {
                    for (i, value) in values.iter().enumerate() {
                        if let toml_edit::Value::InlineTable(t) = value {
                            path.push(i.to_string());
                            walk(body, t, path, out);
                            path.pop();
                        }
                    }
                }
                _ => {}
            }
        }
        path.pop();
    }
}

fn explain(path: &[String], line: usize, item: &Item) -> Explained {
    let segments: Vec<&str> = path.iter().map(String::as_str).collect();
    let root = &*schema::SCHEMA;
    let nodes = schema::at(&segments);
    let (name, parent) = segments.split_last().expect("a key has a name");
    let known = !nodes.is_empty() || accepts_any_key(parent);
    let suggestion = (!known)
        .then(|| crate::fuzzy::nearest(name, schema::known_keys(parent)))
        .flatten()
        .map(str::to_string);
    // Each shape the parent allows for the key, and of those the first the value fits: another
    // shape's description would misdescribe it (`required = true` is not `required = false`).
    // A value that fits no shape, such as a setting set to a choice it does not have, still
    // gets the key's first description.
    let value = Written::of(item);
    let parent_nodes = schema::at(parent);
    let candidates: Vec<&Value> = parent_nodes
        .iter()
        .filter_map(|node| schema::direct_child(node, name))
        .collect();
    let description = candidates
        .iter()
        .find_map(|candidate| describe(root, candidate, &value, 0))
        .or_else(|| {
            candidates
                .iter()
                .find_map(|candidate| own_description(root, candidate))
        })
        .map(|d| d.lines().next().unwrap_or_default().to_string());
    let deprecated = nodes
        .first()
        .is_some_and(|node| deprecated_itself(root, node));
    Explained {
        key: segments.join("."),
        line,
        description,
        deprecated,
        known,
        suggestion,
        described: !nodes.is_empty(),
    }
}

/// Whether the table at `path` accepts keys it does not describe: `[_]`, an env plugin under
/// `env._` (`additionalProperties = true`), or a plugin tool's own options
/// (`unevaluatedProperties` with a schema of its own).
fn accepts_any_key(path: &[&str]) -> bool {
    schema::at(path).iter().any(|node| {
        node.get("additionalProperties") == Some(&Value::Bool(true))
            || node
                .get("unevaluatedProperties")
                .is_some_and(|v| v == &Value::Bool(true) || v.is_object())
    })
}

/// A value as far as matching it to a shape needs: its JSON type, and the value itself when it
/// is a scalar a `const` or `enum` can name.
struct Written {
    json_type: &'static str,
    scalar: Option<Value>,
}

impl Written {
    fn of(item: &Item) -> Self {
        let scalar = match item {
            Item::Value(toml_edit::Value::String(s)) => Some(Value::from(s.value().as_str())),
            Item::Value(toml_edit::Value::Integer(i)) => Some(Value::from(*i.value())),
            Item::Value(toml_edit::Value::Float(f)) => Some(Value::from(*f.value())),
            Item::Value(toml_edit::Value::Boolean(b)) => Some(Value::from(*b.value())),
            _ => None,
        };
        Self {
            json_type: json_type(item),
            scalar,
        }
    }

    fn has_type(&self, t: &str) -> bool {
        t == self.json_type || (t == "number" && self.json_type == "integer")
    }
}

/// The description of the first shape, from `node` down through `$ref` and its alternatives,
/// that `value` fits.
fn describe<'a>(
    root: &'a Value,
    node: &'a Value,
    value: &Written,
    depth: usize,
) -> Option<&'a str> {
    if depth > 16 || !fits(root, node, value, depth) {
        return None;
    }
    own_description(root, node).or_else(|| {
        let node = resolve(root, node);
        ["oneOf", "anyOf", "allOf"]
            .iter()
            .filter_map(|combinator| node.get(*combinator).and_then(Value::as_array))
            .flatten()
            .find_map(|variant| describe(root, variant, value, depth + 1))
    })
}

/// Whether `value` fits `node`'s `const`, `enum`, `type` and alternatives. Only what tells the
/// shapes of a key apart is checked, not everything a validator would.
fn fits(root: &Value, node: &Value, value: &Written, depth: usize) -> bool {
    if depth > 16 {
        return true;
    }
    let node = resolve(root, node);
    if let Some(expected) = node.get("const")
        && value.scalar.as_ref() != Some(expected)
    {
        return false;
    }
    if let Some(Value::Array(choices)) = node.get("enum")
        && !value.scalar.as_ref().is_some_and(|v| choices.contains(v))
    {
        return false;
    }
    let typed = match node.get("type") {
        Some(Value::String(t)) => value.has_type(t),
        Some(Value::Array(types)) => types
            .iter()
            .filter_map(Value::as_str)
            .any(|t| value.has_type(t)),
        _ => true,
    };
    if !typed {
        return false;
    }
    let alternatives = |key: &str| node.get(key).and_then(Value::as_array);
    if let Some(variants) = alternatives("oneOf").or_else(|| alternatives("anyOf"))
        && !variants.iter().any(|v| fits(root, v, value, depth + 1))
    {
        return false;
    }
    alternatives("allOf").is_none_or(|parts| parts.iter().all(|p| fits(root, p, value, depth + 1)))
}

/// `node` with any `$ref` followed.
fn resolve<'a>(root: &'a Value, node: &'a Value) -> &'a Value {
    let mut node = node;
    for _ in 0..32 {
        match node
            .get("$ref")
            .and_then(Value::as_str)
            .and_then(|r| r.strip_prefix('#'))
            .and_then(|pointer| root.pointer(pointer))
        {
            Some(target) => node = target,
            None => break,
        }
    }
    node
}

/// The description on `node` or on a schema it points at with `$ref`.
fn own_description<'a>(root: &'a Value, node: &'a Value) -> Option<&'a str> {
    let mut node = node;
    for _ in 0..32 {
        if let Some(description) = node.get("description").and_then(Value::as_str) {
            return Some(description);
        }
        node = node
            .get("$ref")
            .and_then(Value::as_str)
            .and_then(|r| r.strip_prefix('#'))
            .and_then(|pointer| root.pointer(pointer))?;
    }
    None
}

/// Whether `node`, or a schema it points at with `$ref`, is marked deprecated. An alternative
/// under `oneOf` being deprecated does not make the key deprecated.
fn deprecated_itself(root: &Value, node: &Value) -> bool {
    let mut node = node;
    for _ in 0..32 {
        if node.get("deprecated") == Some(&Value::Bool(true)) {
            return true;
        }
        let Some(target) = node
            .get("$ref")
            .and_then(Value::as_str)
            .and_then(|r| r.strip_prefix('#'))
            .and_then(|pointer| root.pointer(pointer))
        else {
            return false;
        };
        node = target;
    }
    false
}

/// The JSON schema type of a TOML item, to pick the shape it is written in.
fn json_type(item: &Item) -> &'static str {
    match item {
        Item::Table(_) | Item::Value(toml_edit::Value::InlineTable(_)) => "object",
        Item::ArrayOfTables(_) | Item::Value(toml_edit::Value::Array(_)) => "array",
        Item::Value(toml_edit::Value::String(_) | toml_edit::Value::Datetime(_)) => "string",
        Item::Value(toml_edit::Value::Integer(_)) => "integer",
        Item::Value(toml_edit::Value::Float(_)) => "number",
        Item::Value(toml_edit::Value::Boolean(_)) => "boolean",
        Item::None => "null",
    }
}
