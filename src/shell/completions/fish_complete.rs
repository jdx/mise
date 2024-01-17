use clap::builder::StyledStr;
use clap::{Arg, ArgAction, Command, ValueHint};
use itertools::Itertools;

use crate::shell::completions::is_banned;

pub fn render(cmd: &Command) -> String {
    let cmds = vec![cmd];
    let subcommands = render_subcommands(&cmds).join("\n");

    format! {r#"
set -l fssf "__fish_seen_subcommand_from"

{subcommands}

function __mise_all_plugins
    if test -z "$__mise_all_plugins_cache"
        set -g __mise_all_plugins_cache (mise plugins ls --all)
    end
    for p in $__mise_all_plugins_cache
        echo $p
    end
end
function __mise_plugins
    if test -z "$__mise_plugins_cache"
        set -g __mise_plugins_cache (mise plugins ls --core --user)
    end
    for p in $__mise_plugins_cache
        echo $p
    end
end
function __mise_tool_versions
    if test -z "$__mise_tool_versions_cache"
        set -g __mise_tool_versions_cache (mise plugins --core --user) (mise ls-remote --all | tac)
    end
    for tv in $__mise_tool_versions_cache
        echo $tv
    end
end
function __mise_installed_tool_versions
    for tv in (mise ls --installed | awk '{{print $1 "@" $2}}')
        echo $tv
    end
end
function __mise_aliases
    if test -z "$__mise_aliases_cache"
        set -g __mise_aliases_cache (mise alias ls | awk '{{print $2}}')
    end
    for a in $__mise_aliases_cache
        echo $a
    end
end
function __mise_tasks
    for tv in (mise task ls --no-header | awk '{{print $1}}')
        echo $tv
    end
end
function __mise_settings
    if test -z "$__mise_settings_cache"
        set -g __mise_settings_cache (mise settings ls | awk '{{print $1}}')
    end
    for s in $__mise_settings_cache
        echo $s
    end
end

# vim: noet ci pi sts=0 sw=4 ts=4
"#}
}

fn render_args(cmds: &[&Command]) -> Vec<String> {
    let cmd = cmds[cmds.len() - 1];
    cmd.get_arguments()
        .filter(|a| !a.is_hide_set())
        .sorted_by_cached_key(|a| a.get_id())
        .map(|a| render_arg(cmds, a))
        .collect()
}

fn render_arg(cmds: &[&Command], a: &Arg) -> String {
    let mut complete_cmd = r#"complete -kxc mise"#.to_string();
    let parents = cmds.iter().skip(1).map(|c| c.get_name()).collect_vec();
    if cmds.len() > 1 {
        let mut p = format!("$fssf {}", &parents[0]);
        for parent in &parents[1..] {
            p.push_str(&format!("; and $fssf {}", parent));
        }
        complete_cmd.push_str(&format!(r#" -n "{p}""#));
    }
    if let Some(short) = a.get_short() {
        complete_cmd.push_str(&format!(" -s {}", short));
    }
    if let Some(long) = a.get_long() {
        complete_cmd.push_str(&format!(" -l {}", long));
    }
    if let Some(c) = render_completer(a) {
        complete_cmd.push_str(&format!(r#" -a "{c}""#));
    }
    let help = about_to_help(a.get_help());
    complete_cmd.push_str(&format!(" -d '{help}'"));
    complete_cmd
}

fn render_completer(a: &Arg) -> Option<String> {
    if let ArgAction::Set = a.get_action() {
        let possible_values = a.get_possible_values();
        if !possible_values.is_empty() {
            return Some(
                possible_values
                    .iter()
                    .map(|v| escape_value(v.get_name()))
                    .collect::<Vec<_>>()
                    .join(" "),
            );
        }
    }
    match a.get_value_hint() {
        ValueHint::DirPath => Some("(__fish_complete_directories)".to_string()),
        ValueHint::FilePath => Some("(__fish_complete_path)".to_string()),
        ValueHint::AnyPath => Some("(__fish_complete_path)".to_string()),
        _ => match a.get_id().as_str() {
            "tool" => Some("(__mise_tool_versions)".to_string()),
            "installed_tool" => Some("(__mise_installed_tool_versions)".to_string()),
            "forge" | "plugin" => Some("(__mise_plugins)".to_string()),
            "new_plugin" => Some("(__mise_all_plugins)".to_string()),
            "alias" => Some("(__mise_aliases)".to_string()),
            "setting" => Some("(__mise_settings)".to_string()),
            "task" => Some("(__mise_tasks)".to_string()),
            //"prefix" => Some("(__mise_prefixes)".to_string()),
            _ => None,
        },
    }
}

fn render_subcommands(cmds: &[&Command]) -> Vec<String> {
    let cmd = cmds[cmds.len() - 1];
    let full_name = cmds.iter().skip(1).map(|c| c.get_name()).join(" ");
    let parents = cmds.iter().skip(1).map(|c| c.get_name()).collect_vec();

    let args = render_args(cmds);
    let subcommands = cmd
        .get_subcommands()
        .filter(|c| !is_banned(c) && !c.is_hide_set())
        .sorted_by_cached_key(|c| c.get_name())
        .collect_vec();
    let command_names = subcommands.iter().map(|c| c.get_name()).join(" ");

    let subcommand_defs = subcommands.iter().map(|cmd| {
        let mut cmds = cmds.iter().copied().collect_vec();
        cmds.push(cmd);
        let name = cmd.get_name();
        let help = about_to_help(cmd.get_about());
        if parents.is_empty() {
            format!(r#"complete -xc mise -n "not $fssf $others" -a {name} -d '{help}'"#)
        } else {
            let mut p = format!("$fssf {}", &parents[0]);
            for parent in &parents[1..] {
                p.push_str(&format!("; and $fssf {}", parent));
            }
            format!(r#"complete -xc mise -n "{p}; and not $fssf $others" -a {name} -d '{help}'"#)
        }
    });

    let rendered_subcommands = subcommands.iter().flat_map(|cmd| {
        let mut cmds = cmds.iter().copied().collect_vec();
        cmds.push(cmd);
        render_subcommands(&cmds)
    });

    let mut out = vec![format! {"# {}", if full_name.is_empty() { "mise" } else { &full_name }}];
    out.extend(args);
    if !subcommands.is_empty() {
        out.push(format! {"set -l others {command_names}"});
        out.extend(subcommand_defs);
        out.push(String::new());
        out.extend(rendered_subcommands);
    }
    out.push(String::new());
    out
}

fn first_line(s: &str) -> &str {
    s.lines().next().unwrap_or_default()
}

fn help_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\'', "'\\''")
}

fn about_to_help(ss: Option<&StyledStr>) -> String {
    match ss {
        Some(ss) => help_escape(first_line(&ss.to_string())),
        None => String::new(),
    }
}

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
