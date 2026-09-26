//! Homebrew metadata integration with mise's tap Ruby fallback.

use crate::http::HTTP_FETCH;
use crate::result::Result;
use eyre::{WrapErr, bail};

pub use mise_brew_metadata::*;

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
