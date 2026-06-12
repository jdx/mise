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

/// Resolve the runtime closure of `roots` into install order (dependencies
/// before dependents). Names are resolved through the API, so aliases map to
/// their canonical formula.
pub async fn resolve_closure(roots: &[String]) -> Result<Vec<ResolvedFormula>> {
    let host_tag = tag::host_tag();
    let mut formulae: HashMap<String, Formula> = HashMap::new();
    let mut on_request: HashSet<String> = HashSet::new();
    let mut queue: Vec<(String, bool)> = roots.iter().map(|r| (r.clone(), true)).collect();
    while let Some((name, requested)) = queue.pop() {
        if formulae.contains_key(&name) {
            if requested {
                on_request.insert(name);
            }
            continue;
        }
        let formula = api::formula(&name).await?;
        let canonical = formula.name.clone();
        if requested {
            on_request.insert(canonical.clone());
        }
        if canonical != name {
            // alias — track under the canonical name
            if formulae.contains_key(&canonical) {
                continue;
            }
        }
        for dep in formula.dependencies_for(&host_tag) {
            queue.push((dep.clone(), false));
        }
        formulae.insert(canonical, formula);
    }

    // depth-first post-order = dependencies first
    let mut sorted: Vec<ResolvedFormula> = vec![];
    let mut done: HashSet<String> = HashSet::new();
    let mut visiting: Vec<String> = vec![];
    fn visit(
        name: &str,
        host_tag: &str,
        formulae: &HashMap<String, Formula>,
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
        for dep in formula.dependencies_for(host_tag) {
            // deps may be recorded under an alias; resolve to canonical via map
            let dep_name = if formulae.contains_key(dep) {
                dep.clone()
            } else {
                // alias was fetched and stored under canonical name; find it
                formulae
                    .values()
                    .find(|f| f.aliases.iter().any(|a| a == dep))
                    .map(|f| f.name.clone())
                    .unwrap_or_else(|| dep.clone())
            };
            visit(
                &dep_name, host_tag, formulae, done, visiting, on_request, sorted,
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
            &mut done,
            &mut visiting,
            &on_request,
            &mut sorted,
        )?;
    }
    Ok(sorted)
}
