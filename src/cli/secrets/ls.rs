use eyre::Result;
use serde_json::{Value, json};
use tabled::Tabled;

use crate::config::{Config, Settings, settings::SettingsExt};
use crate::file::display_path;
use crate::secrets::{
    self, InjectMode, Inventory, InventoryTask, InventoryUse, KeyKind, Problem, UseVia,
};
use crate::ui::table;

/// List the secret names this project's secrets source provides, without their values
///
/// Shows names and metadata only; values are never shown. The source is declared in the
/// project's `mise.toml` with `[secrets.fnox]`.
#[derive(Debug, usage_rs::Args)]
#[usage(
    visible_alias = "list",
    example(
        r###"mise secrets ls
KEY                ENV    FILE  SCOPES  TASKS   DESCRIPTION
DATABASE_URL       true   no    run, exec  deploy  app database
GCP_SA_JSON        exec   yes   run               service account
SIGNING_KEY        false  no    -                 release signing key"###
    ),
    verbatim_doc_comment
)]
pub(super) struct SecretsLs {
    /// Output in JSON format
    #[usage(long, short = 'J')]
    pub json: bool,

    /// Don't show table header
    #[usage(long)]
    pub no_header: bool,

    /// Render completions: the injectable key names, one per line
    #[usage(long, hide = true)]
    pub complete: bool,
}

#[derive(Tabled)]
struct Row {
    #[tabled(rename = "KEY")]
    key: String,
    #[tabled(rename = "ENV")]
    env: String,
    #[tabled(rename = "FILE")]
    file: String,
    #[tabled(rename = "SCOPES")]
    scopes: String,
    #[tabled(rename = "TASKS")]
    tasks: String,
    #[tabled(rename = "DESCRIPTION")]
    description: String,
}

fn env_json(mode: Option<InjectMode>) -> Value {
    match mode {
        Some(InjectMode::Shell) => json!(true),
        Some(InjectMode::Exec) => json!("exec"),
        Some(InjectMode::Never) => json!(false),
        None => Value::Null,
    }
}

fn env_text(mode: Option<InjectMode>) -> &'static str {
    match mode {
        Some(InjectMode::Shell) => "true",
        Some(InjectMode::Exec) => "exec",
        Some(InjectMode::Never) => "false",
        None => "-",
    }
}

impl SecretsLs {
    pub(super) async fn run(self) -> Result<()> {
        if self.complete {
            return self.complete().await;
        }
        Settings::get().ensure_experimental("mise secrets")?;
        Settings::ensure_not_safe("mise secrets")?;
        let config = Config::get().await?;
        let Inventory {
            source,
            catalog,
            ignored,
            tasks,
            problems,
        } = secrets::inventory(&config, !self.json).await?;
        let (Some(source), Some(catalog)) = (source, catalog) else {
            return self.print_no_source(&ignored);
        };
        if self.json {
            let keys = catalog
                .entries
                .iter()
                .map(|(name, e)| {
                    json!({
                        "key": name.as_str(),
                        "kind": match e.kind {
                            KeyKind::Secret => "secret",
                            KeyKind::Lease { .. } => "lease",
                        },
                        "env": env_json(e.mode),
                        "as_file": e.as_file,
                        "lease": match &e.kind {
                            KeyKind::Lease { name } => json!(name),
                            KeyKind::Secret => Value::Null,
                        },
                        "description": e.description,
                        "scopes": scopes_json(e.injectable, e.as_file),
                        "tasks": tasks_for(&tasks, name.as_str())
                            .map(|(t, u)| match &u.via {
                                UseVia::List => json!({"task": t.task, "via": "list", "file": t.file}),
                                UseVia::Template { var } => json!({"task": t.task, "via": "template", "var": var, "file": t.file}),
                            })
                            .collect::<Vec<_>>(),
                    })
                })
                .collect::<Vec<_>>();
            let doc = json!({
                "source": {
                    "kind": source.kind,
                    "root": source.root,
                    "declared_in": source.declared_in,
                    "profile": catalog.profile.join(","),
                    "tool": {
                        "path": source.tool_path,
                        "version": catalog.tool_version,
                    },
                },
                "keys": keys,
                "dynamic_leases": catalog.dynamic_leases,
                "ignored": ignored,
                "problems": problems.iter().map(problem_json).collect::<Vec<_>>(),
            });
            miseprintln!("{}", serde_json::to_string_pretty(&doc)?);
            return Ok(());
        }
        let nearest = source
            .declared_in
            .first()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let daemon = source
            .daemon
            .as_ref()
            .map(|d| format!(" · {d}"))
            .unwrap_or_default();
        eprintln!(
            "{} · profile {} · {} ({}) · {} {}{daemon}",
            source.kind,
            catalog.profile.join(","),
            display_path(&source.root),
            nearest,
            source.kind,
            catalog.tool_version
        );
        for file in &ignored {
            eprintln!("{}", secrets::ignored_line(file));
        }
        for problem in &problems {
            eprintln!("warning: {}", problem.render());
        }
        let rows = catalog
            .entries
            .iter()
            .map(|(name, e)| Row {
                key: name.to_string(),
                env: env_text(e.mode).to_string(),
                file: if e.as_file { "yes" } else { "no" }.to_string(),
                scopes: match scopes_json(e.injectable, e.as_file) {
                    scopes if scopes.is_empty() => "-".to_string(),
                    scopes => scopes.join(", "),
                },
                tasks: tasks_for(&tasks, name.as_str())
                    .map(|(t, u)| match &u.via {
                        UseVia::List => t.task.clone(),
                        UseVia::Template { var } => format!("{} (env.{var})", t.task),
                    })
                    .collect::<Vec<_>>()
                    .join(", "),
                description: match (&e.description, &e.kind) {
                    (Some(d), _) => d.clone(),
                    (None, KeyKind::Lease { name }) => format!("(lease {name})"),
                    (None, KeyKind::Secret) => String::new(),
                },
            })
            .collect::<Vec<_>>();
        let mut table = tabled::Table::new(rows);
        table::print(&mut table, self.no_header)?;
        Ok(())
    }

    /// Key names for `mise run --secrets <TAB>` and `mise x --secrets <TAB>`. Silent when
    /// there is nothing to offer: a completion must never print an error.
    async fn complete(&self) -> Result<()> {
        if !Settings::get().experimental || Settings::safe_mode() {
            return Ok(());
        }
        let Ok(config) = Config::get().await else {
            return Ok(());
        };
        if let Ok(Inventory {
            catalog: Some(catalog),
            ..
        }) = secrets::inventory(&config, false).await
        {
            for (name, entry) in &catalog.entries {
                if entry.injectable {
                    miseprintln!("{name}");
                }
            }
        }
        Ok(())
    }

    fn print_no_source(&self, ignored: &[std::path::PathBuf]) -> Result<()> {
        if self.json {
            let doc = json!({
                "source": null,
                "keys": [],
                "dynamic_leases": [],
                "ignored": ignored,
                "problems": [],
            });
            miseprintln!("{}", serde_json::to_string_pretty(&doc)?);
            return Ok(());
        }
        eprintln!(
            "{}",
            secrets::no_source_message(&std::env::current_dir().unwrap_or_default(), ignored)
        );
        Ok(())
    }
}

/// Where a key can be handed to a process: tasks (`mise run`) and `mise x`, except that
/// `mise x` cannot clean up a file secret after it hands the process over.
fn scopes_json(injectable: bool, as_file: bool) -> Vec<&'static str> {
    match (injectable, as_file) {
        (false, _) => vec![],
        (true, true) => vec!["run"],
        (true, false) => vec!["run", "exec"],
    }
}

fn tasks_for<'a>(
    tasks: &'a [InventoryTask],
    key: &'a str,
) -> impl Iterator<Item = (&'a InventoryTask, &'a InventoryUse)> {
    tasks.iter().flat_map(move |t| {
        t.uses
            .iter()
            .filter(move |u| u.key == key)
            .map(move |u| (t, u))
    })
}

fn problem_json(p: &Problem) -> Value {
    json!({
        "task": p.task,
        "key": p.key,
        "kind": p.kind.as_str(),
        "suggestion": p.suggestion,
    })
}
