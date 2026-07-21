use clap::ValueEnum;
use demand::DemandOption;
use demand::Select;
use eyre::Result;
use eyre::bail;
use eyre::eyre;
use itertools::Itertools;
use xx::regex;

use crate::fuzzy::{FuzzyMatcher, FuzzyPattern};
use crate::registry::RegistryTool;
use crate::{
    config::Settings,
    registry::{REGISTRY, tool_enabled},
    ui::table::MiseTable,
};

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
        let theme = crate::ui::theme::get_theme();
        let mut s = Select::new("Tool")
            .description("Search a tool")
            .filtering(true)
            .filterable(true)
            .theme(&theme);
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
        let name = self.name.as_deref().unwrap_or("");
        let mut fuzzy_matcher = FuzzyMatcher::default();
        let fuzzy_pattern = FuzzyPattern::new(&name.to_lowercase());
        let mut matches = self
            .get_tools()
            .iter()
            .filter_map(|(short, rt)| {
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
                        MatchType::Fuzzy => fuzzy_matcher
                            .score_pattern(&short.to_lowercase(), &fuzzy_pattern)
                            .map(|score| (score, short, rt)),
                    }
                }
            })
            .map(|(score, short, rt)| (score, short.to_string(), get_description(rt)))
            .collect_vec();

        if matches.is_empty() {
            matches.extend(self.get_aqua_matches(name, &mut fuzzy_matcher, &fuzzy_pattern));
        }

        matches
            .into_iter()
            .sorted_by_key(|(score, _short, _description)| std::cmp::Reverse(*score))
            .map(|(_score, short, description)| (short, description))
            .collect()
    }

    fn get_aqua_matches(
        &self,
        name: &str,
        fuzzy_matcher: &mut FuzzyMatcher,
        fuzzy_pattern: &FuzzyPattern,
    ) -> Vec<(u32, String, String)> {
        if name.is_empty() {
            return vec![];
        }

        crate::aqua::standard_registry::package_ids()
            .into_iter()
            .map(|s| s.to_string())
            .filter_map(|id| {
                let tool_name = id.rsplit_once('/').map_or(id.as_str(), |(_, name)| name);
                let score = match self.match_type {
                    MatchType::Equal => {
                        if tool_name == name || id == name || format!("aqua:{id}") == name {
                            Some(0)
                        } else {
                            None
                        }
                    }
                    MatchType::Contains => {
                        if tool_name.contains(name) || id.contains(name) {
                            Some(0)
                        } else {
                            None
                        }
                    }
                    MatchType::Fuzzy => {
                        fuzzy_matcher.score_pattern(&tool_name.to_lowercase(), fuzzy_pattern)
                    }
                }?;

                Some((
                    score,
                    format!("aqua:{id}"),
                    get_aqua_description(id.as_str()),
                ))
            })
            .collect()
    }

    fn get_tools(&self) -> Vec<(String, &'static RegistryTool)> {
        REGISTRY
            .iter()
            .filter(|(short, _)| filter_enabled(short))
            .map(|(short, rt)| (short.to_string(), rt))
            .collect_vec()
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise search jq</bold>
    Tool  Description
    jq    Command-line JSON processor. https://github.com/jqlang/jq
    jqp   A TUI playground to experiment with jq. https://github.com/noahgorstein/jqp
    jiq   jid on jq - interactive JSON query tool using jq expressions. https://github.com/fiatjaf/jiq
    gojq  Pure Go implementation of jq. https://github.com/itchyny/gojq

    $ <bold>mise search --interactive</bold>
    Tool
    Search a tool
    ❯ jq    Command-line JSON processor. https://github.com/jqlang/jq
      jqp   A TUI playground to experiment with jq. https://github.com/noahgorstein/jqp
      jiq   jid on jq - interactive JSON query tool using jq expressions. https://github.com/fiatjaf/jiq
      gojq  Pure Go implementation of jq. https://github.com/itchyny/gojq
    /jq 
    esc clear filter • enter confirm
"#
);

fn filter_enabled(short: &str) -> bool {
    let settings = Settings::get();
    let enable_tools = settings.enable_tools();
    let disable_tools = settings.disable_tools();
    tool_enabled(enable_tools.as_ref(), &disable_tools, &short.to_string())
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
                "core" => format!("https://mise.en.dev/lang/{slug}.html"),
                "cargo" => format!("https://crates.io/crates/{slug}"),
                "go" => format!("https://pkg.go.dev/{slug}"),
                "pipx" => format!("https://pypi.org/project/{slug}"),
                "npm" => format!("https://www.npmjs.com/package/{slug}"),
                _ => format!("https://github.com/{slug}"),
            }
        })
        .collect()
}

fn get_aqua_description(id: &str) -> String {
    let fallback = format!("aqua:{id}");
    let Ok(pkg) =
        crate::aqua::standard_registry::package(id).unwrap_or_else(|| Ok(Default::default()))
    else {
        return fallback;
    };

    let backend = if !pkg.repo_owner.is_empty() && !pkg.repo_name.is_empty() {
        format!("https://github.com/{}/{}", pkg.repo_owner, pkg.repo_name)
    } else {
        fallback
    };

    match pkg.description.as_deref().filter(|d| !d.is_empty()) {
        Some(description) => format!("{description}. {backend}"),
        None => backend,
    }
}
