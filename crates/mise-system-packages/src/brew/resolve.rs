//! Runtime dependency closure resolution, topologically sorted (deps first).

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap, HashSet};

use eyre::{Report, bail};
use futures_util::stream::{self, StreamExt};

use super::api::{self, Formula};
use super::tag;
use crate::PackageRequest;
use eyre::Result;
use mise_settings::Settings;

#[derive(Debug, Clone)]
pub(super) struct ResolvedFormula {
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

    fn dependency(&self, name: &str, tap_urls: &BTreeMap<String, String>) -> Self {
        if api::split_tap_name(name).is_none() {
            return Self::new(name.to_owned(), self.tap_name.clone(), self.tap_url.clone());
        }

        let tap_name = api::tap_name(name);
        let tap_url = tap_name.as_ref().and_then(|tap| {
            if Some(tap) == self.tap_name.as_ref() {
                self.tap_url.clone()
            } else {
                tap_urls.get(tap).cloned()
            }
        });
        Self::new(name.to_owned(), tap_name, tap_url)
    }
}

fn accept_resolved_alias_failures(
    failed: Vec<(FormulaKey, bool, Report)>,
    canonical: &HashMap<FormulaKey, FormulaKey>,
    on_request: &mut HashSet<FormulaKey>,
) -> Result<()> {
    for (key, requested, err) in failed {
        let Some(canonical_key) = canonical.get(&key).cloned() else {
            return Err(err);
        };
        if requested {
            on_request.insert(canonical_key);
        }
    }
    Ok(())
}

/// Every name in the closure mapped to the formula it refers to. A name
/// claimed by several formulae resolves by canonical name, then alias, then
/// old name, the same precedence as the alias index.
pub(super) fn formulae_by_name(
    closure: &[ResolvedFormula],
) -> HashMap<Cow<'_, str>, &ResolvedFormula> {
    fn names<'a>(formula: &Formula, name: &'a str) -> [Cow<'a, str>; 2] {
        let tap = formula.tap.as_deref().unwrap_or("homebrew/core");
        [Cow::Borrowed(name), Cow::Owned(format!("{tap}/{name}"))]
    }

    let mut by_name = HashMap::new();
    for rf in closure {
        for name in names(&rf.formula, &rf.formula.name) {
            by_name.insert(name, rf);
        }
    }
    for rf in closure {
        for alias in &rf.formula.aliases {
            for name in names(&rf.formula, alias) {
                by_name.entry(name).or_insert(rf);
            }
        }
    }
    for rf in closure {
        for oldname in &rf.formula.oldnames {
            for name in names(&rf.formula, oldname) {
                by_name.entry(name).or_insert(rf);
            }
        }
    }
    by_name
}

/// The `variations` entry that applies to what will actually be installed:
/// the selected bottle tag (which may be older than the host's), or the
/// host's own tag for formulae that will be built from source. Shared with
/// source.rs so the build environment walks the same dependency lists this
/// resolution installed.
pub(super) fn dep_tag(formula: &Formula, host_tag: &str) -> String {
    if super::source::has_bottle(formula)
        && let Some((tag, _)) = super::source::select_bottle(formula)
    {
        return tag;
    }
    host_tag.to_string()
}

/// Runtime dependencies, including libraries Homebrew gets from macOS but
/// must install separately on Linux.
pub(super) fn runtime_deps<'a>(formula: &'a Formula, tag: &str) -> Vec<&'a String> {
    let mut deps: Vec<&String> = formula.dependencies_for(tag).iter().collect();
    if cfg!(target_os = "linux") {
        deps.extend(
            formula
                .uses_from_macos_for(tag)
                .iter()
                .filter_map(|dep| dep.runtime_name()),
        );
    }
    deps
}

/// dependencies that must be installed before this formula: runtime deps
/// always, plus build deps when the formula will be built from source
pub(super) fn install_deps<'a>(formula: &'a Formula, tag: &str) -> Vec<&'a String> {
    let mut deps = runtime_deps(formula, tag);
    if !super::source::has_bottle(formula) {
        deps.extend(formula.build_dependencies_for(tag));
        if cfg!(target_os = "linux") {
            deps.extend(
                formula
                    .uses_from_macos_for(tag)
                    .iter()
                    .filter_map(|dep| dep.build_name()),
            );
        }
    }
    deps
}

pub(super) async fn resolve_closure_with_taps(
    roots: &[PackageRequest],
    configured_taps: &BTreeMap<String, String>,
    provision_ruby: bool,
) -> Result<Vec<ResolvedFormula>> {
    let mut tap_urls = configured_taps.clone();
    // Keep explicit request URLs available for callers without a tap config.
    for req in roots {
        if let (Some(tap), Some(url)) = (api::tap_name(&req.name), &req.tap_url) {
            tap_urls.entry(tap).or_insert_with(|| url.clone());
        }
    }
    let roots = roots
        .iter()
        .map(|req| {
            let tap_name = api::tap_name(&req.name)
                .or_else(|| req.tap_url.as_deref().and_then(api::tap_name_from_url));
            let tap_url = req
                .tap_url
                .clone()
                .or_else(|| tap_name.as_ref().and_then(|tap| tap_urls.get(tap).cloned()));
            FormulaKey::new(req.name.clone(), tap_name, tap_url)
        })
        .collect::<Vec<_>>();
    resolve_closure(&roots, &tap_urls, provision_ruby).await
}

/// Resolve the runtime closure of `roots` into install order (dependencies
/// before dependents). Names are resolved through the API, so aliases map to
/// their canonical formula.
async fn resolve_closure(
    roots: &[FormulaKey],
    tap_urls: &BTreeMap<String, String>,
    provision_ruby: bool,
) -> Result<Vec<ResolvedFormula>> {
    let host_tag = tag::host_tag();
    let mut formulae: HashMap<FormulaKey, Formula> = HashMap::new();
    let mut raw_bases: HashMap<FormulaKey, Option<String>> = HashMap::new();
    // alias (or canonical name) -> canonical name, so repeated alias
    // occurrences in the dep graph don't re-fetch from the API
    let mut canonical: HashMap<FormulaKey, FormulaKey> = HashMap::new();
    let mut on_request: HashSet<FormulaKey> = HashSet::new();
    let mut queue: Vec<(FormulaKey, bool)> = roots.iter().cloned().map(|key| (key, true)).collect();
    while !queue.is_empty() {
        // Resolve one dependency frontier at a time. Formulae within the
        // frontier are independent, so their metadata can be fetched
        // concurrently; the next frontier is discovered from these results.
        let mut pending: Vec<(FormulaKey, bool)> = vec![];
        let mut pending_positions: HashMap<FormulaKey, usize> = HashMap::new();
        while let Some((key, requested)) = queue.pop() {
            if let Some(canonical_key) = canonical.get(&key).cloned() {
                if requested {
                    on_request.insert(canonical_key);
                }
                continue;
            }
            if let Some(index) = pending_positions.get(&key).copied() {
                pending[index].1 |= requested;
            } else {
                pending_positions.insert(key.clone(), pending.len());
                pending.push((key, requested));
            }
        }

        let fetches: Vec<_> = pending
            .into_iter()
            .enumerate()
            .map(|(index, (key, requested))| async move {
                let result = fetch_formula_with_fallback(&key, requested, provision_ruby).await;
                (index, key, requested, result)
            })
            .collect();
        let mut fetched = stream::iter(fetches)
            .buffer_unordered(mise_util::jobs::normalize(Settings::get().jobs).max(1))
            .collect::<Vec<_>>()
            .await;
        // buffer_unordered returns completion order. Restore the frontier's
        // deterministic stack order before aliases and dependencies are added.
        fetched.sort_by_key(|(index, ..)| *index);

        let mut failed = vec![];
        for (_, key, requested, result) in fetched {
            let (formula, effective_tap_name, effective_tap_url) = match result {
                Ok(resolved) => resolved,
                Err(err) => {
                    failed.push((key, requested, err));
                    continue;
                }
            };
            if let Some(canonical_key) = canonical.get(&key).cloned() {
                if requested {
                    on_request.insert(canonical_key);
                }
                continue;
            }
            let canonical_key = FormulaKey::new(
                formula.name.clone(),
                effective_tap_name.clone(),
                effective_tap_url.clone(),
            );
            canonical.insert(key, canonical_key.clone());
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
            // an alias another formula already claimed outranks an old name
            for oldname in &formula.oldnames {
                canonical
                    .entry(FormulaKey::new(
                        oldname.clone(),
                        effective_tap_name.clone(),
                        effective_tap_url.clone(),
                    ))
                    .or_insert_with(|| canonical_key.clone());
            }
            if !formulae.contains_key(&canonical_key) {
                let tag = dep_tag(&formula, &host_tag);
                for dep in install_deps(&formula, &tag) {
                    queue.push((canonical_key.dependency(dep, tap_urls), false));
                }
                raw_bases.insert(canonical_key.clone(), tap_raw_base(&canonical_key));
                formulae.insert(canonical_key.clone(), formula);
            }
            if requested {
                on_request.insert(canonical_key);
            }
        }
        // A frontier may contain both a canonical name and one of its aliases
        // before either response teaches us that they are the same formula.
        // Accept a failed redundant request when another response resolved its
        // key, but preserve errors for genuinely unresolved formulae.
        accept_resolved_alias_failures(failed, &canonical, &mut on_request)?;
    }

    // depth-first post-order = dependencies first
    let mut sorted: Vec<ResolvedFormula> = vec![];
    let mut done: HashSet<FormulaKey> = HashSet::new();
    let mut visiting: Vec<FormulaKey> = vec![];
    struct VisitContext<'a> {
        host_tag: &'a str,
        tap_urls: &'a BTreeMap<String, String>,
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
            let dep_key = key.dependency(dep, ctx.tap_urls);
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
        tap_urls,
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

async fn fetch_formula_with_fallback(
    key: &FormulaKey,
    requested: bool,
    provision_ruby: bool,
) -> Result<(Formula, Option<String>, Option<String>)> {
    if !requested && key.tap_name.is_some() && api::split_tap_name(&key.name).is_none() {
        // Exact names only: a sibling formula from the same tap 404s here,
        // and resolving that through the alias index would download it for
        // nothing. Aliases are resolved by the core fallback below.
        match api::formula_exact(&key.name).await {
            Ok(formula) => return Ok((formula, None, None)),
            Err(err) => {
                debug!(
                    "brew: {} unavailable in core metadata ({err}); trying parent tap metadata",
                    key.name
                );
            }
        }
    }
    match api::formula_with_tap_name(
        &key.name,
        key.tap_name.as_deref(),
        key.tap_url.as_deref(),
        provision_ruby,
    )
    .await
    {
        Ok(mut formula) => {
            let effective_tap_name = match formula.tap.as_deref() {
                Some("homebrew/core") => None,
                Some(tap) => Some(tap.to_string()),
                None => key.tap_name.clone(),
            };
            let effective_tap_url = effective_tap_name.as_ref().and(key.tap_url.clone());
            if formula.tap.is_none() {
                formula.tap.clone_from(&effective_tap_name);
            }
            Ok((formula, effective_tap_name, effective_tap_url))
        }
        Err(err) if key.tap_name.is_some() && api::split_tap_name(&key.name).is_none() => {
            debug!(
                "brew: {} unavailable in tap metadata ({err}); falling back to core metadata",
                key.name
            );
            Ok((api::formula(&key.name).await?, None, None))
        }
        Err(err) => Err(err),
    }
    .and_then(|resolved| {
        validate_formula_identity(&resolved.0)?;
        Ok(resolved)
    })
}

/// Fetched metadata names the formula's rack and keg (`Cellar/<name>/<version>`),
/// so its name and version each have to be one plain path component.
fn validate_formula_identity(formula: &Formula) -> Result<()> {
    let fields = [
        ("name", Some(&formula.name)),
        ("version", formula.versions.stable.as_ref()),
    ];
    for (field, value) in fields {
        if let Some(value) = value
            && !super::is_normal_formula_component(value)
        {
            bail!(
                "brew: refusing formula metadata whose {field} {value:?} is not a single path \
                 component; mise installs a formula into Cellar/<name>/<version>, so fix the \
                 formula's metadata in its tap or API source"
            );
        }
    }
    Ok(())
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

    fn resolved(json: serde_json::Value) -> ResolvedFormula {
        ResolvedFormula {
            formula: serde_json::from_value(json).unwrap(),
            tap_raw_base: None,
            on_request: false,
        }
    }

    #[test]
    fn formula_identity_must_be_one_path_component_each() {
        let formula = |name: &str, stable: Option<&str>| -> Formula {
            serde_json::from_value(serde_json::json!({
                "name": name, "versions": {"stable": stable},
            }))
            .unwrap()
        };
        for (name, stable) in [
            ("hello", Some("1.0")),
            ("openssl@3", Some("3.6.0")),
            ("libfoo", None),
        ] {
            validate_formula_identity(&formula(name, stable)).unwrap();
        }
        for (name, stable) in [
            ("/home/alice/", Some("Documents")),
            ("helper/1.0/home", Some(".ssh")),
            ("../outside", Some("1.0")),
            ("hello/", Some("1.0")),
            ("a\\b", Some("1.0")),
            ("a\0b", Some("1.0")),
            ("", Some("1.0")),
            ("hello", Some("..")),
            ("hello", Some(".")),
            ("hello", Some("1.0/../..")),
            ("hello", Some("/tmp")),
        ] {
            let err = validate_formula_identity(&formula(name, stable)).unwrap_err();
            assert!(err.to_string().contains("single path component"), "{err}");
        }
    }

    #[test]
    fn formula_dependencies_use_their_own_explicit_tap() {
        let parent_url = "https://github.com/qmk/homebrew-qmk.git";
        let avr_url = "https://github.com/example/custom-avr.git";
        let tap_urls = BTreeMap::from([("osx-cross/avr".into(), avr_url.into())]);
        let parent = FormulaKey::new(
            "qmk".into(),
            Some("qmk/qmk".into()),
            Some(parent_url.into()),
        );

        for (name, tap, url) in [
            ("unqualified", Some("qmk/qmk"), Some(parent_url)),
            ("qmk/qmk/sibling", Some("qmk/qmk"), Some(parent_url)),
            (
                "osx-cross/avr/avr-gcc@8",
                Some("osx-cross/avr"),
                Some(avr_url),
            ),
            (
                "osx-cross/arm/arm-none-eabi-gcc@8",
                Some("osx-cross/arm"),
                None,
            ),
            ("homebrew/core/git", None, None),
        ] {
            assert_eq!(
                parent.dependency(name, &tap_urls),
                FormulaKey::new(name.into(), tap.map(str::to_owned), url.map(str::to_owned))
            );
        }
    }

    #[test]
    fn formulae_by_name_prefers_canonical_then_alias_then_old_name() {
        let closure = [
            resolved(serde_json::json!({
                "name": "renamed", "oldnames": ["shared", "openssl@3"],
                "tap": "example/tap",
                "versions": {"stable": "1"},
            })),
            resolved(serde_json::json!({
                "name": "openssl@3", "aliases": ["shared"],
                "versions": {"stable": "3"},
            })),
        ];
        let by_name = formulae_by_name(&closure);
        let name_of = |n: &str| by_name[n].formula.name.as_str();
        assert_eq!(name_of("openssl@3"), "openssl@3");
        assert_eq!(name_of("shared"), "openssl@3");
        assert_eq!(name_of("renamed"), "renamed");
        assert_eq!(name_of("example/tap/renamed"), "renamed");
        assert_eq!(name_of("example/tap/shared"), "renamed");
        assert_eq!(name_of("homebrew/core/shared"), "openssl@3");
    }

    #[test]
    fn linux_bottle_includes_uses_from_macos_runtime_dependencies_only() {
        let formula: Formula = serde_json::from_value(serde_json::json!({
            "name": "freetype",
            "versions": {"stable": "1.0"},
            "dependencies": ["libpng"],
            "build_dependencies": ["pkgconf"],
            "uses_from_macos": [
                "bzip2",
                {"bison": "build"},
                {"sqlite": "test"},
                {"optional-tool": "optional"},
                {"recommended-tool": "recommended"}
            ],
            "bottle": {
                "stable": {
                    "files": {
                        "x86_64_linux": {
                            "cellar": ":any",
                            "url": "https://example.com/freetype.tar.gz",
                            "sha256": "0000000000000000000000000000000000000000000000000000000000000000"
                        },
                        "arm64_linux": {
                            "cellar": ":any",
                            "url": "https://example.com/freetype-arm64.tar.gz",
                            "sha256": "0000000000000000000000000000000000000000000000000000000000000000"
                        }
                    }
                }
            },
            "variations": {
                "x86_64_linux": {"dependencies": ["libpng", "zlib-ng-compat"]},
                "arm64_linux": {"dependencies": ["libpng", "zlib-ng-compat"]}
            }
        }))
        .unwrap();

        if !cfg!(target_os = "linux") {
            return;
        }
        assert!(super::super::source::has_bottle(&formula));
        let tag = dep_tag(&formula, &tag::host_tag());
        let install: Vec<&str> = install_deps(&formula, &tag)
            .iter()
            .map(|name| name.as_str())
            .collect();

        assert_eq!(
            install,
            ["libpng", "zlib-ng-compat", "bzip2", "recommended-tool"]
        );
    }

    #[test]
    fn linux_source_build_includes_build_dependencies() {
        let formula: Formula = serde_json::from_value(serde_json::json!({
            "name": "freetype",
            "versions": {"stable": "1.0"},
            "dependencies": ["libpng"],
            "build_dependencies": ["pkgconf"],
            "uses_from_macos": [
                "bzip2",
                {"bison": "build"},
                {"sqlite": "test"},
                {"optional-tool": "optional"}
            ]
        }))
        .unwrap();

        let install: Vec<&str> = install_deps(&formula, &tag::host_tag())
            .iter()
            .map(|name| name.as_str())
            .collect();
        if cfg!(target_os = "linux") {
            assert_eq!(install, ["libpng", "bzip2", "pkgconf", "bison"]);
        } else {
            assert_eq!(install, ["libpng", "pkgconf"]);
        }
    }

    #[test]
    fn failed_alias_is_accepted_after_canonical_resolution() {
        let canonical_key = FormulaKey::new("canonical".into(), None, None);
        let alias_key = FormulaKey::new("alias".into(), None, None);
        let canonical = HashMap::from([(alias_key.clone(), canonical_key.clone())]);
        let mut on_request = HashSet::new();

        accept_resolved_alias_failures(
            vec![(alias_key, true, eyre::eyre!("redundant alias failed"))],
            &canonical,
            &mut on_request,
        )
        .unwrap();

        assert_eq!(on_request, HashSet::from([canonical_key]));
    }

    #[test]
    fn failed_unresolved_formula_is_preserved() {
        let key = FormulaKey::new("missing".into(), None, None);
        let err = accept_resolved_alias_failures(
            vec![(key, false, eyre::eyre!("metadata failed"))],
            &HashMap::new(),
            &mut HashSet::new(),
        )
        .unwrap_err();

        assert_eq!(err.to_string(), "metadata failed");
    }
}
