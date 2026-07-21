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

        // The `run`/`tasks run` subcommands redeclare some root globals as their own
        // non-global flags (see `cli::run::Run`). When the usage parser descends into
        // a mounted task subcommand it keeps only inherited global flags, so a leading
        // `mise -C <dir> run <task>` (or `mise run <flag> <task>`) used to mis-parse
        // the redeclared flag during completion. See mise#10069.
        //
        // jdx/usage#649 fixes the common case in the parser: when a subcommand
        // redeclares an inherited global as non-global, the inherited global (with all
        // its aliases) is now preserved. That covers `-C`/`--cd`, `-j`/`--jobs`, and
        // `-q`/`--quiet`, whose short *is* a root global short, so no spec workaround
        // is needed for them anymore.
        //
        // It cannot cover `-r`/`--raw` and `-S`/`--silent`: the root globals are
        // long-only (`--raw`/`--silent` have no short, see `cli::Cli`), and the `-r`/
        // `-S` shorts live only on the non-global `run` redeclarations. The parser
        // preserves the short-less root global and drops the redeclaration, losing the
        // short. So we still promote just those "orphan short" flags to global in the
        // completion spec (spec-only; clap runtime parsing is unchanged).
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
        // Promote a redeclared flag only when it carries a short alias that the
        // matching root global lacks (so jdx/usage#649 would otherwise drop it).
        let promote_orphan_shorts = |cmd: &mut usage::SpecCommand| {
            for f in cmd.flags.iter_mut() {
                let long_is_root_global = f.long.iter().any(|l| global_longs.contains(l));
                let has_orphan_short = f.short.iter().any(|c| !global_shorts.contains(c));
                if long_is_root_global && has_orphan_short {
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
            promote_orphan_shorts(run);
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
            promote_orphan_shorts(tasks_run);
        }

        // Require usage >= 3.5, the release that ships zsh colon completion
        // fixes for task names and insert strings (see jdx/usage#666 and
        // jdx/usage#670). This guards old `usage` CLIs from silently
        // re-triggering the broken colon completion behavior.
        let min_version = r#"min_usage_version "3.5""#;
        let extra = include_str!("../assets/mise-extra.usage.kdl").trim();
        println!("{min_version}\n{}\n{extra}", spec.to_string().trim());
        Ok(())
    }
}
