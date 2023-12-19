use eyre::Result;
use tabled::Tabled;

use crate::config::Config;
use crate::plugins::PluginName;
use crate::ui::table;

/// List aliases
/// Shows the aliases that can be specified.
/// These can come from user config or from plugins in `bin/list-aliases`.
///
/// For user config, aliases are defined like the following in `~/.config/rtx/config.toml`:
///
///   [alias.node]
///   lts = "20.0.0"
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "list", after_long_help = AFTER_LONG_HELP, verbatim_doc_comment)]
pub struct AliasLs {
    /// Show aliases for <PLUGIN>
    #[clap()]
    pub plugin: Option<PluginName>,

    /// Don't show table header
    #[clap(long)]
    pub no_header: bool,
}

impl AliasLs {
    pub fn run(self) -> Result<()> {
        let config = Config::get();
        let rows = config
            .get_all_aliases()
            .iter()
            .flat_map(|(plugin, aliases)| {
                aliases
                    .iter()
                    .filter(|(from, _to)| plugin != "node" || !from.starts_with("lts/"))
                    .map(|(from, to)| Row {
                        plugin: plugin.clone(),
                        alias: from.clone(),
                        version: to.clone(),
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let mut table = tabled::Table::new(rows);
        table::default_style(&mut table, self.no_header);
        rtxprintln!("{table}");
        Ok(())
    }
}

#[derive(Tabled)]
struct Row {
    plugin: String,
    alias: String,
    version: String,
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>
  $ <bold>rtx aliases</bold>
  node    lts-hydrogen   20.0.0
"#
);

#[cfg(test)]
mod tests {
    #[test]
    fn test_alias_ls() {
        assert_cli_snapshot!("aliases", @r###"
        java  lts          21   
        node  lts          20   
        node  lts-argon    4    
        node  lts-boron    6    
        node  lts-carbon   8    
        node  lts-dubnium  10   
        node  lts-erbium   12   
        node  lts-fermium  14   
        node  lts-gallium  16   
        node  lts-hydrogen 18   
        node  lts-iron     20   
        tiny  lts          3.1.0
        tiny  lts-prev     2.0.0
        tiny  my/alias     3.0
        "###);
    }
}
