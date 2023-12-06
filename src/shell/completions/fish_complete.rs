use clap::builder::StyledStr;
use clap::{Arg, Command, ValueHint};
use itertools::Itertools;

use crate::shell::completions::is_banned;

pub fn render(cmd: &Command) -> String {
    let cmds = vec![cmd];
    let subcommands = render_subcommands(&cmds).join("\n");

    format! {r#"
set -l fssf "__fish_seen_subcommand_from"

{subcommands}

function __rtx_all_plugins
    if test -z "$__rtx_all_plugins_cache"
        set -g __rtx_all_plugins_cache (rtx plugins ls --all)
    end
    for p in $__rtx_all_plugins_cache
        echo $p
    end
end
function __rtx_plugins
    if test -z "$__rtx_plugins_cache"
        set -g __rtx_plugins_cache (rtx plugins ls --core --user)
    end
    for p in $__rtx_plugins_cache
        echo $p
    end
end
function __rtx_tool_versions
    if test -z "$__rtx_tool_versions_cache"
        set -g __rtx_tool_versions_cache (rtx ls-remote --all)
    end
    for tv in $__rtx_tool_versions_cache
        echo $tv
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
    let mut complete_cmd = r#"complete -xc rtx"#.to_string();
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
    match a.get_value_hint() {
        ValueHint::DirPath => Some("(__fish_complete_directories)".to_string()),
        ValueHint::FilePath => Some("(__fish_complete_path)".to_string()),
        ValueHint::AnyPath => Some("(__fish_complete_path)".to_string()),
        _ => match a.get_id().as_str() {
            "tool" => Some("(__rtx_tool_versions)".to_string()),
            "plugin" => Some("(__rtx_plugins)".to_string()),
            "new_plugin" => Some("(__rtx_all_plugins)".to_string()),
            //"alias" => Some("(__rtx_aliases)".to_string()),
            //"prefix" => Some("(__rtx_prefixes)".to_string()),
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
            format!(r#"complete -xc rtx -n "not $fssf $others" -a {name} -d '{help}'"#)
        } else {
            let mut p = format!("$fssf {}", &parents[0]);
            for parent in &parents[1..] {
                p.push_str(&format!("; and $fssf {}", parent));
            }
            format!(r#"complete -xc rtx -n "{p}; and not $fssf $others" -a {name} -d '{help}'"#)
        }
    });

    let rendered_subcommands = subcommands.iter().flat_map(|cmd| {
        let mut cmds = cmds.iter().copied().collect_vec();
        cmds.push(cmd);
        render_subcommands(&cmds)
    });

    let mut out = vec![format! {"# {}", if full_name.is_empty() { "rtx" } else { &full_name }}];
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
