use crate::cli::Cli;
use clap::CommandFactory;
use clap::builder::Resettable;
use eyre::Result;
use std::collections::HashSet;

/// Generate a usage CLI spec
///
/// See https://usage.jdx.dev for more information on this specification.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, hide = true)]
pub struct Usage {}

impl Usage {
    pub fn run(self) -> Result<()> {
        let cli = Cli::command().version(Resettable::Reset);
        let mut spec: usage::Spec = cli.into();

        // Enable "naked" task completions: `mise foo` completes like `mise run foo`
        spec.default_subcommand = Some("run".to_string());

        // Promote completion-spec flags that collide with a root-level global flag
        // (e.g. `-C`/`--cd`) to global on the mounted `run`/`tasks run` subcommands.
        //
        // The `run` subcommand redeclares some root globals as its own non-global
        // flags (notably `-C`/`--cd`, see `cli::run::Run::cd`). When the usage parser
        // descends into a mounted task subcommand it keeps only `global` flags
        // (`available_flags.retain(|_, f| f.global)`), so the non-global redeclaration
        // causes the inherited global to be dropped. A leading `mise -C <dir> run
        // <task> ...` then mis-parses `-C` as the task's positional arg during
        // completion. Marking the colliding flags global here (completion-spec only,
        // no effect on clap runtime parsing) keeps them recognized. See mise#10069.
        //
        // Collect the root global flag identifiers up front so the immutable borrow
        // of `spec.cmd.flags` is released before the subcommands are borrowed mutably.
        let global_shorts: HashSet<char> = spec
            .cmd
            .flags
            .iter()
            .filter(|f| f.global)
            .flat_map(|f| f.short.iter().copied())
            .collect();
        let global_longs: HashSet<String> = spec
            .cmd
            .flags
            .iter()
            .filter(|f| f.global)
            .flat_map(|f| f.long.iter().cloned())
            .collect();
        let promote = |cmd: &mut usage::SpecCommand| {
            for f in cmd.flags.iter_mut() {
                if f.short.iter().any(|c| global_shorts.contains(c))
                    || f.long.iter().any(|l| global_longs.contains(l))
                {
                    f.global = true;
                }
            }
        };

        if let Some(run) = spec.cmd.subcommands.get_mut("run") {
            run.args = vec![];
            run.mounts.push(usage::SpecMount {
                run: "mise tasks --usage".to_string(),
            });
            // Enable completions after ::: separator for multi-task invocations
            run.restart_token = Some(":::".to_string());
            promote(run);
        }

        if let Some(tasks_run) = spec
            .cmd
            .subcommands
            .get_mut("tasks")
            .and_then(|tasks| tasks.subcommands.get_mut("run"))
        {
            tasks_run.mounts.push(usage::SpecMount {
                run: "mise tasks --usage".to_string(),
            });
            tasks_run.restart_token = Some(":::".to_string());
            promote(tasks_run);
        }

        let min_version = r#"min_usage_version "2.11""#;
        let extra = include_str!("../assets/mise-extra.usage.kdl").trim();
        println!("{min_version}\n{}\n{extra}", spec.to_string().trim());
        Ok(())
    }
}
