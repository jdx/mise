//! Runtime dependency closure resolution, topologically sorted (deps first).

use std::collections::{HashMap, HashSet};

use eyre::bail;

use super::api::{self, Formula};
use super::tag;
use crate::result::Result;

#[derive(Debug, Clone)]
pub struct ResolvedFormula {
    pub formula: Formula,
    /// directly requested in config (vs pulled in as a dependency)
    pub on_request: bool,
}

/// the `variations` entry that applies to the bottle that will actually be
/// poured — the selected bottle tag, which may be older than the host's
fn dep_tag(formula: &Formula, host_tag: &str) -> String {
    formula
        .bottle_files()
        .and_then(|files| tag::select(files).map(|(tag, _)| tag))
        .unwrap_or_else(|| host_tag.to_string())
}

/// Resolve the runtime closure of `roots` into install order (dependencies
/// before dependents). Names are resolved through the API, so aliases map to
/// their canonical formula.
pub async fn resolve_closure(roots: &[String]) -> Result<Vec<ResolvedFormula>> {
    let host_tag = tag::host_tag();
    let mut formulae: HashMap<String, Formula> = HashMap::new();
    // alias (or canonical name) -> canonical name, so repeated alias
    // occurrences in the dep graph don't re-fetch from the API
    let mut canonical: HashMap<String, String> = HashMap::new();
    let mut on_request: HashSet<String> = HashSet::new();
    let mut queue: Vec<(String, bool)> = roots.iter().map(|r| (r.clone(), true)).collect();
    while let Some((name, requested)) = queue.pop() {
        let known = canonical.get(&name).cloned();
        let canonical_name = match known {
            Some(c) => c,
            None => {
                let formula = api::formula(&name).await?;
                let c = formula.name.clone();
                canonical.insert(name.clone(), c.clone());
                canonical.insert(c.clone(), c.clone());
                for alias in &formula.aliases {
                    canonical.insert(alias.clone(), c.clone());
                }
                if !formulae.contains_key(&c) {
                    let tag = dep_tag(&formula, &host_tag);
                    for dep in formula.dependencies_for(&tag) {
                        queue.push((dep.clone(), false));
                    }
                    formulae.insert(c.clone(), formula);
                }
                c
            }
        };
        if requested {
            on_request.insert(canonical_name);
        }
    }

    // depth-first post-order = dependencies first
    let mut sorted: Vec<ResolvedFormula> = vec![];
    let mut done: HashSet<String> = HashSet::new();
    let mut visiting: Vec<String> = vec![];
    #[allow(clippy::too_many_arguments)]
    fn visit(
        name: &str,
        host_tag: &str,
        formulae: &HashMap<String, Formula>,
        canonical: &HashMap<String, String>,
        done: &mut HashSet<String>,
        visiting: &mut Vec<String>,
        on_request: &HashSet<String>,
        sorted: &mut Vec<ResolvedFormula>,
    ) -> Result<()> {
        if done.contains(name) {
            return Ok(());
        }
        if visiting.iter().any(|n| n == name) {
            // dependency cycles exist in homebrew/core (rare, e.g. mutual
            // optional deps); break the cycle rather than erroring
            debug!("dependency cycle involving {name}, breaking");
            return Ok(());
        }
        let Some(formula) = formulae.get(name) else {
            bail!("unresolved dependency: {name}");
        };
        visiting.push(name.to_string());
        let tag = dep_tag(formula, host_tag);
        for dep in formula.dependencies_for(&tag) {
            let dep_name = canonical.get(dep).cloned().unwrap_or_else(|| dep.clone());
            visit(
                &dep_name, host_tag, formulae, canonical, done, visiting, on_request, sorted,
            )?;
        }
        visiting.pop();
        done.insert(name.to_string());
        sorted.push(ResolvedFormula {
            formula: formulae[name].clone(),
            on_request: on_request.contains(name),
        });
        Ok(())
    }
    let mut names: Vec<&String> = formulae.keys().collect();
    names.sort(); // deterministic order
    for name in names {
        visit(
            name,
            &host_tag,
            &formulae,
            &canonical,
            &mut done,
            &mut visiting,
            &on_request,
            &mut sorted,
        )?;
    }
    Ok(sorted)
}
