use clap::builder::StyledStr;
use console::strip_ansi_codes;
use eyre::Result;
use itertools::Itertools;

use crate::cli::Cli;
use crate::file;

/// internal command to generate markdown from help
#[derive(Debug, clap::Args)]
#[clap(hide = true)]
pub struct RenderHelp {}

impl RenderHelp {
    pub fn run(self) -> Result<()> {
        let readme = file::read_to_string("docs/cli-reference.md")?;
        let mut current_readme = readme.split("<!-- MISE:COMMANDS -->");

        let mut doc = String::new();
        doc.push_str(current_readme.next().unwrap());
        current_readme.next(); // discard existing commands
        doc.push_str(render_commands().as_str());
        doc.push_str(current_readme.next().unwrap());
        doc = remove_trailing_spaces(&doc) + "\n";
        file::write("docs/cli-reference.md", &doc)?;
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
            <!-- MISE:COMMANDS -->

            # Commands

    "#
    );
    for command in cli
        .get_subcommands_mut()
        .sorted_by_cached_key(|c| c.get_name().to_string())
    {
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
    doc.push_str("<!-- MISE:COMMANDS -->");
    doc
}

fn render_command(parent: Option<&str>, c: &clap::Command) -> Option<String> {
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
    let mut c = c.override_usage(&usage);

    let aliases = c.get_visible_aliases().sorted().collect_vec();
    let aliases = if !aliases.is_empty() {
        format!("\n**Aliases:** `{}`\n", aliases.join(", "))
    } else {
        String::new()
    };

    let about = strip_ansi_codes(&c.render_long_help().to_string())
        .trim()
        .to_string();
    Some(formatdoc!(
        "
        ## `mise {usage}`
        {aliases}
        ```text
        {about}
        ```

        ",
    ))
}

fn remove_trailing_spaces(s: &str) -> String {
    s.lines()
        .map(|line| line.trim_end().to_string())
        .collect::<Vec<String>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use crate::test::reset;
    use std::fs;

    use crate::file;
    use test_log::test;

    #[test]
    fn test_render_help() {
        reset();
        file::create_dir_all("docs").unwrap();
        file::write(
            "docs/cli-reference.md",
            indoc! {r#"
            <!-- MISE:COMMANDS -->
            <!-- MISE:COMMANDS -->
        "#},
        )
        .unwrap();
        assert_cli!("render-help");
        let readme = fs::read_to_string("docs/cli-reference.md").unwrap();
        assert!(readme.contains("# Commands"));
        file::remove_file("docs/cli-reference.md").unwrap();
    }
}
