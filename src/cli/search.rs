use std::sync::LazyLock as Lazy;

use clap::ValueEnum;
use demand::DemandOption;
use demand::Select;
use eyre::Result;
use eyre::bail;
use eyre::eyre;
use fuzzy_matcher::FuzzyMatcher;
use fuzzy_matcher::skim::SkimMatcherV2;
use itertools::Itertools;
use xx::regex;

use crate::registry::RegistryTool;
use crate::{
    config::Settings,
    registry::{REGISTRY, tool_enabled},
    ui::table::MiseTable,
};

static FUZZY_MATCHER: Lazy<SkimMatcherV2> =
    Lazy::new(|| SkimMatcherV2::default().use_cache(true).smart_case());

#[derive(Debug, Clone, ValueEnum)]
pub enum MatchType {
    Equal,
    Contains,
    Fuzzy,
}

/// Search for tools in the registry
///
/// This command searches a tool in the registry.
///
/// By default, it will show all tools that fuzzy match the search term. For
/// non-fuzzy matches, use the `--match-type` flag.
#[derive(Debug, clap::Args)]
#[clap(after_long_help = AFTER_LONG_HELP, verbatim_doc_comment)]
pub struct Search {
    /// The tool to search for
    name: Option<String>,

    /// Show interactive search
    #[clap(long, short, conflicts_with_all = &["match_type", "no_header"])]
    interactive: bool,

    /// Match type: equal, contains, or fuzzy
    #[clap(long, short, value_enum, default_value = "fuzzy")]
    match_type: MatchType,

    /// Don't display headers
    #[clap(long, alias = "no-headers")]
    no_header: bool,
}

impl Search {
    pub async fn run(self) -> Result<()> {
        if self.interactive {
            self.interactive()?;
        } else {
            self.display_table()?;
        }
        Ok(())
    }

    fn interactive(&self) -> Result<()> {
        let tools = self.get_tools();
        let mut s = Select::new("Tool")
            .description("Search a tool")
            .filtering(true)
            .filterable(true);
        for t in tools.iter() {
            let short = t.0.as_str();
            let description = get_description(t.1);
            s = s.option(
                DemandOption::new(short)
                    .label(short)
                    .description(&description),
            );
        }
        match s.run() {
            Ok(_) => Ok(()),
            Err(err) => {
                if err.kind() == std::io::ErrorKind::Interrupted {
                    // user interrupted, exit gracefully
                    Ok(())
                } else {
                    Err(eyre!(err))
                }
            }
        }
    }

    fn display_table(&self) -> Result<()> {
        let tools = self
            .get_matches()
            .into_iter()
            .map(|(short, description)| vec![short, description])
            .collect_vec();
        if tools.is_empty() {
            bail!("tool {} not found in registry", self.name.as_ref().unwrap());
        }

        let mut table = MiseTable::new(self.no_header, &["Tool", "Description"]);
        for row in tools {
            table.add_row(row);
        }
        table.print()
    }

    fn get_matches(&self) -> Vec<(String, String)> {
        self.get_tools()
            .iter()
            .filter_map(|(short, rt)| {
                let name = self.name.as_deref().unwrap_or("");
                if name.is_empty() {
                    Some((0, short, rt))
                } else {
                    match self.match_type {
                        MatchType::Equal => {
                            if *short == name {
                                Some((0, short, rt))
                            } else {
                                None
                            }
                        }
                        MatchType::Contains => {
                            if short.contains(name) {
                                Some((0, short, rt))
                            } else {
                                None
                            }
                        }
                        MatchType::Fuzzy => FUZZY_MATCHER
                            .fuzzy_match(&short.to_lowercase(), name.to_lowercase().as_str())
                            .map(|score| (score, short, rt)),
                    }
                }
            })
            .sorted_by_key(|(score, _short, _rt)| -1 * *score)
            .map(|(_score, short, rt)| (short.to_string(), get_description(rt)))
            .collect()
    }

    fn get_tools(&self) -> Vec<(String, &'static RegistryTool)> {
        REGISTRY
            .iter()
            .filter(|(short, _)| filter_enabled(short))
            .map(|(short, rt)| (short.to_string(), rt))
            .sorted_by(|(a, _), (b, _)| a.cmp(b))
            .collect_vec()
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise search jq</bold>
    Tool  Description
    jq    Command-line JSON processor. https://github.com/jqlang/jq
    jqp   https://github.com/noahgorstein/jqp
    jiq   https://github.com/fiatjaf/jiq
    gojq  https://github.com/itchyny/gojq

    $ <bold>mise search --interactive</bold>
    Tool
    Search a tool
    ❯ jq    Command-line JSON processor. https://github.com/jqlang/jq
      jqp   https://github.com/noahgorstein/jqp
      jiq   https://github.com/fiatjaf/jiq
      gojq  https://github.com/itchyny/gojq
    /jq 
    esc clear filter • enter confirm
"#
);

fn filter_enabled(short: &str) -> bool {
    tool_enabled(
        &Settings::get().enable_tools,
        &Settings::get().disable_tools,
        &short.to_string(),
    )
}

fn get_description(tool: &RegistryTool) -> String {
    let description = tool.description.unwrap_or_default();
    let backend = get_backends(tool.backends())
        .iter()
        .filter(|b| !Settings::get().disable_backends.contains(b))
        .map(|b| b.to_string())
        .next()
        .unwrap_or_default();
    if description.is_empty() {
        backend.to_string()
    } else {
        format!("{description}. {backend}")
    }
}

fn get_backends(backends: Vec<&'static str>) -> Vec<String> {
    if backends.is_empty() {
        return vec!["".to_string()];
    }
    backends
        .iter()
        .map(|backend| {
            let prefix = backend.split(':').next().unwrap_or("");
            let slug = backend.split(':').next_back().unwrap_or("");
            let slug = regex!(r"^(.*?)\[.*\]$").replace_all(slug, "$1");
            match prefix {
                "core" => format!("https://mise.jdx.dev/lang/{slug}.html"),
                "cargo" => format!("https://crates.io/crates/{slug}"),
                "go" => format!("https://pkg.go.dev/{slug}"),
                "pipx" => format!("https://pypi.org/project/{slug}"),
                "npm" => format!("https://www.npmjs.com/package/{slug}"),
                _ => format!("https://github.com/{slug}"),
            }
        })
        .collect()
}
