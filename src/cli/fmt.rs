use crate::Result;
use crate::config::ALL_TOML_CONFIG_FILES;
use crate::{config, dirs, file};
use eyre::bail;
use taplo::formatter::Options;

/// Formats mise.toml
///
/// Sorts keys and cleans up whitespace in mise.toml
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Fmt {
    /// Format all files from the current directory
    #[clap(short, long)]
    pub all: bool,
}

impl Fmt {
    pub fn run(self) -> eyre::Result<()> {
        let cwd = dirs::CWD.clone().unwrap_or_default();
        let configs = if self.all {
            ALL_TOML_CONFIG_FILES.clone()
        } else {
            config::config_files_in_dir(&cwd)
        };
        if configs.is_empty() {
            bail!("No config file found in current directory");
        }
        for p in configs {
            if !p
                .file_name()
                .is_some_and(|f| f.to_string_lossy().ends_with("toml"))
            {
                continue;
            }
            let toml = file::read_to_string(&p)?;
            let toml = sort(toml)?;
            let toml = taplo::formatter::format(
                &toml,
                Options {
                    align_entries: false,
                    align_comments: true,
                    align_single_comments: true,
                    array_trailing_comma: true,
                    array_auto_expand: true,
                    inline_table_expand: true,
                    array_auto_collapse: true,
                    compact_arrays: true,
                    compact_inline_tables: false,
                    compact_entries: false,
                    column_width: 80,
                    indent_tables: false,
                    indent_entries: false,
                    indent_string: "  ".to_string(),
                    trailing_newline: true,
                    reorder_keys: false,
                    reorder_arrays: false,
                    allowed_blank_lines: 2,
                    crlf: false,
                },
            );
            file::write(&p, &toml)?;
        }

        Ok(())
    }
}

fn sort(toml: String) -> Result<String> {
    let mut doc: toml_edit::DocumentMut = toml.parse()?;
    let order = |k: String| match k.as_str() {
        "min_version" => 0,
        "env_file" => 1,
        "env_path" => 2,
        "_" => 3,
        "env" => 4,
        "vars" => 5,
        "hooks" => 6,
        "watch_files" => 7,
        "tools" => 8,
        "tasks" => 10,
        "task_config" => 11,
        "redactions" => 12,
        "alias" => 13,
        "plugins" => 14,
        "settings" => 15,
        _ => 9,
    };
    doc.sort_values_by(|a, _, b, _| order(a.to_string()).cmp(&order(b.to_string())));
    Ok(doc.to_string())
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise fmt</bold>
"#
);
