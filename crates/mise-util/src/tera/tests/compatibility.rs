use super::*;

#[test]
fn test_tera_v1_setting_selects_v1_engine() {
    let _guard = SettingsGuard::tera_v1();
    assert!(matches!(get_tera(None), TeraEngine::V1(_)));
}

#[test]
fn test_miserc_tera_ignores_tera_v1_setting() {
    let _guard = SettingsGuard::tera_v1();
    assert!(matches!(get_miserc_tera(), TeraEngine::V2(_)));
}

#[test]
fn test_tera_v1_engine_renders_v1_macro() {
    let mut tera_ctx = BASE_CONTEXT.clone();
    tera_ctx.insert("name", "mise");
    let mut tera = TeraEngine::V1(Box::new(TERA1.clone()));
    assert_eq!(
        render_str(
            &mut tera,
            "{% macro greet(name) %}hi {{ name }}{% endmacro %}{{ self::greet(name=name) }}",
            &tera_ctx
        )
        .unwrap(),
        "hi mise"
    );

    let mut tera = TeraEngine::V2(Box::new(TERA.clone()));
    assert!(
        render_str(
            &mut tera,
            "{% macro greet(name) %}hi {{ name }}{% endmacro %}{{ self::greet(name=name) }}",
            &tera_ctx
        )
        .is_err()
    );
}

#[test]
fn test_tera_v1_context_preserves_common_nested_roots() {
    let mut tera_ctx = Context::new();
    tera_ctx.insert("env", &json!({ "FOO": "bar" }));
    tera_ctx.insert("vars", &json!({ "nested": { "name": "baz" } }));
    tera_ctx.insert("tools", &json!([{ "name": "node" }]));
    let mut tera = TeraEngine::V1(Box::new(TERA1.clone()));
    assert_eq!(
        render_str(
            &mut tera,
            "{% for tool in tools %}{{ env.FOO }} {{ vars.nested.name }} {{ tool.name }}{% endfor %}",
            &tera_ctx
        )
        .unwrap(),
        "bar baz node"
    );
}

#[test]
fn test_tera_v1_compat_filters() {
    assert_eq!(
        render("v{{ 'v0.80.0' | trim_start_matches(pat='v') }}"),
        "v0.80.0"
    );
    assert_eq!(
        render("{{ 'v0.80.0' | trim_start_matches(pat='v') }}"),
        "0.80.0"
    );
    assert_eq!(
        render("{{ '0.80.0.tar.gz' | trim_end_matches(pat='.tar.gz') }}"),
        "0.80.0"
    );
    assert_eq!(
        render("{{ '1.12.6' | split(pat='.') | slice(start=0, end=2) | join(sep='.') }}"),
        "1.12"
    );
    assert_eq!(
        render("{{ '1.12.6' | split(pat='.') | slice(start=-2) | join(sep='.') }}"),
        "12.6"
    );
    assert_eq!(
        render("{{ ('1.12.6' | split(pat='.'))[0:2] | join(sep='.') }}"),
        "1.12"
    );
    assert_eq!(
        render("{{ ('a/b/c' | split(pat='/'))[0] ~ '/mod.rs' }}"),
        "a/mod.rs"
    );
    assert_eq!(render("{{ ['a'] | concat(with=['b']) | join }}"), "ab");
    assert_eq!(
        render(
            "{{ [{'name': 'alice', 'active': true}, {'name': 'bob', 'active': false}] | map(attribute='name') | join(sep=',') }}"
        ),
        "alice,bob"
    );
    assert_eq!(
        render(
            "{{ [{'name': 'alice', 'active': true}, {'name': 'bob', 'active': false}] | filter(attribute='active', value=true) | length }}"
        ),
        "1"
    );
    assert_eq!(
        render(
            "{{ [{'name': 'alice', 'active': true}, {'name': 'bob', 'active': false}] | filter(attribute='active') | map(attribute='name') | join(sep=',') }}"
        ),
        "alice"
    );
    assert_eq!(render("{{ '<b>x</b>' | striptags }}"), "x");
    assert_eq!(
        render("{{ '<b> x </b> <i>y</i>' | spaceless }}"),
        "<b> x </b><i>y</i>"
    );
    assert_eq!(render(r#"{{ "a'b" | addslashes }}"#), r#"a\'b"#);
    assert_eq!(render("{{ 'Hello, world!' | slugify }}"), "hello-world");
    assert_eq!(render("{{ 'a b' | urlencode }}"), "a%20b");
    assert_eq!(render("{{ 'a/b c' | urlencode }}"), "a/b%20c");
    assert_eq!(render("{{ 'a/b c' | urlencode_strict }}"), "a%2Fb%20c");
    assert_eq!(render("{{ '<br>' | escape }}"), "&lt;br&gt;");
    assert_eq!(render("{{ 'a\nb' | linebreaksbr }}"), "a<br>b");
    assert_eq!(render("{{ {'ok': true} | json_encode }}"), r#"{"ok":true}"#);
    assert_eq!(render("{{ 0 | date(format='%Y-%m-%d') }}"), "1970-01-01");
    assert_eq!(render("{{ 'abc' | truncate }}"), "abc");
    assert_eq!(render("{{ 'a\nb' | indent(prefix='>') }}"), "a\n>b");
    assert_eq!(render("{{ 'nope' | int(default=7) }}"), "7");
    assert_eq!(render("{{ 'nope' | float(default=1.5) }}"), "1.5");
    assert_eq!(render("{{ [] | first }}"), "");
    assert_eq!(render("{{ [] | last }}"), "");
    assert_eq!(render("{{ 'abc' | first }}"), "a");
    assert_eq!(render("{{ 'abc' | last }}"), "c");
    assert_eq!(render("{{ [] | nth(n=0) }}"), "");
    assert_eq!(render("{{ ['a', 'A'] | unique | join }}"), "a");
    assert_eq!(
        render("{{ ['a', 'A'] | unique(case_sensitive=false) | join }}"),
        "a"
    );
    assert_eq!(render("{{ {'ok': true} is object }}"), "true");
    assert_eq!(render("{{ 6 is divisibleby(divisor=3) }}"), "true");
}

/// v1's builtin `trim_start`/`trim_end` take no `pat` and drop it silently,
/// which turned the spelling mise recommends into a no-op under `tera_v1`.
/// Failing here also means the v1 builtins can no longer be overridden.
#[test]
fn test_tera_v1_trim_honors_pat() {
    assert_eq!(render_v1("{{ 'v1.2.3' | trim_start(pat='v') }}"), "1.2.3");
    assert_eq!(
        render_v1("{{ '3.27.2-stable' | trim_end(pat='-stable') }}"),
        "3.27.2"
    );
    // A pattern that is not present leaves the value alone, as in v2.
    assert_eq!(
        render_v1("{{ '1.2.3' | trim_end(pat='-stable') }}"),
        "1.2.3"
    );
}

/// Without `pat` these must keep trimming whitespace, as v1 always has.
#[test]
fn test_tera_v1_trim_without_pat_still_trims_whitespace() {
    assert_eq!(render_v1("[{{ '  x  ' | trim_start }}]"), "[x  ]");
    assert_eq!(render_v1("[{{ '  x  ' | trim_end }}]"), "[  x]");
}

/// The engines must agree on the shape registry entries use (azd,
/// clickhouse, magika), so a template does not silently render differently
/// under `tera_v1`.
///
/// `registry/flutter.toml` leans on this: `version_expr` strips the channel
/// suffix and the URLs append it back, so appending has to be idempotent for
/// a version that already carries it and for one that does not (#4170).
#[test]
fn test_tera_engines_agree_on_the_registry_trim_pattern() {
    let suffixed = "{{ '3.27.2-stable' | trim_end(pat='-stable') }}-stable";
    assert_eq!(render_v2(suffixed), "3.27.2-stable");
    assert_eq!(render_v1(suffixed), "3.27.2-stable");

    let bare = "{{ '3.27.2' | trim_end(pat='-stable') }}-stable";
    assert_eq!(render_v2(bare), "3.27.2-stable");
    assert_eq!(render_v1(bare), "3.27.2-stable");
}
