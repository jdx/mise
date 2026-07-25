use crate::cli::Cli;
use clap::CommandFactory;
use clap::builder::Resettable;
use eyre::Result;

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

        // `run`/`tasks run` redeclare some root globals as their own non-global flags and
        // add shorts the root global lacks (`-r`/`--raw`, `-S`/`--silent`, see
        // `cli::run::Run`). Those flags used to be promoted back to global here so the
        // completion parser would still recognize them before a task name (mise#10069);
        // jdx/usage#738 makes the parser scan across any known flag, global or not, and
        // bind each word to the flag it was read as, so the promotion is no longer needed.
        if let Some(run) = spec.cmd.subcommands.get_mut("run") {
            run.args = vec![];
            run.mounts.push(usage::SpecMount {
                run: "mise tasks --usage".to_string(),
            });
            // Enable completions after ::: separator for multi-task invocations
            run.restart_token = Some(":::".to_string());
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
        }

        // Require usage >= 3.5.7, the release that stops the mounting CLI's flags from
        // being inherited into mounted task commands and keeps scanning for the task
        // across non-global `run` flags (jdx/usage#738). Older `usage` CLIs offer mise's
        // globals after a task name — where they are forwarded to the task and rejected —
        // let a global shadow a same-named task flag, dropping its choices (mise#11282),
        // and fail outright on `mise run --force <task>`. 3.5 was required for the zsh
        // colon completion fixes for task names and insert strings (jdx/usage#666,
        // jdx/usage#670).
        // Declare what each command does to the world. clap cannot express this,
        // so it is applied to the derived spec; see command_effects.
        crate::cli::command_effects::apply(&mut spec);

        // 3.6 added `effect=` (jdx/usage#739), which the spec now carries; older
        // `usage` CLIs reject it outright with "unsupported cmd prop effect", so
        // this has to move in lockstep with emitting the field.
        let min_version = r#"min_usage_version "3.6""#;
        let extra = include_str!("../assets/mise-extra.usage.kdl").trim();
        println!("{min_version}\n{}\n{extra}", spec.to_string().trim());
        Ok(())
    }
}
