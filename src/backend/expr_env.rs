//! The expr-lang environment for expressions that read documents mise fetched:
//! `version_expr`, `version_date_expr`, `version_date_next_expr` and
//! `checksum_expr`.
//!
//! The fetched body is bounded before an expression sees it, but a string inside
//! that body can hold more JSON, which the builtin `fromJSON` parses with no
//! limit: `fromJSON(fromJSON(body).payload)` can build far more values than the
//! body was allowed to hold. The `fromJSON` here refuses a string longer than
//! [`MAX_BODY_BYTES`], and stops an evaluation once its calls together could
//! build more than [`MAX_JSON_VALUES`] values, before the builtin parses the
//! string that would go over.

use expr::{Environment, Value};
use mise_util::http::{MAX_BODY_BYTES, MAX_JSON_VALUES, json_value_count};
use std::sync::LazyLock;
use std::sync::atomic::{AtomicUsize, Ordering};

/// A fresh environment for one evaluation: the `fromJSON` budget is spent by
/// every call the evaluation makes, so build one for each expression run.
pub(crate) fn environment() -> Environment<'static> {
    environment_with_limits(MAX_BODY_BYTES, MAX_JSON_VALUES)
}

fn environment_with_limits(max_bytes: usize, max_values: usize) -> Environment<'static> {
    // The builtin `fromJSON`, kept so the bounded one parses exactly as it did.
    static BUILTINS: LazyLock<Environment<'static>> = LazyLock::new(Environment::new);

    let mut env = Environment::new();
    let remaining = AtomicUsize::new(max_values);
    env.add_function("fromJSON", move |c| {
        if let [Value::String(json)] = c.args.as_slice() {
            if json.len() > max_bytes {
                return Err(format!(
                    "fromJSON() was given {} bytes, more than the {max_bytes} it parses",
                    json.len()
                )
                .into());
            }
            if !spend(&remaining, json_value_count(json.as_bytes())) {
                return Err(format!(
                    "fromJSON() would build more than {max_values} values in one expression"
                )
                .into());
            }
        }
        // Argument errors are the builtin's to report.
        BUILTINS.eval_func(c.ctx, "fromJSON", c.args, None)
    });
    env
}

/// Take `n` values from `budget`, or return false when fewer remain. A
/// compare-exchange loop because `fetch_update` is deprecated as `try_update`
/// on newer toolchains and `try_update` is not stable on the MSRV.
fn spend(budget: &AtomicUsize, n: usize) -> bool {
    let mut left = budget.load(Ordering::Relaxed);
    loop {
        let Some(next) = left.checked_sub(n) else {
            return false;
        };
        match budget.compare_exchange_weak(left, next, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => return true,
            Err(actual) => left = actual,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use expr::Context;

    fn eval(env: &Environment, expr: &str, body: &str) -> expr::Result<Value> {
        let mut ctx = Context::default();
        ctx.insert("body".to_string(), Value::String(body.to_string()));
        env.eval(expr, &ctx)
    }

    #[test]
    fn parses_a_normal_document() {
        let env = environment();
        let body = r#"{"releases": [{"version": "1.2.3"}, {"version": "1.2.4"}]}"#;
        let result = eval(&env, "map(fromJSON(body).releases, { #.version })", body).unwrap();
        assert_eq!(result, Value::Array(vec!["1.2.3".into(), "1.2.4".into()]));
    }

    #[test]
    fn parses_json_nested_in_a_string() {
        let env = environment();
        let body = r#"{"payload": "[1, 2, 3]"}"#;
        let result = eval(&env, "fromJSON(fromJSON(body).payload)[2]", body).unwrap();
        assert_eq!(result, Value::Integer(3));
    }

    #[test]
    fn rejects_a_string_over_the_byte_limit() {
        let env = environment_with_limits(16, MAX_JSON_VALUES);
        let err = eval(&env, "fromJSON(body)", r#"{"name": "a long enough value"}"#).unwrap_err();
        assert!(
            err.to_string().contains("more than the 16 it parses"),
            "{err}"
        );
        assert!(eval(&env, "fromJSON(body)", r#"{"a": 1}"#).is_ok());
    }

    #[test]
    fn rejects_a_document_over_the_value_limit() {
        let env = environment_with_limits(MAX_BODY_BYTES, 10);
        let err = eval(&env, "fromJSON(body)", "[0,0,0,0,0,0,0,0,0,0,0]").unwrap_err();
        assert!(err.to_string().contains("more than 10 values"), "{err}");
    }

    #[test]
    fn counts_every_call_in_one_evaluation() {
        // Each parse holds 4 values; the third goes over a budget of 10.
        let expr = "map(fromJSON(body), { fromJSON(#) })";
        let body = r#"["[0,0,0]", "[0,0,0]", "[0,0,0]"]"#;
        let err = eval(&environment_with_limits(MAX_BODY_BYTES, 10), expr, body).unwrap_err();
        assert!(err.to_string().contains("more than 10 values"), "{err}");
        assert!(eval(&environment_with_limits(MAX_BODY_BYTES, 20), expr, body).is_ok());
    }

    #[test]
    fn rejects_a_large_document_nested_in_a_small_body() {
        // The body holds two values, so the check on the body passes it; the
        // array inside its string holds more than MAX_JSON_VALUES.
        let nested = format!("[{}0]", "0,".repeat(MAX_JSON_VALUES));
        let body = serde_json::json!({ "payload": nested }).to_string();
        mise_util::http::ensure_json_values_bounded(body.as_bytes()).unwrap();
        let err = eval(
            &environment(),
            "len(fromJSON(fromJSON(body).payload))",
            &body,
        )
        .unwrap_err();
        assert!(
            err.to_string()
                .contains(&format!("more than {MAX_JSON_VALUES} values")),
            "{err}"
        );
    }

    #[test]
    fn leaves_argument_errors_to_the_builtin() {
        let err = eval(&environment(), "fromJSON(1)", "").unwrap_err();
        assert!(err.to_string().contains("takes a string"), "{err}");
    }
}
