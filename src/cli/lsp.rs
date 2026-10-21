use crate::Result;
use crate::config::schema::SCHEMA;

/// Run the language server for mise config files over stdin/stdout
///
/// Editors start this command and exchange Language Server Protocol messages with it.
///
/// It serves `mise.toml` and the other config files mise loads (`.mise.toml`, `mise.<env>.toml`, `mise.local.toml`, `mise/config.toml`, and the files of a `conf.d`). It ignores every other file, including files that `[task_config] includes` lists, so it can be attached to the TOML language without touching `Cargo.toml` or `pyproject.toml`.
///
/// It offers:
/// - diagnostics: TOML syntax errors, unknown keys with a suggestion, values of the wrong type, and deprecated keys
/// - completion of keys, including inside inline tables, and of values that have a fixed set
/// - hover documentation for keys
/// - quick fixes that replace an unknown key with the suggested one
/// - formatting, as `mise fmt` does
/// - for tasks: completion of task names in `depends`, `depends_post`, `wait_for` and `run = [{ task = "..." }]` (and of templates in `extends`), hover with the task's description and command, go to definition, find references, and rename together with every reference in the file
///
/// Tasks come from the open file, the configs in the directories above it and the TOML files their `[task_config] includes` name.
///
/// A client that runs another TOML server beside this one can switch features off with `initializationOptions`: `{"formatting": false, "hover": false, "completion": false, "definition": false, "references": false, "rename": false}`.
///
/// The server only parses a config. It never renders templates or runs anything, so opening an untrusted file is safe.
///
/// A mise that predates this command runs a task named `lsp` instead, so a client that needs to know whether the command exists can check that `mise help lsp` prints `Usage: mise lsp`.
///
/// See https://mise.jdx.dev/ide-integration.html for editor configuration.
#[derive(Debug, usage_rs::Args)]
#[usage(
    verbatim_doc_comment,
    example(
        r###"mise lsp"###,
        help = r###"Start the server; an editor's LSP configuration runs this command"###
    )
)]
pub(crate) struct Lsp {}

impl Lsp {
    pub(crate) async fn run(self) -> Result<()> {
        let options = mise_lsp::Options {
            schema: SCHEMA.clone(),
            format: Some(Box::new(|text| {
                super::fmt::format_document(text).map_err(|e| e.to_string())
            })),
            version: env!("CARGO_PKG_VERSION").to_string(),
        };
        // The server blocks on stdin, so keep it off the async runtime's worker threads.
        tokio::task::spawn_blocking(move || {
            mise_lsp::serve(options, std::io::stdin().lock(), std::io::stdout().lock())
        })
        .await??;
        Ok(())
    }
}
