//! Runtime dependency closure resolution, topologically sorted (deps first).

use std::collections::{HashMap, HashSet};
use std::path::{Component, Path};

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
    resolve_closure_with_taps_mode(roots, api::FetchMode::Cached).await
}

pub(super) async fn resolve_closure_with_taps_mode(
    roots: &[PackageRequest],
    mode: api::FetchMode,
) -> Result<Vec<ResolvedFormula>> {
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
    resolve_closure_pairs(&roots, mode).await
}

/// Resolve the runtime closure of `roots` into install order (dependencies
/// before dependents). Names are resolved through the API, so aliases map to
/// their canonical formula.
async fn resolve_closure_pairs(
    roots: &[(String, Option<String>, Option<String>)],
    mode: api::FetchMode,
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
        validate_formula_key(&key)?;
        let known = canonical.get(&key).cloned();
        let canonical_key = match known {
            Some(c) => c,
            None => {
                let (formula, effective_tap_name, effective_tap_url) = match fetch_formula(
                    &key, requested, mode,
                )
                .await
                {
                    Ok(formula) => {
                        let effective_tap_name = match formula.tap.as_deref() {
                            Some("homebrew/core") => None,
                            Some(tap) => Some(tap.to_string()),
                            None => expected_tap_name(&key),
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
                        (api::formula_with_mode(&key.name, mode).await?, None, None)
                    }
                    Err(err) => return Err(err),
                };
                validate_formula_response_identity(&key, requested, &formula)?;
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
                        queue.push((dependency_key(dep, &canonical_key), false));
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
    struct VisitContext<'a> {
        host_tag: &'a str,
        formulae: &'a HashMap<FormulaKey, Formula>,
        raw_bases: &'a HashMap<FormulaKey, Option<String>>,
        canonical: &'a HashMap<FormulaKey, FormulaKey>,
        done: &'a mut HashSet<FormulaKey>,
        visiting: &'a mut Vec<FormulaKey>,
        on_request: &'a HashSet<FormulaKey>,
        sorted: &'a mut Vec<ResolvedFormula>,
    }
    fn visit(key: &FormulaKey, ctx: &mut VisitContext<'_>) -> Result<()> {
        if ctx.done.contains(key) {
            return Ok(());
        }
        if ctx.visiting.iter().any(|n| n == key) {
            // dependency cycles exist in homebrew/core (rare, e.g. mutual
            // optional deps); break the cycle rather than erroring
            debug!("dependency cycle involving {}, breaking", key.name);
            return Ok(());
        }
        let Some(formula) = ctx.formulae.get(key) else {
            bail!("unresolved dependency: {}", key.name);
        };
        ctx.visiting.push(key.clone());
        let tag = dep_tag(formula, ctx.host_tag);
        for dep in install_deps(formula, &tag) {
            let dep_key = dependency_key(dep, key);
            let dep_key = ctx.canonical.get(&dep_key).cloned().unwrap_or(dep_key);
            visit(&dep_key, ctx)?;
        }
        ctx.visiting.pop();
        ctx.done.insert(key.clone());
        ctx.sorted.push(ResolvedFormula {
            formula: ctx.formulae[key].clone(),
            tap_raw_base: ctx.raw_bases.get(key).cloned().flatten(),
            on_request: ctx.on_request.contains(key),
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
    let mut visit_ctx = VisitContext {
        host_tag: &host_tag,
        formulae: &formulae,
        raw_bases: &raw_bases,
        canonical: &canonical,
        done: &mut done,
        visiting: &mut visiting,
        on_request: &on_request,
        sorted: &mut sorted,
    };
    for key in keys {
        visit(&key, &mut visit_ctx)?;
    }
    Ok(sorted)
}

/// Validate every formula-controlled value that may become a filesystem path
/// before install planning constructs a Cellar, cache, or staging path.
pub(super) fn validate_formula_path_identity(formula: &Formula) -> Result<()> {
    validate_path_component(&formula.name, "formula name")?;
    validate_path_component(&formula.pkg_version()?, "formula package version")?;
    for alias in &formula.aliases {
        validate_path_component(alias, "formula alias")?;
    }
    for dependency in formula
        .dependencies
        .iter()
        .chain(&formula.build_dependencies)
        .chain(formula.variations.values().flat_map(|variation| {
            variation
                .dependencies
                .iter()
                .flatten()
                .chain(variation.build_dependencies.iter().flatten())
        }))
    {
        validate_formula_reference(dependency, "formula dependency name")?;
    }
    for conflict in formula.conflicts_with() {
        validate_path_component(conflict, "formula conflict name")?;
    }
    if let Some(tap) = formula.tap.as_deref() {
        validate_tap_name(tap, "formula response tap")?;
    }
    Ok(())
}

pub(super) fn formula_reference_name(reference: &str) -> &str {
    api::split_tap_name(reference)
        .map(|(_, _, formula)| formula)
        .unwrap_or(reference)
}

fn validate_formula_reference(value: &str, label: &str) -> Result<()> {
    if let Some((owner, tap, formula)) = api::split_tap_name(value) {
        validate_path_component(owner, label)?;
        validate_path_component(tap, label)?;
        validate_path_component(formula, label)?;
        return Ok(());
    }
    validate_path_component(value, label)
}

fn validate_path_component(value: &str, label: &str) -> Result<()> {
    let mut components = Path::new(value).components();
    let normal = matches!(components.next(), Some(Component::Normal(_)));
    if value.is_empty() || value.contains(['/', '\\']) || !normal || components.next().is_some() {
        bail!("invalid {label} {value:?}: expected one normal path component");
    }
    Ok(())
}

fn validate_tap_name<'a>(tap: &'a str, label: &str) -> Result<(&'a str, &'a str)> {
    let mut parts = tap.split('/');
    let owner = parts.next().unwrap_or_default();
    let repository = parts.next().unwrap_or_default();
    if parts.next().is_some() {
        bail!("invalid {label} {tap:?}: expected owner/tap");
    }
    validate_path_component(owner, label)?;
    validate_path_component(repository, label)?;
    Ok((owner, repository))
}

fn validate_formula_key(key: &FormulaKey) -> Result<()> {
    validate_formula_reference(&key.name, "requested formula name")?;
    if let Some(tap) = key.tap_name.as_deref() {
        validate_tap_name(tap, "requested tap")?;
    }
    if let Some((owner, tap, _)) = api::split_tap_name(&key.name) {
        let qualified_tap = format!("{owner}/{tap}");
        if let Some(configured_tap) = key.tap_name.as_deref()
            && configured_tap != qualified_tap
        {
            bail!(
                "requested formula tap identity mismatch: name uses {qualified_tap:?}, request context uses {configured_tap:?}"
            );
        }
    }
    Ok(())
}

fn expected_tap_name(key: &FormulaKey) -> Option<String> {
    api::split_tap_name(&key.name)
        .map(|(owner, tap, _)| format!("{owner}/{tap}"))
        .filter(|tap| tap != "homebrew/core")
        .or_else(|| key.tap_name.clone())
}

fn dependency_key(dependency: &str, parent: &FormulaKey) -> FormulaKey {
    let Some((owner, tap, _)) = api::split_tap_name(dependency) else {
        return FormulaKey::new(
            dependency.to_string(),
            parent.tap_name.clone(),
            parent.tap_url.clone(),
        );
    };
    let tap_name = (owner != "homebrew" || tap != "core").then(|| format!("{owner}/{tap}"));
    let tap_url = (tap_name == parent.tap_name)
        .then(|| parent.tap_url.clone())
        .flatten();
    FormulaKey::new(dependency.to_string(), tap_name, tap_url)
}

fn validate_formula_response_identity(
    key: &FormulaKey,
    requested: bool,
    formula: &Formula,
) -> Result<()> {
    validate_formula_path_identity(formula)?;
    let requested_name = api::split_tap_name(&key.name)
        .map(|(_, _, formula)| formula)
        .unwrap_or(&key.name);
    if formula.name != requested_name
        && !formula.aliases.iter().any(|alias| alias == requested_name)
    {
        bail!(
            "brew metadata identity mismatch: requested {requested_name:?}, response names canonical formula {:?} with no matching alias",
            formula.name
        );
    }

    let qualified_tap =
        api::split_tap_name(&key.name).map(|(owner, tap, _)| format!("{owner}/{tap}"));
    let expected_tap = qualified_tap.as_deref().or(key.tap_name.as_deref());
    let Some(response_tap) = formula.tap.as_deref() else {
        // The exact configured/derived API URL and canonical response name
        // still bind an explicit tapped request when the optional `tap` field
        // is absent. An inherited dependency is tried against core first, so
        // absence there is ambiguous and must fail closed.
        if expected_tap.is_some() && qualified_tap.is_none() && !requested {
            bail!(
                "brew metadata tap identity is absent for inherited dependency {:?}; refusing ambiguous core/tap provenance",
                formula.name
            );
        }
        return Ok(());
    };
    let response_matches = match expected_tap {
        Some(expected) if qualified_tap.is_some() || requested => response_tap == expected,
        Some(expected) => response_tap == expected || response_tap == "homebrew/core",
        None => response_tap == "homebrew/core",
    };
    if !response_matches {
        bail!(
            "brew metadata tap identity mismatch for {:?}: expected {}, response names {response_tap:?}",
            formula.name,
            expected_tap.unwrap_or("homebrew/core")
        );
    }
    Ok(())
}

async fn fetch_formula(key: &FormulaKey, requested: bool, mode: api::FetchMode) -> Result<Formula> {
    if !requested && key.tap_name.is_some() && api::split_tap_name(&key.name).is_none() {
        match api::formula_with_mode(&key.name, mode).await {
            Ok(formula) => return Ok(formula),
            Err(err) => {
                debug!(
                    "brew: {} unavailable in core metadata ({err}); trying parent tap metadata",
                    key.name
                );
            }
        }
    }
    api::formula_with_tap_name_mode(
        &key.name,
        key.tap_name.as_deref(),
        key.tap_url.as_deref(),
        mode,
    )
    .await
}

fn tap_raw_base(key: &FormulaKey) -> Option<String> {
    let tap_name = key.tap_name.as_ref()?;
    let formula_name = format!("{tap_name}/x");
    let (owner, tap, _) = api::split_tap_name(&formula_name)?;
    api::tap_raw_base(owner, tap, key.tap_url.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn formula(name: &str, version: &str, tap: Option<&str>, aliases: &[&str]) -> Formula {
        serde_json::from_value(json!({
            "name": name,
            "tap": tap,
            "aliases": aliases,
            "versions": {"stable": version},
        }))
        .unwrap()
    }

    #[test]
    fn formula_path_identity_requires_single_normal_components() {
        for name in ["", ".", "..", "../escape", "/tmp/escape", "a/b", "a\\b"] {
            let error = validate_formula_path_identity(&formula(name, "1.0", None, &[]))
                .unwrap_err()
                .to_string();
            assert!(error.contains("expected one normal path component"));
        }
        for version in ["", ".", "..", "../escape", "/tmp/escape", "1/2", "1\\2"] {
            let error = validate_formula_path_identity(&formula("safe", version, None, &[]))
                .unwrap_err()
                .to_string();
            assert!(error.contains("expected one normal path component"));
        }
        validate_formula_path_identity(&formula("postgresql@17", "17.6_1", None, &[])).unwrap();

        let qualified_dependency: Formula = serde_json::from_value(json!({
            "name": "widget",
            "versions": {"stable": "1.0"},
            "dependencies": ["other/tools/helper"],
        }))
        .unwrap();
        validate_formula_path_identity(&qualified_dependency).unwrap();

        for request in ["", ".", "..", "../escape", "/tmp/escape", "a/b", "a\\b"] {
            assert!(validate_formula_key(&FormulaKey::new(request.into(), None, None)).is_err());
        }
    }

    #[test]
    fn formula_response_name_must_match_request_or_declared_alias() {
        let key = FormulaKey::new("alias".into(), None, None);
        let canonical = formula("canonical", "1.0", Some("homebrew/core"), &["alias"]);
        validate_formula_response_identity(&key, true, &canonical).unwrap();

        let mismatched = formula("other", "1.0", Some("homebrew/core"), &[]);
        let error = validate_formula_response_identity(&key, true, &mismatched)
            .unwrap_err()
            .to_string();
        assert!(error.contains("metadata identity mismatch"));
    }

    #[test]
    fn formula_response_tap_must_match_qualified_request() {
        let key = FormulaKey::new(
            "owner/tools/widget".into(),
            Some("owner/tools".into()),
            None,
        );
        validate_formula_response_identity(
            &key,
            true,
            &formula("widget", "1.0", Some("owner/tools"), &[]),
        )
        .unwrap();

        let error = validate_formula_response_identity(
            &key,
            true,
            &formula("widget", "1.0", Some("attacker/tools"), &[]),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("tap identity mismatch"));

        // Some tap-generated API documents omit `tap`; the exact requested
        // URL plus response-name binding remains authoritative in that case.
        validate_formula_response_identity(&key, true, &formula("widget", "1.0", None, &[]))
            .unwrap();
    }

    #[test]
    fn qualified_dependency_uses_its_own_tap_context() {
        let parent = FormulaKey::new(
            "parent".into(),
            Some("owner/parent".into()),
            Some("https://github.com/owner/homebrew-parent".into()),
        );
        let dependency = dependency_key("other/tools/helper", &parent);
        assert_eq!(dependency.name, "other/tools/helper");
        assert_eq!(formula_reference_name(&dependency.name), "helper");
        assert_eq!(dependency.tap_name.as_deref(), Some("other/tools"));
        assert_eq!(dependency.tap_url, None);

        let sibling = dependency_key("owner/parent/helper", &parent);
        assert_eq!(sibling.tap_name.as_deref(), Some("owner/parent"));
        assert_eq!(sibling.tap_url, parent.tap_url);

        let mismatched = FormulaKey::new(
            "other/tools/helper".into(),
            Some("attacker/tools".into()),
            None,
        );
        assert!(validate_formula_key(&mismatched).is_err());
    }

    #[test]
    fn inherited_tap_dependency_may_resolve_to_core_only() {
        let key = FormulaKey::new("openssl@3".into(), Some("owner/tools".into()), None);
        validate_formula_response_identity(
            &key,
            false,
            &formula("openssl@3", "3.6.0", Some("homebrew/core"), &[]),
        )
        .unwrap();
        assert!(
            validate_formula_response_identity(
                &key,
                false,
                &formula("openssl@3", "3.6.0", Some("attacker/tools"), &[]),
            )
            .is_err()
        );
        assert!(
            validate_formula_response_identity(
                &key,
                false,
                &formula("openssl@3", "3.6.0", None, &[]),
            )
            .is_err()
        );
    }
}
