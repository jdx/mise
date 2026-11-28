use super::*;

#[tokio::test]
async fn test_os_arch_for_target() {
    // os()/arch() resolve to the requested target, not the host.
    assert_eq!(
        render_for_target("{{os()}}-{{arch()}}", "windows", "arm64"),
        "windows-arm64"
    );
    assert_eq!(render_for_target("{{os()}}", "macos", "x64"), "macos");
}
#[tokio::test]
async fn test_os_arch_remap_for_target() {
    // Remap arguments keep host semantics but apply to the target value.
    assert_eq!(
        render_for_target(
            r#"{{os(macos="darwin")}}_{{arch(x64="amd64")}}"#,
            "macos",
            "x64"
        ),
        "darwin_amd64"
    );
    // A remap that does not match the target value is ignored.
    assert_eq!(
        render_for_target(r#"{{arch(x64="amd64")}}"#, "linux", "arm64"),
        "arm64"
    );
}
#[tokio::test]
async fn test_os_family_for_target() {
    // os_family() follows the target, not the host.
    assert_eq!(
        render_for_target("{{os_family()}}", "windows", "x64"),
        "windows"
    );
    assert_eq!(render_for_target("{{os_family()}}", "linux", "x64"), "unix");
    assert_eq!(
        render_for_target("{{os_family()}}", "macos", "arm64"),
        "unix"
    );
}
#[tokio::test]
async fn test_preserving_os_arch_round_trips_through_target() {
    // A deferred os(...) remap survives config-load preservation and then
    // re-renders correctly for a target platform.
    let mut ctx = BASE_CONTEXT.clone();
    ctx.insert("cwd", "/");
    let mut deferred = {
        let _lock = crate::testing::lock_ignoring_poison(&crate::testing::SETTINGS_LOCK);
        get_tera_preserving_os_arch(None)
    };
    let preserved = render_str(&mut deferred, r#"{{ os(macos="darwin") }}"#, &ctx).unwrap();
    let preserved = escape_rendered_data(&preserved).unwrap();
    assert_eq!(preserved, r#"{{ os(macos="darwin") }}"#);
    let mut tera = locked_tera_for_target(None, "macos", "arm64");
    assert_eq!(render_str(&mut tera, &preserved, &ctx).unwrap(), "darwin");
}
#[tokio::test]
async fn test_preserving_os_family_round_trips_through_target() {
    // os_family() must be deferred at config-load time and resolve against
    // the lock target, not the host — otherwise a windows target locked from
    // a unix host would get "unix" baked in.
    let mut ctx = BASE_CONTEXT.clone();
    ctx.insert("cwd", "/");
    let mut deferred = {
        let _lock = crate::testing::lock_ignoring_poison(&crate::testing::SETTINGS_LOCK);
        get_tera_preserving_os_arch(None)
    };
    let preserved = render_str(&mut deferred, r#"{{ os_family() }}"#, &ctx).unwrap();
    let preserved = escape_rendered_data(&preserved).unwrap();
    assert_eq!(preserved, r#"{{ os_family() }}"#);
    let mut tera = locked_tera_for_target(None, "windows", "x64");
    assert_eq!(render_str(&mut tera, &preserved, &ctx).unwrap(), "windows");
}
#[tokio::test]
async fn test_libc_and_avx2_follow_the_target_qualifier() {
    let mut ctx = BASE_CONTEXT.clone();
    ctx.insert("cwd", "/");
    let template = r#"{{ avx2(yes="", no="-baseline") }}{{ libc(gnu="", musl="-musl") }}"#;
    for (qualifier, expected) in [
        (None, ""),
        (Some("musl"), "-musl"),
        (Some("baseline"), "-baseline"),
        (Some("musl-baseline"), "-baseline-musl"),
    ] {
        let mut tera = locked_tera_for_target_variant(None, "linux", "x64", qualifier);
        assert_eq!(
            render_str(&mut tera, template, &ctx).unwrap(),
            expected,
            "{qualifier:?}"
        );
    }
}
#[tokio::test]
async fn test_preserving_libc_and_avx2_round_trip_through_target() {
    let mut ctx = BASE_CONTEXT.clone();
    ctx.insert("cwd", "/");
    let mut deferred = {
        let _lock = crate::testing::lock_ignoring_poison(&crate::testing::SETTINGS_LOCK);
        get_tera_preserving_os_arch(None)
    };
    let preserved = render_str(
        &mut deferred,
        r#"{{ avx2(yes="", no="-baseline") }}{{ libc(gnu="", musl="-musl") }}"#,
        &ctx,
    )
    .unwrap();
    let preserved = escape_rendered_data(&preserved).unwrap();
    assert_eq!(
        preserved,
        r#"{{ avx2(no="-baseline", yes="") }}{{ libc(gnu="", musl="-musl") }}"#
    );
    let mut tera = locked_tera_for_target_variant(None, "linux", "x64", Some("musl-baseline"));
    assert_eq!(
        render_str(&mut tera, &preserved, &ctx).unwrap(),
        "-baseline-musl"
    );
}
#[tokio::test]
async fn test_substituted_template_syntax_renders_back_as_data() {
    // A value the first render substitutes is data: rendering the escaped output again
    // gives it back unchanged instead of evaluating it, while the re-emitted call stays live.
    let mut ctx = BASE_CONTEXT.clone();
    ctx.insert("cwd", "/");
    ctx.insert(
        "branch",
        "{{ exec(command='touch RAN') }}{% if true %}x{% endif %}{# c #}",
    );
    let mut deferred = {
        let _lock = crate::testing::lock_ignoring_poison(&crate::testing::SETTINGS_LOCK);
        get_tera_preserving_os_arch(None)
    };
    let preserved = render_str(
        &mut deferred,
        r#"https://e/{{ branch }}/{{ os(macos="darwin") }}"#,
        &ctx,
    )
    .unwrap();
    let preserved = escape_rendered_data(&preserved).unwrap();
    let mut tera = locked_tera_for_target(None, "macos", "arm64");
    assert_eq!(
        render_str(&mut tera, &preserved, &ctx).unwrap(),
        "https://e/{{ exec(command='touch RAN') }}{% if true %}x{% endif %}{# c #}/darwin"
    );
}
#[tokio::test]
async fn test_reemitted_argument_with_quotes_stays_a_literal() {
    // An argument holding `"` or `\` must come back as the same string, not end the
    // literal and become code.
    let value = r#"x"and(exec(command='touch RAN'))and"y\z"#;
    let mut ctx = BASE_CONTEXT.clone();
    ctx.insert("cwd", "/");
    ctx.insert("branch", value);
    let mut deferred = {
        let _lock = crate::testing::lock_ignoring_poison(&crate::testing::SETTINGS_LOCK);
        get_tera_preserving_os_arch(None)
    };
    let preserved = render_str(
        &mut deferred,
        r#"{{ os(linux=branch, macos="main") }}"#,
        &ctx,
    )
    .unwrap();
    let preserved = escape_rendered_data(&preserved).unwrap();
    let mut tera = locked_tera_for_target(None, "linux", "x64");
    assert_eq!(render_str(&mut tera, &preserved, &ctx).unwrap(), value);
}
#[test]
fn test_reemitted_argument_with_quotes_stays_a_literal_in_tera_v1() {
    let value = r#"x"~exec(command='touch RAN')~"y\z"#;
    let mut first = tera1::Tera::default();
    first.register_function("os", reemit_template_fn_v1("os"));
    let mut ctx = Context::new();
    ctx.insert("branch", value);
    let mut first = TeraEngine::V1(Box::new(first));
    let preserved = first.render_str("{{ os(linux=branch) }}", &ctx).unwrap();
    let preserved = escape_rendered_data(&preserved).unwrap();
    let mut second = tera1::Tera::default();
    second.register_function("os", |args: &HashMap<String, JsonValue>| {
        Ok(args["linux"].clone())
    });
    let mut second = TeraEngine::V1(Box::new(second));
    assert_eq!(second.render_str(&preserved, &ctx).unwrap(), value);
}
#[test]
fn test_escaped_braces_render_back_in_both_engines() {
    // Braces next to an opener, and data that ends in `{` before deferred code, render
    // back to the same text.
    let mut ctx = Context::new();
    ctx.insert("version", "1.2.3");
    for data in [
        "{{{x",
        "{{{",
        "{{{{",
        "{{{{{",
        "{{}}",
        "{%}",
        "{#}",
        "{{{%",
        "{",
        "x{",
        "}}}",
        "{{ \"{{\" }}",
    ] {
        for mut tera in [
            TeraEngine::V1(Box::default()),
            TeraEngine::V2(Box::default()),
        ] {
            let escaped = escape_template_syntax(data);
            assert_eq!(tera.render_str(&escaped, &ctx).unwrap(), data, "{escaped}");
            let with_code =
                escape_rendered_data(&format!("{data}{}{data}", mark_deferred("{{ version }}")))
                    .unwrap();
            assert_eq!(
                tera.render_str(&with_code, &ctx).unwrap(),
                format!("{data}1.2.3{data}"),
                "{with_code}"
            );
        }
    }
}
#[tokio::test]
async fn test_cut_deferred_marker_is_an_error() {
    // A filter that strips the end of a re-emitted `os()` call leaves its opening marker alone.
    // The substituted text after it must not be taken for deferred code: the render fails.
    let mut ctx = BASE_CONTEXT.clone();
    ctx.insert("cwd", "/");
    ctx.insert("branch", "{{ exec(command='touch RAN') }}");
    let mut deferred = {
        let _lock = crate::testing::lock_ignoring_poison(&crate::testing::SETTINGS_LOCK);
        get_tera_preserving_os_arch(None)
    };
    for template in [
        "{{ os() | trim_end(pat=\"\u{1f}\") }}{{ branch }}",
        "{{ os() | trim_start(pat=\"\u{1f}\") }}{{ branch }}",
        "{{ os() | replace(from=\"(\", to=\"\") }}{{ branch }}",
        "{{ os() | replace(from=\")\", to=\"\") }}{{ branch }}",
    ] {
        let rendered = render_str(&mut deferred, template, &ctx).unwrap();
        let err = escape_rendered_data(&rendered).unwrap_err().to_string();
        assert!(err.contains("a filter cut into code"), "{template}: {err}");
    }
    // A filter that changes the code of a span is an error too; one that leaves it alone is not.
    let rendered = render_str(
        &mut deferred,
        "{% filter upper %}{{ os() }}{% endfilter %}",
        &ctx,
    )
    .unwrap();
    let err = escape_rendered_data(&rendered).unwrap_err().to_string();
    assert!(err.contains("a filter or section changed code"), "{err}");
    assert!(!err.contains("OS()"), "{err}");
    // The error does not quote the changed call, whose arguments can hold a secret from `env`
    // that the filter changed past its redaction.
    ctx.insert("token", "hunter2-secret");
    let rendered = render_str(
        &mut deferred,
        "{% filter upper %}{{ os(windows=token) }}{% endfilter %}",
        &ctx,
    )
    .unwrap();
    assert!(rendered.contains("HUNTER2-SECRET"), "{rendered}");
    let err = format!("{:?}", escape_rendered_data(&rendered).unwrap_err());
    assert!(err.contains("a filter or section changed code"), "{err}");
    assert!(!err.to_lowercase().contains("hunter2"), "{err}");
    let rendered = render_str(
        &mut deferred,
        "{% filter lower %}{{ os() }}{% endfilter %}",
        &ctx,
    )
    .unwrap();
    assert_eq!(escape_rendered_data(&rendered).unwrap(), "{{ os() }}");
    // Whole spans keep working, next to each other and next to data.
    let rendered = render_str(
        &mut deferred,
        "{{ os() }}{{ branch }}{{ arch() }}{{ os() }}",
        &ctx,
    )
    .unwrap();
    assert_eq!(
        escape_rendered_data(&rendered).unwrap(),
        r#"{{ os() }}{{ "{{" }} exec(command='touch RAN') }}{{ arch() }}{{ os() }}"#
    );
}
#[test]
fn test_escape_template_syntax_leaves_plain_text_alone() {
    assert_eq!(escape_template_syntax("a}}b%}c"), "a}}b%}c");
    assert_eq!(escape_template_syntax("{{x"), r#"{{ "{{" }}x"#);
}

/// A tag ends at the first closer outside its string literals. Tera 2 strings take `\` escapes,
/// so an escaped quote leaves the string open there; Tera 1 strings do not.
#[test]
fn test_template_tag_end_skips_string_literals() {
    let cases = |v1: bool| {
        let tags = [
            ("{{ x }}rest", "}}", Some(7)),
            ("{{ 'a}}' }}rest", "}}", Some(11)),
            ("{% set a = \"%}\" %}rest", "%}", Some(18)),
            ("{{ `}}` }}rest", "}}", Some(10)),
            ("{# '#} rest", "#}", Some(6)),
            ("{{ 'a }}", "}}", None),
            ("{{ x", "}}", None),
        ];
        for (tag, close, end) in tags {
            assert_eq!(template_tag_end(tag, close), end, "{tag} v1={v1}");
        }
        let escaped = r#"{{ "a\" }}" }}rest"#;
        let end = if v1 { 10 } else { 14 };
        assert_eq!(template_tag_end(escaped, "}}"), Some(end), "v1={v1}");
        assert_eq!(template_tag_end(r#"{{ "a\\" }}rest"#, "}}"), Some(11));
    };
    {
        let _lock = crate::testing::lock_ignoring_poison(&crate::testing::SETTINGS_LOCK);
        cases(false);
    }
    let _guard = SettingsGuard::tera_v1();
    cases(true);
}

/// The engine passed in decides how strings are read, whatever `tera_v1` says.
#[test]
fn test_template_tag_end_for_reads_strings_as_the_given_engine() {
    let _guard = SettingsGuard::tera_v1();
    let escaped = r#"{{ "a\" }}" }}rest"#;
    assert_eq!(template_tag_end_for(escaped, "}}", false), Some(14));
    assert_eq!(template_tag_end_for(escaped, "}}", true), Some(10));
}

/// A raw block runs to its first `{% endraw %}`, any `-` markers included, or to the end.
#[test]
fn test_raw_block_end() {
    assert_eq!(raw_block_end("{% raw %}{{x{% endraw %}rest"), Some(24));
    assert_eq!(raw_block_end("{%- raw -%}{{x{%- endraw -%}rest"), Some(28));
    assert_eq!(raw_block_end("{%raw%}{{x"), Some(10));
    assert_eq!(raw_block_end("{% if raw %}"), None);
    assert_eq!(raw_block_end("{{ raw }}"), None);
}

#[test]
fn test_raw_block_body() {
    let block = |body, end| Some(RawBlock { body, end });
    assert_eq!(raw_block("{% raw %}{{x{% endraw %}rest"), block(9..12, 24));
    assert_eq!(
        raw_block("{%- raw -%}{{x{%- endraw -%}rest"),
        block(11..14, 28)
    );
    // an unclosed block runs to the end of the text
    assert_eq!(raw_block("{%raw%}{{x"), block(7..10, 10));
    assert_eq!(raw_block("{% raw %}{% endraw %}"), block(9..9, 21));
    assert_eq!(raw_block("a{% raw %}"), None);
}
