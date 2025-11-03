use crate::Result;
use crate::config::ALL_TOML_CONFIG_FILES;
use crate::{config, dirs, file};
use eyre::bail;
use std::io::{self, Read, Write};
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

    /// Check if the configs are formatted, no formatting is done
    #[clap(short, long)]
    pub check: bool,

    /// Read config from stdin and write its formatted version into
    /// stdout
    #[clap(short, long)]
    pub stdin: bool,
}

impl Fmt {
    pub fn run(self) -> eyre::Result<()> {
        if self.stdin {
            let mut toml = String::new();
            io::stdin().read_to_string(&mut toml)?;

            let toml = sort(toml)?;
            let toml = format(toml)?;
            let mut stdout = io::stdout();
            write!(stdout, "{toml}")?;

            return Ok(());
        }

        let cwd = dirs::CWD.clone().unwrap_or_default();
        let configs = if self.all {
            ALL_TOML_CONFIG_FILES.clone()
        } else {
            config::config_files_in_dir(&cwd)
        };
        if configs.is_empty() {
            bail!("No config file found in current directory");
        }
        let mut errors = Vec::new();
        for p in configs {
            if !p
                .file_name()
                .is_some_and(|f| f.to_string_lossy().ends_with("toml"))
            {
                continue;
            }
            let source = file::read_to_string(&p)?;
            let toml = source.clone();
            let toml = sort(toml)?;
            let toml = format(toml)?;
            if self.check {
                if source != toml {
                    errors.push(p.display().to_string());
                }
                continue;
            }
            file::write(&p, &toml)?;
        }

        if !errors.is_empty() {
            bail!(
                "Following config files are not properly formatted:\n{}",
                errors.join("\n")
            );
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

fn format(toml: String) -> Result<String> {
    let tmp = taplo::formatter::format(
        &toml,
        Options {
            align_comments: true,
            align_entries: false,
            align_single_comments: true,
            allowed_blank_lines: 2,
            array_auto_collapse: true,
            array_auto_expand: true,
            array_trailing_comma: true,
            column_width: 80,
            compact_arrays: true,
            compact_entries: false,
            compact_inline_tables: false,
            crlf: false,
            indent_entries: false,
            indent_string: "  ".to_string(),
            indent_tables: false,
            inline_table_expand: true,
            reorder_arrays: false,
            reorder_keys: false,
            reorder_inline_tables: false,
            trailing_newline: true,
        },
    );

    Ok(tmp)
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise fmt</bold>
"#
);
