use color_eyre::eyre::Result;
use itertools::Itertools;

use crate::cli::command::Command;
use crate::cli::Cli;
use crate::config::Config;
use crate::output::Output;
use crate::runtimes::RuntimeVersion;

/// [internal] simulates asdf for plugins that call "asdf" internally
#[derive(Debug, clap::Args)]
#[clap(hide = true, verbatim_doc_comment)]
pub struct Asdf {
    /// all arguments
    #[clap(allow_hyphen_values = true)]
    args: Vec<String>,
}

impl Command for Asdf {
    fn run(mut self, config: Config, out: &mut Output) -> Result<()> {
        let mut args = vec![String::from("rtx")];
        args.append(&mut self.args);

        match args.get(1).map(|s| s.as_str()) {
            Some("reshim") => Ok(()),
            Some("list") => list_versions(out, &args),
            Some("install") => {
                if args.len() == 4 {
                    let version = args.pop().unwrap();
                    args[2] = format!("{}@{}", args[2], version);
                }
                Cli::new().run(config, &args, out)
            }
            _ => Cli::new().run(config, &args, out),
        }
    }
}

fn list_versions(out: &mut Output, args: &Vec<String>) -> Result<()> {
    let mut versions = RuntimeVersion::list()?;
    let plugin = match args.len() {
        3 => Some(&args[2]),
        _ => None,
    };
    if let Some(plugin) = plugin {
        versions.retain(|v| &v.plugin.name == plugin);
        for version in versions {
            rtxprintln!(out, "{}", version.version);
        }
    } else {
        for (plugin, versions) in &versions.into_iter().group_by(|v| v.plugin.name.clone()) {
            rtxprintln!(out, "{}", plugin);
            for version in versions {
                rtxprintln!(out, "  {}", version.version);
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod test {
    use crate::assert_cli;

    use super::*;

    #[test]
    fn test_fake_asdf() {
        let Output { stdout, .. } = assert_cli!("asdf", "-v");
        assert!(stdout.content.starts_with("rtx "));
    }

    #[test]
    fn test_fake_asdf_list() {
        assert_cli!("plugin", "install", "shfmt");
        assert_cli!("install", "shfmt@2");
        let Output { stdout, .. } = assert_cli!("asdf", "list", "shfmt");
        assert!(stdout.content.contains('2'));
    }
}
