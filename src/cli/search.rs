use std::sync::{Arc, LazyLock as Lazy};

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
    backend::{self, SearchResult},
    config::{Config, Settings},
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
            self.interactive().await?;
        } else {
            self.display_table().await?;
        }
        Ok(())
    }

    async fn interactive(&self) -> Result<()> {
        let (registry_tools, backend_results) = self.get_all_matches().await?;
        let mut s = Select::new("Tool")
            .description("Search a tool")
            .filtering(true)
            .filterable(true);
        
        // Add registry tools
        for (short, rt) in registry_tools.iter() {
            let description = get_description(rt);
            s = s.option(
                DemandOption::new(short.as_str())
                    .label(short.as_str())
                    .description(&description),
            );
        }
        
        // Add backend results
        for result in backend_results.iter() {
            let description = result.description.as_deref().unwrap_or("");
            s = s.option(
                DemandOption::new(&result.name)
                    .label(&result.name)
                    .description(description),
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

    async fn display_table(&self) -> Result<()> {
        let (registry_matches, backend_results) = self.get_all_matches().await?;
        
        // Convert registry matches to table rows
        let mut tools: Vec<Vec<String>> = registry_matches
            .into_iter()
            .map(|(short, rt)| vec![short, get_description(rt)])
            .collect();
        
        // Add backend results
        tools.extend(backend_results.into_iter().map(|result| {
            vec![
                result.name,
                result.description.unwrap_or_default(),
            ]
        }));
        
        if tools.is_empty() {
            bail!("no tools found matching query: {}", self.name.as_ref().unwrap_or(&"".to_string()));
        }

        let mut table = MiseTable::new(self.no_header, &["Tool", "Description"]);
        for row in tools {
            table.add_row(row);
        }
        table.print()
    }

    async fn get_all_matches(&self) -> Result<(Vec<(String, &'static RegistryTool)>, Vec<SearchResult>)> {
        let registry_matches = self.get_registry_matches();
        let backend_results = self.get_backend_matches().await?;
        Ok((registry_matches, backend_results))
    }

    fn get_registry_matches(&self) -> Vec<(String, &'static RegistryTool)> {
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
            .map(|(_score, short, rt)| (short.to_string(), *rt))
            .collect()
    }

    async fn get_backend_matches(&self) -> Result<Vec<SearchResult>> {
        let query = match &self.name {
            Some(name) => name,
            None => return Ok(vec![]), // Don't search backends if no query provided
        };

        let config = Arc::new(Config::load()?);
        let mut all_results = Vec::new();

        // Get all available backends and search each one that supports search
        for backend in backend::list() {
            match backend.search(&config, query).await {
                Ok(Some(results)) => {
                    all_results.extend(results);
                }
                Ok(None) => {
                    // Backend doesn't support search, skip
                }
                Err(e) => {
                    // Log error but don't fail the entire search
                    debug!("Search failed for backend {}: {:#}", backend.id(), e);
                }
            }
        }

        Ok(all_results)
    }

    fn get_matches(&self) -> Vec<(String, String)> {
        self.get_registry_matches()
            .into_iter()
            .map(|(short, rt)| (short, get_description(rt)))
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
