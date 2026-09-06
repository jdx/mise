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
    let mut mount = usage::SpecMount::new("mise tasks --usage".to_string());
    mount.synopsis = Some("[TASK] [ARGS]…".to_string());
    command.mounts.push(mount);
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

        spec.restamp();

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
        // 3.6 added `effect=` (jdx/usage#739), 4.0 added it on flags and args
        // (jdx/usage#742), and 6.6 added flags scoped to implicit clauses
        // (jdx/usage#1343). 6.8 adds mount synopsis metadata (jdx/usage#1393).
        // Older `usage` CLIs reject the spec outright, so this
        // moves in lockstep with the fields and layouts the spec actually carries.
        let min_version = r#"min_usage_version "6.8""#;
        println!("{min_version}\n{}", completion_spec().to_string().trim());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn example_descriptions_are_not_shell_input() {
        let spec = super::spec();
        for name in ["prune", "set", "watch", "en", "registry", "reshim"] {
            for example in &spec.cmd.subcommands[name].examples {
                for line in example.code.lines() {
                    assert!(
                        !line.starts_with("rm -rf ")
                            && !line.starts_with("Runs the ")
                            && !line.starts_with("Skip loading ")
                            && !line.starts_with("Enter value for ")
                            && line != "v20.0.0"
                            && line != "core:node"
                            && !line.ends_with("Encryption:"),
                        "{name}: {line}"
                    );
                }
            }
        }
    }

    #[test]
    fn task_mounts_describe_arguments_without_discovery() {
        let spec = super::spec();
        for cmd in [
            &spec.cmd.subcommands["run"],
            &spec.cmd.subcommands["tasks"].subcommands["run"],
        ] {
            assert_eq!(cmd.mounts[0].synopsis.as_deref(), Some("[TASK] [ARGS]…"));
            assert!(cmd.usage.ends_with("[TASK] [ARGS]…"), "{}", cmd.usage);
            let page = usage::docs::markdown::MarkdownRenderer::new(spec.clone())
                .with_link_extension(".html")
                .render_cmd(cmd)
                .unwrap();
            assert!(page.contains("[TASK] [ARGS]…"), "{page}");
        }
    }

    #[test]
    fn command_examples_reach_the_spec_and_renderers() {
        let spec = super::spec();
        let markdown = usage::docs::markdown::MarkdownRenderer::new(spec.clone());
        for name in ["activate", "run", "install", "env", "use"] {
            let cmd = &spec.cmd.subcommands[name];
            assert!(
                !cmd.examples.is_empty(),
                "{name} has no structured examples"
            );
            let help = usage::docs::cli::render_help(&spec, cmd, true);
            assert_eq!(help.matches("Examples:").count(), 1, "{help}");
            let page = markdown.render_cmd(cmd).unwrap();
            assert_eq!(page.matches("## Examples").count(), 1, "{page}");
            for example in &cmd.examples {
                assert!(!example.code.contains("<bold>"));
                assert!(!example.code.contains('\u{1b}'));
                assert!(page.contains(&example.code), "{name}: {}", example.code);
            }
        }
        let reparsed: usage::Spec = spec.to_string().parse().unwrap();
        assert_eq!(
            reparsed.cmd.subcommands["run"].examples.len(),
            spec.cmd.subcommands["run"].examples.len()
        );
        assert!(
            usage::docs::manpage::ManpageRenderer::new(spec)
                .render()
                .unwrap()
                .contains("Examples")
        );
    }
}
