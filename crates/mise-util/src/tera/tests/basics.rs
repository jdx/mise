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

#[test]
fn test_tera1_get_env_uses_supplied_env_only() {
    // A variable present in the live process env but absent from the supplied
    // (filtered) map must not be readable, as with inherited secrets.
    let mut env = EnvMap::new();
    env.insert("MISE_TEST_V1_VISIBLE".into(), "visible".into());
    let mut tera = tera1::Tera::default();
    tera.register_function("get_env", tera1_get_env(env));
    let ctx = tera1::Context::new();
    let path = std::env::var("PATH").is_ok();
    assert!(path, "PATH must be set for this test");

    assert_eq!(
        tera.render_str("{{ get_env(name='MISE_TEST_V1_VISIBLE') }}", &ctx)
            .unwrap(),
        "visible"
    );
    assert_eq!(
        tera.render_str("{{ get_env(name='PATH', default='hidden') }}", &ctx)
            .unwrap(),
        "hidden"
    );
    assert!(tera.render_str("{{ get_env(name='PATH') }}", &ctx).is_err());
}

#[test]
fn test_undefined_env_field_does_not_list_env_names() {
    let mut env = EnvMap::new();
    env.insert("MISE_TEST_SECRET_TOKEN".into(), "x".into());
    env.insert("MISE_TEST_OTHER".into(), "y".into());
    let mut ctx = Context::new();
    ctx.insert("env", &env);
    ctx.insert("vars", &EnvMap::from([("a".to_string(), "1".to_string())]));

    let mut tera = Tera::default();
    let err = render_str_v2(&mut tera, "{{ env.NOPE }}", &ctx).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("Field `NOPE` is not defined in `env`"),
        "{msg}"
    );
    assert!(
        msg.contains("get_env(name=\"NOPE\", default=\"\")"),
        "{msg}"
    );
    assert!(!msg.contains("MISE_TEST_SECRET_TOKEN"), "{msg}");
    assert!(!msg.contains("MISE_TEST_OTHER"), "{msg}");

    // other objects keep tera's own list of fields
    let err = render_str_v2(&mut tera, "{{ vars.nope }}", &ctx).unwrap_err();
    assert!(err.to_string().contains("Available fields: a"), "{err}");

    // a defined variable still renders
    assert_eq!(
        render_str_v2(&mut tera, "{{ env.MISE_TEST_OTHER }}", &ctx).unwrap(),
        "y"
    );
}
