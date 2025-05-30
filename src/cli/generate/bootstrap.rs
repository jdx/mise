use crate::config::Settings;
use crate::http::HTTP;
use crate::ui::info;
use crate::{Result, file, minisign};
use clap::ValueHint;
use std::path::PathBuf;
use xx::file::display_path;
use xx::regex;

/// [experimental] Generate a script to download+execute mise
///
/// This is designed to be used in a project where contributors may not have mise installed.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Bootstrap {
    /// Sandboxes mise internal directories like MISE_DATA_DIR and MISE_CACHE_DIR into a `.mise` directory in the project
    ///
    /// This is necessary if users may use a different version of mise outside the project.
    #[clap(long, short, verbatim_doc_comment)]
    localize: bool,
    /// Directory to put localized data into
    #[clap(long, verbatim_doc_comment, default_value=".mise", value_hint=ValueHint::DirPath)]
    localized_dir: PathBuf,
    /// Specify mise version to fetch
    #[clap(long, short = 'V', verbatim_doc_comment)]
    version: Option<String>,
    /// instead of outputting the script to stdout, write to a file and make it executable
    #[clap(long, short, verbatim_doc_comment, num_args=0..=1, default_missing_value = "./bin/mise")]
    write: Option<PathBuf>,
}

impl Bootstrap {
    pub async fn run(self) -> eyre::Result<()> {
        Settings::get().ensure_experimental("generate bootstrap")?;
        let output = self.generate().await?;
        if let Some(bin) = &self.write {
            if let Some(parent) = bin.parent() {
                file::create_dir_all(parent)?;
            }
            file::write(bin, &output)?;
            file::make_executable(bin)?;
            miseprintln!("Wrote to {}", display_path(bin));
        } else {
            miseprintln!("{output}");
        }
        Ok(())
    }

    async fn generate(&self) -> Result<String> {
        let url = if let Some(v) = &self.version {
            format!("https://mise.jdx.dev/v{v}/install.sh")
        } else {
            "https://mise.jdx.dev/install.sh".into()
        };
        let install = HTTP.get_text(&url).await?;
        let install_sig = HTTP.get_text(format!("{url}.minisig")).await?;
        minisign::verify(&minisign::MISE_PUB_KEY, install.as_bytes(), &install_sig)?;
        let install = info::indent_by(install, "        ");
        let version = regex!(r#"version="\$\{MISE_VERSION:-v([0-9.]+)\}""#)
            .captures(&install)
            .unwrap()
            .get(1)
            .unwrap()
            .as_str();

        let shared_vars = r#"
local script_dir=$( cd -- "$( dirname -- "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )
local project_dir=$( cd -- "$( dirname -- "$script_dir" )" &> /dev/null && pwd )
export MISE_BOOTSTRAP_PROJECT_DIR="$project_dir"
"#;

        let vars = if self.localize {
            // TODO: this will only work right if it is in the base directory, not an absolute path or has a subdirectory
            let localized_dir = self.localized_dir.to_string_lossy();
            format!(
                r#"
local localized_dir="$project_dir/{localized_dir}"
export MISE_BOOTSTRAP_PROJECT_DIR="$project_dir"
export MISE_DATA_DIR="$localized_dir"
export MISE_CONFIG_DIR="$localized_dir"
export MISE_CACHE_DIR="$localized_dir/cache"
export MISE_STATE_DIR="$localized_dir/state"
export MISE_INSTALL_PATH="$localized_dir/mise-{version}"
export MISE_TRUSTED_CONFIG_PATHS="$project_dir${{MISE_TRUSTED_CONFIG_PATHS:+:$MISE_TRUSTED_CONFIG_PATHS}}"
export MISE_IGNORED_CONFIG_PATHS="$HOME/.config/mise${{MISE_IGNORED_CONFIG_PATHS:+:$MISE_IGNORED_CONFIG_PATHS}}"
"#
            )
        } else {
            format!(
                r#"
local cache_home="${{XDG_CACHE_HOME:-$HOME/.cache}}/mise"
export MISE_INSTALL_PATH="$cache_home/mise-{version}"
"#
            )
        };
        let shared_vars = info::indent_by(shared_vars.trim(), "    ");
        let vars = info::indent_by(vars.trim(), "    ");
        let script = format!(
            r#"
#!/usr/bin/env bash
set -eu

__mise_bootstrap() {{
{shared_vars}
{vars}
    install() {{
{install}
        cd "$MISE_BOOTSTRAP_PROJECT_DIR"
    }}
    local MISE_INSTALL_HELP=0
    test -f "$MISE_INSTALL_PATH" || install
}}
__mise_bootstrap
exec "$MISE_INSTALL_PATH" "$@"
"#
        );
        Ok(script.trim().to_string())
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise generate bootstrap >./bin/mise</bold>
    $ <bold>chmod +x ./bin/mise</bold>
    $ <bold>./bin/mise install</bold> – automatically downloads mise to .mise if not already installed
"#
);
