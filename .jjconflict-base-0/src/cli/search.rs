use demand::DemandOption;
use demand::Select;
use eyre::Result;
use eyre::bail;
use eyre::eyre;
use itertools::Itertools;
use xx::regex;

use crate::fuzzy::{FuzzyMatcher, FuzzyPattern};
use crate::registry::RegistryTool;
use crate::tool_catalog::{ToolCatalogEntry, ToolCatalogSource};
use crate::{config::Settings, ui::table::MiseTable};

#[derive(Debug, Clone, usage_rs::ValueEnum)]
pub(crate) enum MatchType {
    Equal,
    Contains,
    Fuzzy,
}

/// Search for available tools
///
/// Searches the registry and installed backend catalogs for tools matching NAME.
///
/// By default, it will show all tools that fuzzy match the search term. For
/// non-fuzzy matches, use the `--match-type` flag.
#[derive(Debug, usage_rs::Args)]
#[usage(
    example(
        r###"mise search jq
Tool  Description
jq    Command-line JSON processor. https://github.com/jqlang/jq
jqp   A TUI playground to experiment with jq. https://github.com/noahgorstein/jqp
jiq   jid on jq - interactive JSON query tool using jq expressions. https://github.com/fiatjaf/jiq
gojq  Pure Go implementation of jq. https://github.com/itchyny/gojq"###
    ),
    example(
        r###"mise search --interactive
Tool
Search a tool
❯ jq    Command-line JSON processor. https://github.com/jqlang/jq
  jqp   A TUI playground to experiment with jq. https://github.com/noahgorstein/jqp
  jiq   jid on jq - interactive JSON query tool using jq expressions. https://github.com/fiatjaf/jiq
  gojq  Pure Go implementation of jq. https://github.com/itchyny/gojq
/jq
esc clear filter • enter confirm"###
    ),
    verbatim_doc_comment
)]
pub(crate) struct Search {
    /// The tool to search for
    name: Option<String>,

    /// Show an interactive search menu
    #[usage(long, short, conflicts = &["match_type", "no_header"])]
    interactive: bool,

    /// Match type: equal, contains, or fuzzy
    #[usage(long, short, value_enum, default = "fuzzy")]
    match_type: MatchType,

    /// Don't display headers
    #[usage(long, alias = "no-headers")]
    no_header: bool,

    /// Print all tools with descriptions for shell completions
    #[usage(long, hide = true)]
    complete: bool,

    /// Print only tool identifiers for shell completions
    #[usage(long, hide = true)]
    complete_ids: bool,
}

impl Search {
    pub(crate) async fn run(self) -> Result<()> {
        let tools = crate::tool_catalog::search(self.name.as_deref().unwrap_or_default()).await;
        if self.complete {
            self.print_completions(&tools, true);
            return Ok(());
        }
        if self.complete_ids {
            self.print_completions(&tools, false);
            return Ok(());
        }
        if self.interactive {
            self.interactive(&tools)?;
        } else {
            self.display_table(&tools)?;
        }
        Ok(())
    }

    fn interactive(&self, tools: &[ToolCatalogEntry]) -> Result<()> {
        let theme = crate::ui::theme::get_theme();
        let mut s = Select::new("Tool")
            .description("Search a tool")
            .filtering(true)
            .filterable(true)
            .theme(&theme);
        for tool in tools {
            s = s.option(
                DemandOption::new(tool.id.as_str())
                    .label(tool.id.as_str())
                    .description(&search_description(tool)),
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

    fn display_table(&self, catalog: &[ToolCatalogEntry]) -> Result<()> {
        let tools = self
            .get_matches(catalog)
            .into_iter()
            .map(|(short, description)| vec![short, description])
            .collect_vec();
        if tools.is_empty() {
            bail!(
                "tool {} not found in registry or installed backend catalogs",
                self.name.as_ref().unwrap()
            );
        }

        let mut table = MiseTable::new(self.no_header, &["Tool", "Description"]);
        for row in tools {
            table.add_row(row);
        }
        table.print()
    }

    fn get_matches(&self, catalog: &[ToolCatalogEntry]) -> Vec<(String, String)> {
        let name = self.name.as_deref().unwrap_or("");
        let mut fuzzy_matcher = FuzzyMatcher::default();
        let fuzzy_pattern = FuzzyPattern::new(&name.to_lowercase());
        let mut matches = catalog
            .iter()
            .filter_map(|tool| {
                if name.is_empty() {
                    Some((0, tool))
                } else {
                    match self.match_type {
                        MatchType::Equal => {
                            if tool.id == name || tool.name == name {
                                Some((0, tool))
                            } else {
                                None
                            }
                        }
                        MatchType::Contains => {
                            if tool.id.contains(name) || tool.name.contains(name) {
                                Some((0, tool))
                            } else {
                                None
                            }
                        }
                        MatchType::Fuzzy => {
                            let candidate = if name.contains(':') {
                                &tool.id
                            } else {
                                &tool.name
                            };
                            fuzzy_matcher
                                .score_pattern(&candidate.to_lowercase(), &fuzzy_pattern)
                                .map(|score| (score, tool))
                        }
                    }
                }
            })
            .map(|(score, tool)| (score, tool.id.clone(), search_description(tool)))
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

        crate::aqua::aqua_registry_wrapper::aqua_search_entries()
            .filter_map(|entry| {
                let tool_name = entry.name();
                let score = if entry.backend_matches(name) {
                    Some(0)
                } else {
                    match self.match_type {
                        MatchType::Equal => {
                            if tool_name == name || entry.id == name {
                                Some(0)
                            } else {
                                None
                            }
                        }
                        MatchType::Contains => {
                            if tool_name.contains(name) || entry.id.contains(name) {
                                Some(0)
                            } else {
                                None
                            }
                        }
                        MatchType::Fuzzy => {
                            fuzzy_matcher.score_pattern(&tool_name.to_lowercase(), fuzzy_pattern)
                        }
                    }
                }?;

                let search_backend = entry.backend();
                let description = get_aqua_description(entry.id, &search_backend);

                Some((score, search_backend, description))
            })
            // Distinct aqua packages can translate to the same backend
            // (e.g. crates.io/eza and eza-community/eza both to cargo:eza)
            .unique_by(|(_score, search_backend, _description)| search_backend.clone())
            .collect()
    }

    fn print_completions(&self, tools: &[ToolCatalogEntry], descriptions: bool) {
        for tool in tools {
            if descriptions {
                println!(
                    "{}:{}",
                    tool.id.replace(':', "\\:"),
                    tool.selector_description().replace(':', "\\:")
                );
            } else {
                println!("{}", tool.id);
            }
        }
    }
}

fn search_description(tool: &ToolCatalogEntry) -> String {
    match &tool.source {
        ToolCatalogSource::Registry(registry_tool) => get_description(registry_tool),
        ToolCatalogSource::VfoxBackend => {
            tool.description.clone().unwrap_or_else(|| tool.id.clone())
        }
    }
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
            backend_homepage_url(prefix, &slug)
                .unwrap_or_else(|| format!("https://github.com/{slug}"))
        })
        .collect()
}

fn backend_homepage_url(prefix: &str, slug: &str) -> Option<String> {
    match prefix {
        "core" => Some(format!("https://mise.jdx.dev/lang/{slug}.html")),
        "cargo" => Some(format!("https://crates.io/crates/{slug}")),
        "go" => Some(format!("https://pkg.go.dev/{slug}")),
        "pipx" => Some(format!("https://pypi.org/project/{slug}")),
        "npm" => Some(format!("https://www.npmjs.com/package/{slug}")),
        _ => None,
    }
}

fn get_aqua_description(id: &str, search_backend: &str) -> String {
    let fallback = search_backend.to_string();
    let Ok(pkg) =
        crate::aqua::standard_registry::package(id).unwrap_or_else(|| Ok(Default::default()))
    else {
        return fallback;
    };

    let backend = if !pkg.repo_owner.is_empty() && !pkg.repo_name.is_empty() {
        format!("https://github.com/{}/{}", pkg.repo_owner, pkg.repo_name)
    } else {
        let (backend_type, tool) = search_backend.split_once(':').unwrap_or_default();
        backend_homepage_url(backend_type, tool).unwrap_or(fallback)
    };

    match pkg.description.as_deref().filter(|d| !d.is_empty()) {
        Some(description) => format!("{description}. {backend}"),
        None => backend,
    }
}
