use crate::config::ALL_TOML_CONFIG_FILES;
use crate::{config, dirs, file};
use eyre::bail;
use taplo::formatter::Options;

/// Formats mise.toml
#[derive(Debug, clap::Args)]
#[clap(visible_alias="fmt", verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Format {
    /// Format all files from the current directory
    #[clap(short, long)]
    pub all: bool,
}

impl Format {
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
            if !p.ends_with("toml") {
                continue;
            }
            let toml = file::read_to_string(&p)?;
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

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise format</bold>
"#
);
