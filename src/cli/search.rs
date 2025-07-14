use std::collections::HashSet;
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

use crate::backend;
use crate::backend::SearchResult;
use crate::backend::backend_type::BackendType::Cargo;
use crate::backend::backend_type::BackendType::Gem;
use crate::backend::backend_type::BackendType::Npm;
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
            self.interactive().await?;
        } else {
            self.display_table().await?;
        }
        Ok(())
    }

    async fn interactive(&self) -> Result<()> {
        let tools = self.get_tools().await;
        let mut s = Select::new("Tool")
            .description("Search a tool")
            .filtering(true)
            .filterable(true);
        for t in tools.iter() {
            let name = &t.name;
            let description = &t.description;
            s = s.option(
                DemandOption::new(name)
                    .label(name)
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

    async fn display_table(&self) -> Result<()> {
        let tools = self
            .get_matches()
            .await
            .into_iter()
            .map(|result| vec![result.name, result.description])
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

    async fn get_matches(&self) -> Vec<SearchResult> {
        self.get_tools()
            .await
            .iter()
            .filter_map(|result| {
                let name = self.name.as_deref().unwrap_or("");
                if name.is_empty() {
                    Some((0, result))
                } else {
                    match self.match_type {
                        MatchType::Equal => {
                            if result.name == name {
                                Some((0, result))
                            } else {
                                None
                            }
                        }
                        MatchType::Contains => {
                            if result.name.contains(name) {
                                Some((0, result))
                            } else {
                                None
                            }
                        }
                        MatchType::Fuzzy => FUZZY_MATCHER
                            .fuzzy_match(&result.name.to_lowercase(), name.to_lowercase().as_str())
                            .map(|score| (score, result)),
                    }
                }
            })
            .sorted_by_key(|(score, _result)| -1 * *score)
            .map(|(_score, result)| result.clone())
            .collect()
    }

    async fn get_tools(&self) -> Vec<SearchResult> {
        // get tools from registry and backend
        let registry_tools = self.get_registry_tools().await;
        let backend_tools = self.get_backend_tools().await;
        // combine and sort them
        let mut all_tools = registry_tools;
        all_tools.extend(backend_tools);
        all_tools
            .into_iter()
            .sorted_by(|a, b| a.name.cmp(&b.name))
            .collect_vec()
    }

    async fn get_registry_tools(&self) -> Vec<SearchResult> {
        REGISTRY
            .iter()
            .filter(|(short, _)| filter_enabled(short))
            .map(|(short, rt)| SearchResult {
                name: short.to_string(),
                description: get_description(rt),
            })
            .sorted_by(|a, b| a.cmp(b))
            .collect_vec()
    }

    async fn get_backend_tools(&self) -> Vec<SearchResult> {
        let query = self.name.as_deref().unwrap_or("");
        if query.is_empty() {
            return vec![];
        }

        // since backend:list() returns backends from installed state,
        // we need to ensure we only use each backend type once
        let backend_list_raw = backend::list();
        let mut seen = HashSet::new();
        let backend_list = backend_list_raw
            .iter()
            .filter(|b| {
                let backend_type = b.ba().backend_type();
                !Settings::get().disable_backends.contains(&b.ba().short)
                    && matches!(backend_type, Cargo | Npm | Gem)
                    && seen.insert(backend_type)
            })
            .collect_vec();
        let mut result = Vec::new();
        for backend in backend_list.iter() {
            let results = backend.search(query).await;
            if results.is_err() {
                trace!(
                    "Error searching backend {}: {}",
                    backend.ba(),
                    results.unwrap_err()
                );
                continue;
            }
            result.extend(results.unwrap());
        }
        result
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
