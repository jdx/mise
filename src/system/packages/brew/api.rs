//! Homebrew API client and tap Ruby fallback.

use std::collections::HashMap;
use std::fmt;

use eyre::{Report, WrapErr, bail};
use serde::Deserialize;

use crate::http::HTTP_FETCH;
use crate::result::Result;

pub(super) use mise_brew_metadata::*;

const API_BASE: &str = "https://formulae.brew.sh/api";

/// Fetch homebrew/core formula metadata by name, alias, or old name.
///
/// The per-formula API serves canonical names only: an alias (`openssl`) or a
/// renamed formula's old name returns 404 rather than redirecting. When the
/// exact lookup fails, the name is resolved through the bulk formula index and
/// the canonical formula is fetched instead; if the index has no entry for it,
/// the original error is returned.
pub(super) async fn formula(name: &str) -> Result<Formula> {
    formula_from(API_BASE, &FORMULA_ALIASES, name).await
}

async fn formula_from(base: &str, aliases: &AliasIndex, name: &str) -> Result<Formula> {
    let err = match formula_exact_from(base, name).await {
        Ok(formula) => return Ok(formula),
        Err(err) => err,
    };
    match canonical_formula_name(base, aliases, name).await {
        Ok(Some(canonical)) => {
            debug!("brew: {name} resolves to {canonical}");
            // keep the requested name outermost so callers find its config
            formula_exact_from(base, &canonical)
                .await
                .wrap_err_with(|| FormulaFetchFailed(name.to_string()))
        }
        Ok(None) => Err(with_cask_hint(base, name, err).await),
        Err(index_err) => {
            debug!("brew: could not load the formula index to resolve {name}: {index_err:#}");
            Err(with_cask_hint(base, name, err).await)
        }
    }
}

/// A formula name that 404s is often a cask declared as `brew:` instead of
/// `brew-cask:`; say so when the cask API knows the name.
async fn with_cask_hint(base: &str, name: &str, err: Report) -> Report {
    // json_cached keeps only the message of the underlying reqwest error
    if !err
        .chain()
        .any(|cause| cause.to_string().contains("(404 Not Found)"))
    {
        return err;
    }
    match HTTP_FETCH.head(format!("{base}/cask/{name}.json")).await {
        Ok(_) => err.wrap_err(format!(
            "'{name}' is a Homebrew cask, not a formula; declare it as \"brew-cask:{name}\""
        )),
        Err(cask_err) => {
            debug!("brew: {name} is not a cask either: {cask_err:#}");
            err
        }
    }
}

/// A homebrew/core formula whose metadata could not be fetched. Typed so that
/// callers can report which config declared the name.
#[derive(Debug)]
pub(super) struct FormulaFetchFailed(pub String);

impl fmt::Display for FormulaFetchFailed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "failed to fetch Homebrew formula '{}'", self.0)
    }
}

/// The formula name behind a failed homebrew/core metadata lookup, if `err`
/// came from one.
pub fn failed_formula_name(err: &Report) -> Option<&str> {
    err.downcast_ref::<FormulaFetchFailed>()
        .map(|failed| failed.0.as_str())
}

/// Fetch homebrew/core formula metadata by its canonical name only.
pub(super) async fn formula_exact(name: &str) -> Result<Formula> {
    formula_exact_from(API_BASE, name).await
}

async fn formula_exact_from(base: &str, name: &str) -> Result<Formula> {
    let url = format!("{base}/formula/{name}.json");
    HTTP_FETCH
        .json_cached::<Formula, _>(url)
        .await
        .wrap_err_with(|| FormulaFetchFailed(name.to_string()))
}

#[derive(Debug, Deserialize)]
struct FormulaIndexEntry {
    name: String,
    #[serde(default)]
    aliases: Vec<String>,
    #[serde(default)]
    oldnames: Vec<String>,
}

/// alias or old name -> canonical name, from the bulk `formula.json`, or why
/// it could not be loaded. Fetched at most once per process, and only after
/// an exact lookup has failed; a failure is kept so that every alias in a
/// dependency frontier does not retry the download.
type AliasIndex = tokio::sync::OnceCell<std::result::Result<HashMap<String, String>, String>>;

static FORMULA_ALIASES: AliasIndex = tokio::sync::OnceCell::const_new();

async fn canonical_formula_name(
    base: &str,
    aliases: &AliasIndex,
    name: &str,
) -> Result<Option<String>> {
    let aliases = aliases
        .get_or_init(|| async {
            HTTP_FETCH
                .json::<Vec<FormulaIndexEntry>, _>(format!("{base}/formula.json"))
                .await
                .map(alias_map)
                .map_err(|err| format!("{err:#}"))
        })
        .await;
    match aliases {
        Ok(aliases) => Ok(aliases.get(name).cloned()),
        Err(err) => bail!("failed to fetch the Homebrew formula index: {err}"),
    }
}

/// A name that is both an alias and an old name resolves as the alias.
fn alias_map(entries: Vec<FormulaIndexEntry>) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for entry in &entries {
        for alias in &entry.aliases {
            map.insert(alias.clone(), entry.name.clone());
        }
    }
    for entry in entries {
        for oldname in entry.oldnames {
            map.entry(oldname).or_insert_with(|| entry.name.clone());
        }
    }
    map
}

pub(super) async fn formula_with_tap_name(
    name: &str,
    tap_name: Option<&str>,
    tap_url: Option<&str>,
    provision_ruby: bool,
) -> Result<Formula> {
    let Some((owner, tap, formula_name)) = split_tap_name(name).or_else(|| {
        let (owner, tap) = split_tap(tap_name?)?;
        Some((owner, tap, name))
    }) else {
        return formula(name).await;
    };
    if owner == "homebrew" && tap == "core" {
        return formula(formula_name).await;
    }
    let Some(url) = tap_formula_api_url(owner, tap, formula_name, tap_url) else {
        bail!(
            "brew: tapped formula '{name}' needs a GitHub tap URL in [bootstrap.brew.taps] \
             so mise can fetch metadata directly without the brew CLI"
        );
    };
    match HTTP_FETCH.json_cached::<Formula, _>(url).await {
        Ok(formula) => Ok(formula),
        Err(api_err) => {
            super::tap::formula_from_ruby(owner, tap, formula_name, tap_url, provision_ruby)
                .await
                .wrap_err_with(|| {
                    format!(
                        "failed to resolve Homebrew tap formula '{name}'; published API metadata \
                     was unavailable ({api_err}) and mise could not evaluate its tap definition"
                    )
                })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alias_map_resolves_aliases_before_old_names() {
        let entries: Vec<FormulaIndexEntry> = serde_json::from_value(serde_json::json!([
            {"name": "openssl@3", "aliases": ["openssl", "openssl@3.6"], "oldnames": []},
            {"name": "gitea-runner", "aliases": [], "oldnames": ["act_runner"]},
            {"name": "shadowed", "oldnames": ["openssl"]},
            {"name": "hello"},
        ]))
        .unwrap();
        let map = alias_map(entries);
        assert_eq!(map.get("openssl").map(String::as_str), Some("openssl@3"));
        assert_eq!(
            map.get("openssl@3.6").map(String::as_str),
            Some("openssl@3")
        );
        assert_eq!(
            map.get("act_runner").map(String::as_str),
            Some("gitea-runner")
        );
        assert_eq!(map.get("hello"), None);
    }

    #[tokio::test]
    async fn failed_lookup_resolves_through_the_alias_index_once() -> Result<()> {
        let mut server = mockito::Server::new_async().await;
        let base = server.url();
        let mut not_found = Vec::new();
        for path in ["/formula/openssl.json", "/formula/missing.json"] {
            not_found.push(
                server
                    .mock("GET", path)
                    .with_status(404)
                    .expect(1)
                    .create_async()
                    .await,
            );
        }
        let index = server
            .mock("GET", "/formula.json")
            .with_body(
                serde_json::json!([{"name": "openssl@3", "aliases": ["openssl"]}]).to_string(),
            )
            .expect(1)
            .create_async()
            .await;
        let canonical = server
            .mock("GET", "/formula/openssl@3.json")
            .with_body(
                serde_json::json!({"name": "openssl@3", "versions": {"stable": "3.6.4"}})
                    .to_string(),
            )
            .expect(1)
            .create_async()
            .await;
        let aliases = AliasIndex::const_new();

        assert_eq!(
            formula_from(&base, &aliases, "openssl").await?.name,
            "openssl@3"
        );
        let err = formula_from(&base, &aliases, "missing").await.unwrap_err();
        assert!(format!("{err:#}").contains("missing.json"), "{err:#}");

        for mock in not_found.iter().chain([&index, &canonical]) {
            mock.assert_async().await;
        }
        Ok(())
    }

    #[tokio::test]
    async fn missing_formula_that_is_a_cask_suggests_brew_cask() -> Result<()> {
        let mut server = mockito::Server::new_async().await;
        let base = server.url();
        server
            .mock("GET", mockito::Matcher::Regex("^/formula/".into()))
            .with_status(404)
            .create_async()
            .await;
        server
            .mock("GET", "/formula.json")
            .with_body("[]")
            .create_async()
            .await;
        server
            .mock("HEAD", "/cask/1password-cli.json")
            .with_status(200)
            .create_async()
            .await;
        server
            .mock("HEAD", "/cask/missing.json")
            .with_status(404)
            .create_async()
            .await;
        let aliases = AliasIndex::const_new();

        let err = formula_from(&base, &aliases, "1password-cli")
            .await
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            "'1password-cli' is a Homebrew cask, not a formula; declare it as \"brew-cask:1password-cli\""
        );
        assert_eq!(failed_formula_name(&err), Some("1password-cli"));

        let err = formula_from(&base, &aliases, "missing").await.unwrap_err();
        assert_eq!(
            err.to_string(),
            "failed to fetch Homebrew formula 'missing'"
        );
        assert_eq!(failed_formula_name(&err), Some("missing"));
        Ok(())
    }

    #[tokio::test]
    async fn failed_alias_reports_the_requested_name() -> Result<()> {
        let mut server = mockito::Server::new_async().await;
        let base = server.url();
        server
            .mock("GET", mockito::Matcher::Regex("^/formula/".into()))
            .with_status(404)
            .create_async()
            .await;
        server
            .mock("GET", "/formula.json")
            .with_body(serde_json::json!([{"name": "renamed", "oldnames": ["old"]}]).to_string())
            .create_async()
            .await;
        let aliases = AliasIndex::const_new();

        let err = formula_from(&base, &aliases, "old").await.unwrap_err();
        assert_eq!(failed_formula_name(&err), Some("old"));
        assert!(format!("{err:#}").contains("renamed.json"), "{err:#}");
        Ok(())
    }

    #[tokio::test]
    async fn failed_alias_index_is_fetched_once() -> Result<()> {
        let mut server = mockito::Server::new_async().await;
        let base = server.url();
        server
            .mock("GET", mockito::Matcher::Regex("^/formula/".into()))
            .with_status(404)
            .create_async()
            .await;
        let index = server
            .mock("GET", "/formula.json")
            .with_status(404)
            .expect(1)
            .create_async()
            .await;
        let aliases = AliasIndex::const_new();

        for name in ["first", "second"] {
            let err = formula_from(&base, &aliases, name).await.unwrap_err();
            assert!(
                format!("{err:#}").contains(&format!("{name}.json")),
                "{err:#}"
            );
        }
        index.assert_async().await;
        Ok(())
    }
}
