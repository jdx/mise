use crate::shell::completions::is_banned;
use clap::{Arg, ArgAction, Command, ValueHint};
use itertools::Itertools;

pub fn render(cmd: &Command) -> String {
    let cmds = vec![cmd];
    let command_funcs = render_command_funcs(&cmds);
    let command_descriptions = render_command_descriptions(&cmds);
    let args = render_args(&cmds);

    formatdoc! {r#"
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
              compadd -a tool_versions -o nosort
          else
            local -a plugins; plugins=($(rtx plugins --core --user))
            _wanted plugin expl 'plugin name' \
              compadd -S '@' -a plugins
          fi
        }}
        (( $+functions[__rtx_installed_tool_versions] )) ||
        __rtx_installed_tool_versions() {{
          if compset -P '*@'; then
            local plugin; plugin=${{words[CURRENT]%%@*}}
            local -a installed_tool_versions; installed_tool_versions=($(rtx ls --installed $plugin | awk '{{print $2}}'))
            _wanted installed_tool_version expl 'version of tool' \
              compadd -a installed_tool_versions -o nosort
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
        (( $+functions[__rtx_settings] )) ||
        __rtx_settings() {{
          local -a settings; settings=($(rtx settings ls | awk '{{print $1}}'))
          _describe -t settings 'setting' settings "$@"
        }}
        (( $+functions[__rtx_tasks] )) ||
        __rtx_tasks() {{
          local -a tasks; tasks=($(rtx tasks ls --no-header | awk '{{print $1}}'))
          _describe -t tasks 'task' tasks "$@"
        }}
        (( $+functions[__rtx_prefixes] )) ||
        __rtx_prefixes() {{
          if [[ CURRENT -gt 2 ]]; then
              local -a prefixes; prefixes=($(rtx ls-remote ${{words[CURRENT-1]}}))
              _describe -t prefixes 'prefix' prefixes "$@"
          fi
        }}

        if [ "$funcstack[1]" = "_rtx" ]; then
            _rtx "$@"
        else
            compdef _rtx rtx
        fi

        # vim: noet ci pi sts=0 sw=4 ts=4
        "#}
}

fn render_args(cmds: &[&Command]) -> String {
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
        .map(render_arg)
        .collect::<Vec<_>>()
        .join(" \\\n    ");
    if cmd.has_subcommands() {
        let subcommands = render_subcommands(cmds);
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

fn render_subcommands(cmds: &[&Command]) -> String {
    let cmd = cmds.last().unwrap();
    let cases = cmd
        .get_subcommands()
        .filter(|c| !is_banned(c))
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

fn render_arg(arg: &Arg) -> String {
    let help = match arg.get_help() {
        Some(help) => format!("[{}]", help_escape(first_line(&help.to_string()))),
        None => return String::new(),
    };

    let multiple = if let ArgAction::Count | ArgAction::Append = arg.get_action() {
        "*"
    } else {
        ""
    };
    let all = get_short_and_longs(arg);
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
    let completions = format!("{name}:{}", render_completion(arg));
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

fn get_short_and_longs(arg: &Arg) -> Vec<String> {
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

fn render_completion(arg: &Arg) -> String {
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
        ValueHint::DirPath => "_directories".to_string(),
        ValueHint::FilePath => "_files".to_string(),
        ValueHint::AnyPath => "_files".to_string(),
        ValueHint::CommandName => "_command_names -e".to_string(),
        ValueHint::CommandString => "_cmdstring".to_string(),
        ValueHint::CommandWithArguments => "_cmdambivalent".to_string(),
        ValueHint::ExecutablePath => "_absolute_command_paths".to_string(),
        ValueHint::Username => "_users".to_string(),
        ValueHint::Hostname => "_hosts".to_string(),
        ValueHint::Url => "_urls".to_string(),
        ValueHint::EmailAddress => "_email_addresses".to_string(),
        ValueHint::Other => "( )".to_string(),
        _ => match arg.get_id().as_str() {
            "tool" => "__rtx_tool_versions".to_string(),
            "installed_tool" => "__rtx_installed_tool_versions".to_string(),
            "plugin" => "__rtx_plugins".to_string(),
            "new_plugin" => "__rtx_all_plugins".to_string(),
            "alias" => "__rtx_aliases".to_string(),
            "setting" => "__rtx_settings".to_string(),
            "task" => "__rtx_tasks".to_string(),
            "prefix" => "__rtx_prefixes".to_string(),
            _ => String::new(),
        },
    }
}

fn render_command_funcs(cmds: &[&Command]) -> String {
    let cmd = cmds.last().unwrap();
    cmd.get_subcommands()
        .filter(|c| !is_banned(c))
        .sorted_by_key(|c| c.get_name())
        .map(|cmd| {
            let mut cmds = cmds.iter().copied().collect_vec();
            cmds.push(cmd);
            let func = func_name(&cmds);
            let args = render_args(&cmds);
            let subcommand_funcs = render_command_funcs(&cmds);
            let s = formatdoc! {r#"
                    (( $+functions[__{func}_cmd] )) ||
                    __{func}_cmd() {{
                      {args}
                    }}
                    {subcommand_funcs}"#,
            };
            s.trim().to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_command_descriptions(cmds: &[&Command]) -> String {
    let cmd = cmds.last().unwrap();
    let commands = cmd
        .get_subcommands()
        .filter(|c| !c.is_hide_set() && !is_banned(c))
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
        .filter(|c| c.has_subcommands() && !is_banned(c))
    {
        let mut cmds = cmds.iter().copied().collect_vec();
        cmds.push(cmd);
        out.push(render_command_descriptions(&cmds));
    }
    out.join("\n")
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
