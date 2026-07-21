//! Runtime dependency closure resolution, topologically sorted (deps first).

use std::collections::{HashMap, HashSet};

use eyre::bail;

use super::api::{self, Formula};
use super::tag;
use crate::result::Result;
use crate::system::packages::PackageRequest;

#[derive(Debug, Clone)]
pub struct ResolvedFormula {
    pub formula: Formula,
    pub tap_raw_base: Option<String>,
    /// directly requested in config (vs pulled in as a dependency)
    pub on_request: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct FormulaKey {
    name: String,
    tap_name: Option<String>,
    tap_url: Option<String>,
}

impl FormulaKey {
    fn new(name: String, tap_name: Option<String>, tap_url: Option<String>) -> Self {
        Self {
            name,
            tap_name,
            tap_url,
        }
    }
}

/// The `variations` entry that applies to what will actually be installed:
/// the selected bottle tag (which may be older than the host's), or the
/// host's own tag for formulae that will be built from source. Shared with
/// source.rs so the build environment walks the same dependency lists this
/// resolution installed.
pub fn dep_tag(formula: &Formula, host_tag: &str) -> String {
    if super::source::has_bottle(formula)
        && let Some((tag, _)) = formula.bottle_files().and_then(tag::select)
    {
        return tag;
    }
    host_tag.to_string()
}

/// dependencies that must be installed before this formula: runtime deps
/// always, plus build deps when the formula will be built from source
fn install_deps<'a>(formula: &'a Formula, tag: &str) -> Vec<&'a String> {
    let mut deps: Vec<&String> = formula.dependencies_for(tag).iter().collect();
    if !super::source::has_bottle(formula) {
        deps.extend(formula.build_dependencies_for(tag));
    }
    deps
}

pub async fn resolve_closure_with_taps(roots: &[PackageRequest]) -> Result<Vec<ResolvedFormula>> {
    let roots = roots
        .iter()
        .map(|req| {
            (
                req.name.clone(),
                api::tap_name(&req.name),
                req.tap_url.clone(),
            )
        })
        .collect::<Vec<_>>();
    resolve_closure_pairs(&roots).await
}

/// Resolve the runtime closure of `roots` into install order (dependencies
/// before dependents). Names are resolved through the API, so aliases map to
/// their canonical formula.
async fn resolve_closure_pairs(
    roots: &[(String, Option<String>, Option<String>)],
) -> Result<Vec<ResolvedFormula>> {
    let host_tag = tag::host_tag();
    let mut formulae: HashMap<FormulaKey, Formula> = HashMap::new();
    let mut raw_bases: HashMap<FormulaKey, Option<String>> = HashMap::new();
    // alias (or canonical name) -> canonical name, so repeated alias
    // occurrences in the dep graph don't re-fetch from the API
    let mut canonical: HashMap<FormulaKey, FormulaKey> = HashMap::new();
    let mut on_request: HashSet<FormulaKey> = HashSet::new();
    let mut queue: Vec<(FormulaKey, bool)> = roots
        .iter()
        .map(|(name, tap_name, tap_url)| {
            (
                FormulaKey::new(name.clone(), tap_name.clone(), tap_url.clone()),
                true,
            )
        })
        .collect();
    while let Some((key, requested)) = queue.pop() {
        let known = canonical.get(&key).cloned();
        let canonical_key = match known {
            Some(c) => c,
            None => {
                let (formula, effective_tap_name, effective_tap_url) = match fetch_formula(
                    &key, requested,
                )
                .await
                {
                    Ok(formula) => {
                        let effective_tap_name = match formula.tap.as_deref() {
                            Some("homebrew/core") => None,
                            Some(tap) => Some(tap.to_string()),
                            None => key.tap_name.clone(),
                        };
                        let effective_tap_url =
                            effective_tap_name.as_ref().and(key.tap_url.clone());
                        (formula, effective_tap_name, effective_tap_url)
                    }
                    Err(err)
                        if key.tap_name.is_some() && api::split_tap_name(&key.name).is_none() =>
                    {
                        debug!(
                            "brew: {} unavailable in tap metadata ({err}); falling back to core metadata",
                            key.name
                        );
                        (api::formula(&key.name).await?, None, None)
                    }
                    Err(err) => return Err(err),
                };
                let c = formula.name.clone();
                let canonical_key = FormulaKey::new(
                    c.clone(),
                    effective_tap_name.clone(),
                    effective_tap_url.clone(),
                );
                canonical.insert(key.clone(), canonical_key.clone());
                canonical.insert(canonical_key.clone(), canonical_key.clone());
                for alias in &formula.aliases {
                    canonical.insert(
                        FormulaKey::new(
                            alias.clone(),
                            effective_tap_name.clone(),
                            effective_tap_url.clone(),
                        ),
                        canonical_key.clone(),
                    );
                }
                if !formulae.contains_key(&canonical_key) {
                    let tag = dep_tag(&formula, &host_tag);
                    for dep in install_deps(&formula, &tag) {
                        queue.push((
                            FormulaKey::new(
                                dep.clone(),
                                effective_tap_name.clone(),
                                effective_tap_url.clone(),
                            ),
                            false,
                        ));
                    }
                    raw_bases.insert(canonical_key.clone(), tap_raw_base(&canonical_key));
                    formulae.insert(canonical_key.clone(), formula);
                }
                canonical_key
            }
        };
        if requested {
            on_request.insert(canonical_key);
        }
    }

    // depth-first post-order = dependencies first
    let mut sorted: Vec<ResolvedFormula> = vec![];
    let mut done: HashSet<FormulaKey> = HashSet::new();
    let mut visiting: Vec<FormulaKey> = vec![];
    #[allow(clippy::too_many_arguments)]
    fn visit(
        key: &FormulaKey,
        host_tag: &str,
        formulae: &HashMap<FormulaKey, Formula>,
        raw_bases: &HashMap<FormulaKey, Option<String>>,
        canonical: &HashMap<FormulaKey, FormulaKey>,
        done: &mut HashSet<FormulaKey>,
        visiting: &mut Vec<FormulaKey>,
        on_request: &HashSet<FormulaKey>,
        sorted: &mut Vec<ResolvedFormula>,
    ) -> Result<()> {
        if done.contains(key) {
            return Ok(());
        }
        if visiting.iter().any(|n| n == key) {
            // dependency cycles exist in homebrew/core (rare, e.g. mutual
            // optional deps); break the cycle rather than erroring
            debug!("dependency cycle involving {}, breaking", key.name);
            return Ok(());
        }
        let Some(formula) = formulae.get(key) else {
            bail!("unresolved dependency: {}", key.name);
        };
        visiting.push(key.clone());
        let tag = dep_tag(formula, host_tag);
        for dep in install_deps(formula, &tag) {
            let dep_key = FormulaKey::new(dep.clone(), key.tap_name.clone(), key.tap_url.clone());
            let dep_key = canonical.get(&dep_key).cloned().unwrap_or(dep_key);
            visit(
                &dep_key, host_tag, formulae, raw_bases, canonical, done, visiting, on_request,
                sorted,
            )?;
        }
        visiting.pop();
        done.insert(key.clone());
        sorted.push(ResolvedFormula {
            formula: formulae[key].clone(),
            tap_raw_base: raw_bases.get(key).cloned().flatten(),
            on_request: on_request.contains(key),
        });
        Ok(())
    }
    let mut keys: Vec<FormulaKey> = formulae.keys().cloned().collect();
    keys.sort_by(|a, b| {
        a.tap_name
            .cmp(&b.tap_name)
            .then_with(|| a.tap_url.cmp(&b.tap_url))
            .then_with(|| a.name.cmp(&b.name))
    }); // deterministic order
    for key in keys {
        visit(
            &key,
            &host_tag,
            &formulae,
            &raw_bases,
            &canonical,
            &mut done,
            &mut visiting,
            &on_request,
            &mut sorted,
        )?;
    }
    Ok(sorted)
}

async fn fetch_formula(key: &FormulaKey, requested: bool) -> Result<Formula> {
    if !requested && key.tap_name.is_some() && api::split_tap_name(&key.name).is_none() {
        match api::formula(&key.name).await {
            Ok(formula) => return Ok(formula),
            Err(err) => {
                debug!(
                    "brew: {} unavailable in core metadata ({err}); trying parent tap metadata",
                    key.name
                );
            }
        }
    }
    api::formula_with_tap_name(&key.name, key.tap_name.as_deref(), key.tap_url.as_deref()).await
}

fn tap_raw_base(key: &FormulaKey) -> Option<String> {
    let tap_name = key.tap_name.as_ref()?;
    let formula_name = format!("{tap_name}/x");
    let (owner, tap, _) = api::split_tap_name(&formula_name)?;
    api::tap_raw_base(owner, tap, key.tap_url.as_deref())
}
