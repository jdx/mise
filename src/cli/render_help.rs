use clap::builder::StyledStr;
use color_eyre::eyre::Result;
use console::strip_ansi_codes;
use indoc::formatdoc;
use std::fs;

use crate::cli::command::Command;
use crate::cli::Cli;
use crate::config::Config;
use crate::output::Output;

/// internal command to generate markdown from help
#[derive(Debug, clap::Args)]
#[clap(hide = true)]
pub struct RenderHelp {}

impl Command for RenderHelp {
    fn run(self, _config: Config, _out: &mut Output) -> Result<()> {
        let readme = fs::read_to_string("README.md")?;
        let mut current_readme = readme.split("<!-- RTX:COMMANDS -->");

        let mut doc = String::new();
        doc.push_str(current_readme.next().unwrap());
        current_readme.next(); // discard existing commands
        doc.push_str(render_commands().as_str());
        doc.push_str(current_readme.next().unwrap());
        fs::write("README.md", &doc)?;
        Ok(())
    }
}

fn render_commands() -> String {
    let mut cli = Cli::command()
        .term_width(80)
        .max_term_width(80)
        .disable_help_subcommand(true)
        .disable_help_flag(true);
    let mut doc = formatdoc!(
        r#"
            <!-- RTX:COMMANDS -->
            ## Commands

    "#
    );
    for command in cli.get_subcommands_mut() {
        match command.has_subcommands() {
            true => {
                let name = command.get_name().to_string();
                for subcommand in command.get_subcommands_mut() {
                    if let Some(output) = render_command(Some(&name), subcommand) {
                        doc.push_str(&output);
                    }
                }
            }
            false => {
                if let Some(output) = render_command(None, command) {
                    doc.push_str(&output);
                }
            }
        }
    }
    doc.push_str("<!-- RTX:COMMANDS -->");
    doc
}

fn render_command(parent: Option<&str>, c: &mut clap::Command) -> Option<String> {
    let mut c = c.clone().disable_help_flag(true);
    if c.is_hide_set() {
        return None;
    }
    let strip_usage = |s: StyledStr| {
        s.to_string()
            .strip_prefix("Usage: ")
            .unwrap_or_default()
            .to_string()
    };
    let usage = match parent {
        Some(p) => format!("{} {}", p, strip_usage(c.render_usage())),
        None => strip_usage(c.render_usage()),
    };
    let about = strip_ansi_codes(&c.render_long_help().to_string())
        .trim()
        .to_string();
    Some(formatdoc!(
        "
        ### `rtx {usage}`

        ```
        {about}
        ```
        ",
    ))
}

#[cfg(test)]
mod tests {
    use crate::assert_cli;
    use indoc::indoc;
    use std::fs;

    #[test]
    fn test_render_help() {
        fs::write(
            "README.md",
            indoc! {r#"
            <!-- RTX:COMMANDS -->
            <!-- RTX:COMMANDS -->
        "#},
        )
        .unwrap();
        assert_cli!("render-help");
        let readme = fs::read_to_string("README.md").unwrap();
        assert!(readme.contains("## Commands"));
        fs::remove_file("README.md").unwrap();
    }
}
