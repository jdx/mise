use color_eyre::eyre::Result;

use crate::cli::command::Command;
use crate::cli::Cli;
use crate::config::Config;
use crate::output::Output;

/// Get the latest runtime version available for install
///
/// this exists for compatibility with asdf
/// it just calls `rtx list --current` under the hood
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP, hide = true)]
pub struct Current {
    /// plugin to filter by
    ///
    /// e.g.: ruby, nodejs
    #[clap()]
    plugin: Option<Vec<String>>,
}

impl Command for Current {
    fn run(self, config: Config, out: &mut Output) -> Result<()> {
        let mut args = vec![
            String::from("rtx"),
            String::from("list"),
            String::from("--current"),
        ];
        if let Some(plugins) = self.plugin {
            for p in plugins {
                args.push("-p".to_string());
                args.push(p.to_string());
            }
        }

        Cli::new().run(config, &args, out)
    }
}

const AFTER_LONG_HELP: &str = r#"
Examples:
  $ rtx current
  -> shfmt      3.6.0                        (set by /Users/jdx/src/rtx/.tool-versions)
     shellcheck 0.9.0 (missing)              (set by /Users/jdx/tool-versions)
  -> nodejs     18.13.0                      (set by /Users/jdx/.tool-versions)

  $ rtx current nodejs
  -> nodejs     18.13.0                      (set by /Users/jdx/.tool-versions)
"#;

#[cfg(test)]
mod test {
    use regex::Regex;

    use crate::assert_cli;

    #[test]
    fn test_current() {
        assert_cli!("plugin", "add", "shellcheck");
        assert_cli!("install");
        let stdout = assert_cli!("current");
        let re = Regex::new(r"-> shellcheck\s+0\.9\.0\s+").unwrap();
        assert!(re.is_match(&stdout));
    }

    #[test]
    fn test_current_with_runtimes() {
        assert_cli!("plugin", "add", "shfmt");
        assert_cli!("install");
        let stdout = assert_cli!("current", "shfmt");
        let re = Regex::new(r"-> shfmt\s+3\.5\.2\s+").unwrap();
        assert!(re.is_match(&stdout));
    }
}
