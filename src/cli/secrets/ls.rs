use eyre::Result;
use serde_json::{Value, json};
use tabled::Tabled;

use crate::config::{Config, Settings, settings::SettingsExt};
use crate::file::display_path;
use crate::secrets::{self, InjectMode, Inventory, KeyKind};
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
KEY                ENV    FILE  DESCRIPTION
DATABASE_URL       true   no    app database
DEPLOY_KEY         exec   no"###
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
}

#[derive(Tabled)]
struct Row {
    #[tabled(rename = "KEY")]
    key: String,
    #[tabled(rename = "ENV")]
    env: String,
    #[tabled(rename = "FILE")]
    file: String,
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
        Settings::get().ensure_experimental("mise secrets")?;
        Settings::ensure_not_safe("mise secrets")?;
        let config = Config::get().await?;
        let Inventory {
            source,
            catalog,
            ignored,
        } = secrets::inventory(&config).await?;
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
                    })
                })
                .collect::<Vec<_>>();
            let doc = json!({
                "source": {
                    "kind": source.kind,
                    "root": source.root,
                    "declared_in": source.declared_in,
                    "profile": source.profile,
                    "tool": {
                        "path": source.tool_path,
                        "version": catalog.tool_version,
                    },
                },
                "keys": keys,
                "dynamic_leases": catalog.dynamic_leases,
                "ignored": ignored,
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
        eprintln!(
            "{} · profile {} · {} ({}) · {} {}",
            source.kind,
            catalog.profile.join(","),
            display_path(&source.root),
            nearest,
            source.kind,
            catalog.tool_version
        );
        for file in &ignored {
            eprintln!("{}", ignored_line(file));
        }
        let rows = catalog
            .entries
            .iter()
            .map(|(name, e)| Row {
                key: name.to_string(),
                env: env_text(e.mode).to_string(),
                file: if e.as_file { "yes" } else { "no" }.to_string(),
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

    fn print_no_source(&self, ignored: &[std::path::PathBuf]) -> Result<()> {
        if self.json {
            let doc = json!({
                "source": null,
                "keys": [],
                "dynamic_leases": [],
                "ignored": ignored,
            });
            miseprintln!("{}", serde_json::to_string_pretty(&doc)?);
            return Ok(());
        }
        eprintln!(
            "no secrets source is configured for {}; add [secrets.fnox] to the project's mise.toml (https://mise.jdx.dev/environments/secrets/fnox.html)",
            display_path(std::env::current_dir().unwrap_or_default())
        );
        for file in ignored {
            eprintln!("{}", ignored_line(file));
        }
        Ok(())
    }
}

fn ignored_line(file: &std::path::Path) -> String {
    format!(
        "  [secrets.fnox] in {} is ignored: secrets sources are allowed only in project config (not global or system config, or files in or above your home directory).",
        display_path(file)
    )
}
