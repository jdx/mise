use crate::cli::Cli;
use eyre::Result;

/// Generate a usage CLI spec
///
/// See https://usage.jdx.dev for more information on this specification.
#[derive(Debug, usage_rs::Args)]
#[usage(verbatim_doc_comment, hide = true)]
pub(crate) struct Usage {}

fn prepare_task_runner(command: &mut usage::SpecCommand) {
    command.args.clear();
    for flag in &mut command.flags {
        flag.conflicts.retain(|selector| selector != "TASK");
        flag.requires.retain(|selector| selector != "TASK");
        flag.required_if.retain(|selector| selector != "TASK");
        flag.required_unless.retain(|selector| selector != "TASK");
    }
    command
        .mounts
        .push(usage::SpecMount::new("mise tasks --usage".to_string()));
    command.restart_token = Some(":::".to_string());
}

/// mise's own usage spec, with everything clap cannot express applied.
///
/// Shared with `mise mcp`, which answers "what does this command do" from the
/// same `effect=` data this prints. Two constructions would drift, and the one
/// an agent reads is the one that must not.
pub(super) fn spec() -> usage::Spec {
    {
        let mut spec: usage::Spec = Cli::to_kdl().parse().expect("generated mise usage spec");

        // Enable "naked" task completions: `mise foo` completes like `mise run foo`
        spec.default_subcommand = Some("run".to_string());

        // `run`/`tasks run` redeclare some root globals as their own non-global flags and
        // add shorts the root global lacks (`-r`/`--raw`, `-S`/`--silent`, see
        // `cli::run::Run`). Those flags used to be promoted back to global here so the
        // completion parser would still recognize them before a task name (mise#10069);
        // jdx/usage#738 makes the parser scan across any known flag, global or not, and
        // bind each word to the flag it was read as, so the promotion is no longer needed.
        if let Some(run) = spec.cmd.subcommands.get_mut("run") {
            prepare_task_runner(run);
        }

        if let Some(tasks_run) = spec
            .cmd
            .subcommands
            .get_mut("tasks")
            .and_then(|tasks| tasks.subcommands.get_mut("run"))
        {
            prepare_task_runner(tasks_run);
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

        spec
    }
}

pub(super) fn completion_spec() -> usage::Spec {
    let mut spec = spec();
    let extra: usage::Spec = include_str!("../assets/mise-extra.usage.kdl")
        .parse()
        .expect("mise completion metadata should parse");
    spec.merge(extra);
    spec
}

impl Usage {
    pub(crate) fn run(self) -> Result<()> {
        // 3.6 added `effect=` (jdx/usage#739) and 4.0 added it on flags and args
        // (jdx/usage#742); older `usage` CLIs reject the spec outright with
        // "unsupported cmd prop effect", so this moves in lockstep with the
        // fields the spec actually carries.
        let min_version = r#"min_usage_version "4.0""#;
        println!("{min_version}\n{}", completion_spec().to_string().trim());
        Ok(())
    }
}
