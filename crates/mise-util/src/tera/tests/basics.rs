use super::*;

#[test]
fn test_get_env_function() {
    let mut env = EnvMap::new();
    env.insert("MISE_TEST_GET_ENV".into(), "from-env".into());
    env.insert("MISE_TEST_EMPTY_ENV".into(), "".into());

    let mut tera = Tera::default();
    tera.register_function("get_env", tera_get_env(env));
    let ctx = Context::new();

    assert_eq!(
        tera.render_str("{{ get_env(name='MISE_TEST_GET_ENV') }}", &ctx, false)
            .unwrap(),
        "from-env"
    );
    assert_eq!(
        tera.render_str(
            "{{ get_env(name='MISE_TEST_MISSING_ENV', default='fallback') }}",
            &ctx,
            false
        )
        .unwrap(),
        "fallback"
    );
    assert_eq!(
        tera.render_str(
            "{{ get_env(name='MISE_TEST_EMPTY_ENV', default='fallback') }}",
            &ctx,
            false
        )
        .unwrap(),
        ""
    );
    assert!(
        tera.render_str("{{ get_env(name='MISE_TEST_MISSING_ENV') }}", &ctx, false)
            .is_err()
    );
}

#[test]
fn test_posix_shell_quote_round_trip() {
    for value in [
        "",
        "plain",
        "with spaces",
        "quoted'str",
        "$HOME",
        r"a\\backslash",
        "multiple\nlines",
    ] {
        let quoted = posix_shell_quote(value);
        assert_eq!(shell_words::split(&quoted).unwrap(), [value]);
    }

    assert_eq!(posix_shell_quote("plain"), "'plain'");
    assert_eq!(posix_shell_quote("quoted'str"), "'quoted'\\''str'");
}

#[test]
fn test_contains_template_syntax() {
    assert!(contains_template_syntax("{{ foo }}"));
    assert!(contains_template_syntax("{{- foo -}}"));
    assert!(contains_template_syntax("{% if foo %}bar{% endif %}"));
    assert!(contains_template_syntax("{%- if foo -%}bar{%- endif -%}"));
    assert!(contains_template_syntax("{# comment #}"));
    assert!(contains_template_syntax("{#- comment -#}"));
    assert!(!contains_template_syntax("plain text"));
}
