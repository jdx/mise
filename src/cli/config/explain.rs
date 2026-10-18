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
}

impl ConfigExplain {
    pub(super) fn run(self) -> Result<()> {
        let Some(file) =
            super::target_file(self.file, self.global, self.system, self.override_file)?
        else {
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
        let explained = explain(path, line);
        // Below an unknown key, or a table whose keys the schema leaves open, nothing more is known.
        let descend = explained.known && !open_table(path);
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
                _ => {}
            }
        }
        path.pop();
    }
}

fn explain(path: &[String], line: usize) -> Explained {
    let segments: Vec<&str> = path.iter().map(String::as_str).collect();
    let root = &*schema::SCHEMA;
    let nodes = schema::at(&segments);
    let (name, parent) = segments.split_last().expect("a key has a name");
    let known =
        !nodes.is_empty() || open_table(&parent.iter().map(|s| s.to_string()).collect::<Vec<_>>());
    let suggestion = (!known)
        .then(|| crate::fuzzy::nearest(name, schema::known_keys(parent)))
        .flatten()
        .map(str::to_string);
    // The key's own description, through `$ref`, before one an alternative shape gives.
    let description = nodes
        .first()
        .and_then(|node| own_description(root, node))
        .or_else(|| {
            nodes
                .iter()
                .find_map(|node| node.get("description").and_then(Value::as_str))
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
    }
}

/// Whether the table at `path` accepts any key, such as `[_]`: its schema says
/// `additionalProperties = true` and names no keys of its own.
fn open_table(path: &[String]) -> bool {
    let segments: Vec<&str> = path.iter().map(String::as_str).collect();
    schema::at(&segments).iter().any(|node| {
        node.get("additionalProperties") == Some(&Value::Bool(true))
            && node.get("properties").is_none()
    })
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
