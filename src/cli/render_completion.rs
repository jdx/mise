use clap::{Arg, ArgAction, Args, Command, ValueHint};
use std::collections::HashSet;
use std::io::Cursor;

use clap_complete::generate;
use color_eyre::eyre::Result;
use itertools::Itertools;
use once_cell::sync::Lazy;

use crate::cli::self_update::SelfUpdate;
use crate::config::Config;
use crate::output::Output;

/// Generate shell completions
#[derive(Debug, Args)]
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

        if let clap_complete::Shell::Zsh = shell {
            rtxprintln!(out, "{}", ZshComplete::new(cmd).render()?.trim());
        } else {
            generate(shell, &mut cmd, "rtx", &mut c);
            rtxprintln!(out, "{}", String::from_utf8(c.into_inner()).unwrap());
        }

        Ok(())
    }
}

struct ZshComplete {
    cmd: Command,
}

impl ZshComplete {
    fn new(cmd: clap::Command) -> Self {
        Self { cmd }
    }

    fn render(&self) -> Result<String> {
        let command_funcs = self.render_command_funcs(&[&self.cmd]);
        let command_descriptions = self.render_command_descriptions(&[&self.cmd]);

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

        (( $+functions[__rtx_tool_versions] )) ||
        __rtx_tool_versions() {{
          if compset -P '*@'; then
            local -a tool_versions; tool_versions=($(rtx ls-remote ${{words[CURRENT]}}))
            _wanted tool_version expl 'version of tool' \
              compadd -a tool_versions
          else
            local -a plugins; plugins=($(rtx plugins --core --user))
            _wanted plugin expl 'plugin name' \
              compadd -S '@' -a plugins
          fi
        }}
        (( $+functions[__rtx_plugins] )) ||
        __rtx_plugins() {{
          local -a plugins; plugins=($(rtx plugins --core --user))
          _describe -t plugins 'plugin' plugins "$@"
        }}
        (( $+functions[__rtx_all_plugins] )) ||
        __rtx_all_plugins() {{
          local -a all_plugins; all_plugins=($(rtx plugins --all))
          _describe -t all_plugins 'all_plugins' all_plugins "$@"
        }}
        (( $+functions[__rtx_aliases] )) ||
        __rtx_aliases() {{
          local -a aliases; aliases=($(rtx aliases ls ${{words[CURRENT-1]}} | awk '{{print $2}}'))
          _describe -t aliases 'alias' aliases "$@"
        }}
        (( $+functions[__rtx_prefixes] )) ||
        __rtx_prefixes() {{
          local -a prefixes; prefixes=($(rtx ls-remote ${{words[CURRENT-1]}}))
          _describe -t prefixes 'prefix' prefixes "$@"
        }}

        if [ "$funcstack[1]" = "_rtx" ]; then
            _rtx "$@"
        else
            compdef _rtx rtx
        fi

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
        let global_args = cmds
            .iter()
            .flat_map(|cmd| cmd.get_arguments())
            .filter(|arg| arg.is_global_set());
        let cmd = cmds.last().unwrap();
        let args = cmd
            .get_arguments()
            .chain(global_args)
            .filter(|arg| !arg.is_hide_set() && !arg.is_last_set())
            .unique_by(|arg| arg.get_id())
            .map(|arg| self.render_arg(arg))
            .collect::<Vec<_>>()
            .join(" \\\n    ");
        if cmd.has_subcommands() {
            let subcommands = self.render_subcommands(cmds);
            let func = format!("__{}_cmds", func_name(cmds));
            formatdoc! {r#"
            _arguments -s -S \
                {args} \
                '1: :{func}' \
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
            .filter(|c| !banned(c))
            .sorted_by_cached_key(|c| c.get_name())
            .map(|cmd| {
                let mut names = cmd.get_all_aliases().sorted().collect_vec();
                names.push(cmd.get_name());
                let names = names.join("|");
                let mut cmds = cmds.iter().copied().collect_vec();
                cmds.push(cmd);
                let func = func_name(&cmds);
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
        let help = match arg.get_help() {
            Some(help) => format!("[{}]", help_escape(first_line(&help.to_string()))),
            None => return String::new(),
        };

        let multiple = if let ArgAction::Count | ArgAction::Append = arg.get_action() {
            "*"
        } else {
            ""
        };
        let all = self.get_short_and_longs(arg);
        let conflicts = if all.len() < 2 || multiple == "*" {
            "".to_string()
        } else {
            format!("({})", all.join(" "))
        };
        let all = if all.len() < 2 {
            all.join(" ")
        } else {
            format!("'{{{}}}'", all.join(","))
        };
        let name = arg.get_id();
        let completions = format!("{name}:{}", self.render_completion(arg));
        if arg.is_positional() {
            if let ArgAction::Count | ArgAction::Append = arg.get_action() {
                format!("'*::{completions}'")
            } else if arg.is_required_set() || name == "new_plugin" {
                format!("':{completions}'")
            } else {
                format!("'::{completions}'")
            }
        } else if arg.get_action().takes_values() {
            format!("'{conflicts}{multiple}{all}={help}:{completions}'")
        } else {
            format!("'{conflicts}{multiple}{all}{help}'")
        }
    }

    fn get_short_and_longs(&self, arg: &Arg) -> Vec<String> {
        let short = arg
            .get_short_and_visible_aliases()
            .unwrap_or_default()
            .into_iter()
            .map(|s| format!("-{s}"))
            .sorted();
        let long = arg
            .get_long_and_visible_aliases()
            .unwrap_or_default()
            .into_iter()
            .map(|s| format!("--{s}"))
            .sorted();
        short.chain(long).collect()
    }

    fn render_completion(&self, arg: &Arg) -> String {
        let possible_values = arg.get_possible_values();
        if !possible_values.is_empty() {
            return format!(
                "({})",
                possible_values
                    .iter()
                    .map(|v| escape_value(v.get_name()))
                    .collect::<Vec<_>>()
                    .join(" ")
            );
        };
        match arg.get_value_hint() {
            ValueHint::DirPath => format!("_directories"),
            ValueHint::FilePath => format!("_files"),
            ValueHint::AnyPath => format!("_files"),
            ValueHint::CommandName => format!("_command_names -e"),
            ValueHint::CommandString => format!("_cmdstring"),
            ValueHint::CommandWithArguments => format!("_cmdambivalent"),
            ValueHint::ExecutablePath => format!("_absolute_command_paths"),
            ValueHint::Username => format!("_users"),
            ValueHint::Hostname => format!("_hosts"),
            ValueHint::Url => format!("_urls"),
            ValueHint::EmailAddress => format!("_email_addresses"),
            ValueHint::Other => format!("( )"),
            _ => match arg.get_id().as_str() {
                "tool" => format!("__rtx_tool_versions"),
                "plugin" => format!("__rtx_plugins"),
                "new_plugin" => format!("__rtx_all_plugins"),
                "alias" => format!("__rtx_aliases"),
                "prefix" => format!("__rtx_prefixes"),
                _ => format!(""),
            },
        }
    }

    fn render_command_funcs(&self, cmds: &[&Command]) -> String {
        let cmd = cmds.last().unwrap();
        cmd.get_subcommands()
            .filter(|c| !banned(c))
            .sorted_by_key(|c| c.get_name())
            .map(|cmd| {
                let mut cmds = cmds.iter().copied().collect_vec();
                cmds.push(cmd);
                let func = func_name(&cmds);
                let args = self.render_args(&cmds);
                let subcommand_funcs = self.render_command_funcs(&cmds);
                (formatdoc! {r#"
                    (( $+functions[__{func}_cmd] )) ||
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

    fn render_command_descriptions(&self, cmds: &[&Command]) -> String {
        let cmd = cmds.last().unwrap();
        let commands = cmd
            .get_subcommands()
            .filter(|c| !c.is_hide_set() && !banned(c))
            .sorted_by_key(|c| c.get_name())
            .map(|cmd| {
                let name = cmd.get_name();
                let about = match cmd.get_about() {
                    Some(about) => help_escape(first_line(&about.to_string())).to_string(),
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
        let func = format!("__{}_cmds", func_name(cmds));
        let mut out = vec![formatdoc! {r#"
        (( $+functions[{func}] )) ||
        {func}() {{
          local commands; commands=(
        {commands}
          )
          _describe -t commands 'command' commands "$@"
        }}"#}];

        for cmd in cmd
            .get_subcommands()
            .filter(|c| c.has_subcommands() && !banned(c))
        {
            let mut cmds = cmds.iter().copied().collect_vec();
            cmds.push(cmd);
            out.push(self.render_command_descriptions(&cmds));
        }
        out.join("\n")
    }
}

fn func_name(cmds: &[&Command]) -> String {
    cmds.iter()
        .map(|c| c.get_name())
        .join("_")
        .replace('-', "_")
}

fn first_line(s: &str) -> &str {
    s.lines().next().unwrap_or_default()
}

fn help_escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('\'', "'\\''")
        .replace('[', "\\[")
        .replace(']', "\\]")
        .replace(':', "\\:")
        .replace('$', "\\$")
        .replace('`', "\\`")
}

/// Escape value string inside single quotes and parentheses
fn escape_value(string: &str) -> String {
    string
        .replace('\\', "\\\\")
        .replace('\'', "'\\''")
        .replace('[', "\\[")
        .replace(']', "\\]")
        .replace(':', "\\:")
        .replace('$', "\\$")
        .replace('`', "\\`")
        .replace('(', "\\(")
        .replace(')', "\\)")
        .replace(' ', "\\ ")
}

static BANNED_COMMANDS: Lazy<HashSet<&str>> =
    Lazy::new(|| ["render-mangen", "render-help", "render-completion"].into());

fn banned(cmd: &Command) -> bool {
    BANNED_COMMANDS.contains(&cmd.get_name())
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
