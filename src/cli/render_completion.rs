use clap::{Arg, ArgAction, Command, ValueHint};
use std::io::Cursor;
use std::iter::once;

use clap_complete::generate;
use color_eyre::eyre::Result;
use itertools::Itertools;

use crate::cli::self_update::SelfUpdate;
use crate::config::Config;
use crate::output::Output;

/// Generate shell completions
#[derive(Debug, clap::Args)]
#[clap(hide = true, verbatim_doc_comment)]
pub struct RenderCompletion {
    /// Shell type to generate completions for
    #[clap(required_unless_present = "shell_type")]
    shell: Option<clap_complete::Shell>,

    /// Shell type to generate completions for
    #[clap(long = "shell", short = 's', hide = true)]
    shell_type: Option<clap_complete::Shell>,
}

impl RenderCompletion {
    pub fn run(self, _config: Config, out: &mut Output) -> Result<()> {
        let shell = self.shell.or(self.shell_type).unwrap();

        let mut c = Cursor::new(Vec::new());
        let mut cmd = crate::cli::Cli::command().subcommand(SelfUpdate::command());

        if matches!(shell, clap_complete::Shell::Zsh) {
            rtxprintln!(out, "{}", ZshComplete::new(cmd).render()?.trim());
        } else {
            generate(shell, &mut cmd, "rtx", &mut c);
            rtxprintln!(out, "{}", String::from_utf8(c.into_inner()).unwrap());
        }

        Ok(())
    }
}

struct ZshComplete {
    cmd: clap::Command,
}

impl ZshComplete {
    fn new(cmd: clap::Command) -> Self {
        Self { cmd }
    }

    fn render(&self) -> Result<String> {
        let command_funcs = self.render_command_funcs(&[&self.cmd]);
        let command_descriptions = self.render_command_descriptions();

        Ok(formatdoc! {r#"
        #compdef rtx
        _rtx() {{
          typeset -A opt_args
          local context state line curcontext=$curcontext
          local ret=1

          {args}
        }}
        {command_funcs}
        {command_descriptions}

        __rtx_tool_versions() {{
          if compset -P '*@'; then
            local -a tool_versions; tool_versions=($(rtx ls-remote ${{words[CURRENT]}}))
            _wanted tool_version expl 'version of tool' \
              compadd -a tool_versions
          else
            local -a plugins; plugins=($(rtx plugins))
            _wanted plugin expl 'plugin name' \
              compadd -S '@' -a plugins
          fi
        }}

        _rtx "$@"

        # Local Variables:
        # mode: Shell-Script
        # sh-indentation: 2
        # indent-tabs-mode: nil
        # sh-basic-offset: 2
        # End:
        # vim: ft=zsh sw=2 ts=2 et
        "#, args = self.render_args(&[&self.cmd])})
    }

    fn render_args(&self, cmds: &[&Command]) -> String {
        let args = cmds
            .iter()
            .flat_map(|cmd| cmd.get_arguments())
            .filter(|arg| arg.is_global_set());
        let cmd = cmds.last().unwrap();
        let args = args
            .chain(cmd.get_arguments())
            .filter(|arg| !arg.is_hide_set())
            .unique_by(|arg| arg.get_id())
            .sorted_by_key(|arg| (arg.get_short(), arg.get_long(), arg.get_id()))
            .map(|arg| self.render_arg(arg))
            .collect::<Vec<_>>()
            .join(" \\\n    ");
        if cmd.has_subcommands() {
            let subcommands = self.render_subcommands(cmds);
            formatdoc! {r#"
            _arguments -s -S \
                {args} \
                '1: :_rtx_cmds' \
                '*::arg:->args' && ret=0

            {subcommands}

              return ret"#
            }
        } else {
            formatdoc! {r#"
            _arguments -s -S \
                {args}"#
            }
        }
    }

    fn render_subcommands(&self, cmds: &[&Command]) -> String {
        let cmd = cmds.last().unwrap();
        let cases = cmd
            .get_subcommands()
            .sorted_by_cached_key(|c| c.get_name())
            .map(|cmd| {
                let name = cmd.get_name();
                let mut names = cmd.get_all_aliases().sorted().collect_vec();
                names.push(name);
                let names = names.join("|");
                let func = cmds
                    .iter()
                    .chain(once(&cmd))
                    .map(|c| c.get_name())
                    .join("_")
                    .replace('-', "_");
                format!("        ({names}) __{func}_cmd && ret=0 ;;",)
            })
            .collect::<Vec<_>>()
            .join("\n");
        formatdoc! {r#"
              case "$state" in
                (args)
                  curcontext="${{curcontext%:*:*}}:rtx-cmd-$words[1]:"
                  case $words[1] in
            {cases}
                  esac
                ;;
              esac"#
        }
    }

    fn render_arg(&self, arg: &clap::Arg) -> String {
        if arg.get_action().takes_values() {
            self.render_option(arg)
        } else {
            self.render_flag(arg)
        }
    }

    fn render_flag(&self, arg: &clap::Arg) -> String {
        // print this: '(-r --raw)'{-r,--raw}'[Directly pipe stdin/stdout/stderr to user.]' \
        let help = match arg.get_help() {
            Some(help) => escape_single_quote(first_line(&help.to_string())).to_string(),
            None => return String::new(),
        };
        match (arg.get_short(), arg.get_long()) {
            (Some(short), Some(long)) => {
                format!("'(-{short} --{long})'{{-{short},--{long}}}'[{help}]'",)
            }
            (Some(short), None) => {
                format!("'-{short}[{help}]'",)
            }
            (None, Some(long)) => format!("'--{long}=[{help}]'"),
            (None, None) => self.render_positional(arg),
        }
    }

    fn render_option(&self, arg: &clap::Arg) -> String {
        // print this: '(-j --jobs)'{-j,--jobs}'=[Number of plugins and runtimes to install in parallel]:: :' \
        let help = match arg.get_help() {
            Some(help) => escape_single_quote(first_line(&help.to_string())).to_string(),
            None => return String::new(),
        };
        let completions = match arg.get_possible_values() {
            values if values.is_empty() => ":: :".to_string(),
            values => format!(
                ": :({})",
                values
                    .iter()
                    .map(|v| v.get_name())
                    .collect::<Vec<_>>()
                    .join(" ")
            ),
        };
        match (arg.get_short(), arg.get_long()) {
            (Some(short), Some(long)) => {
                format!("'(-{short} --{long})'{{-{short},--{long}}}'=[{help}]{completions}'",)
            }
            (Some(short), None) => {
                format!("'-{short}[{help}]{completions}'")
            }
            (None, Some(long)) => format!("'--{long}=[{help}]{completions}'"),
            (None, None) => self.render_positional(arg),
        }
    }

    fn render_positional(&self, arg: &Arg) -> String {
        let name = arg.get_id();
        let plural = if matches!(arg.get_action(), ArgAction::Append) {
            "*"
        } else {
            "1"
        };
        match arg.get_value_hint() {
            ValueHint::DirPath => format!("'{plural}: :_directories'"),
            ValueHint::FilePath => format!("'{plural}: :_files'"),
            ValueHint::AnyPath => format!("'{plural}: :_files'"),
            ValueHint::CommandString => format!("'{plural}: :_command_string -c'"),
            ValueHint::ExecutablePath => format!("'{plural}: :_command_names -e'"),
            ValueHint::Username => format!("'{plural}: :_users'"),
            ValueHint::Hostname => format!("'{plural}: :_hosts'"),
            ValueHint::Url => format!("'{plural}: :_urls'"),
            ValueHint::EmailAddress => format!("'{plural}: :_email_addresses'"),
            _ if name == "tool" => format!("'{plural}::{name}:__rtx_tool_versions'"),
            _ => format!("'*::{name}:'"),
        }
    }

    fn render_command_funcs(&self, cmds: &[&Command]) -> String {
        let cmd = cmds.last().unwrap();
        cmd.get_subcommands()
            .sorted_by_key(|c| c.get_name())
            .map(|cmd| {
                let func = cmds
                    .iter()
                    .chain(once(&cmd))
                    .map(|c| c.get_name())
                    .join("_")
                    .replace('-', "_");
                let mut cmds = cmds.iter().copied().collect_vec();
                cmds.push(cmd);
                let args = self.render_args(&cmds);
                let subcommand_funcs = self.render_command_funcs(&cmds);
                (formatdoc! {r#"
                    __{func}_cmd() {{
                      {args}
                    }}
                    {subcommand_funcs}"#,
                })
                .trim()
                .to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn render_command_descriptions(&self) -> String {
        let commands = self
            .cmd
            .get_subcommands()
            .filter(|c| !c.is_hide_set())
            .sorted_by_key(|c| c.get_name())
            .map(|cmd| {
                let name = cmd.get_name();
                let about = match cmd.get_about() {
                    Some(about) => escape_single_quote(first_line(&about.to_string())).to_string(),
                    None => String::new(),
                };
                let aliases = cmd.get_visible_aliases().sorted().collect_vec();
                if aliases.is_empty() {
                    format!("    '{}:{}'", name, about)
                } else {
                    let aliases = aliases.join(",");
                    format!("    {{{aliases},{name}}}':{about}'")
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        formatdoc! {r#"
        (( $+functions[_rtx_cmds] )) ||
        _rtx_cmds() {{
          local commands; commands=(
        {commands}
          )
          _describe -t commands 'command' commands "$@"
        }}"#}
    }
}

fn first_line(s: &str) -> &str {
    s.lines().next().unwrap_or_default()
}

fn escape_single_quote(s: &str) -> String {
    s.replace('\'', r"'\''")
}

#[cfg(test)]
mod tests {
    use crate::assert_cli;

    #[test]
    fn test_completion() {
        assert_cli!("render-completion", "bash");
        assert_cli!("render-completion", "fish");
        assert_cli!("render-completion", "zsh");
    }
}
