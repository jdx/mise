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
    let preserved = escape_rendered_data(&preserved);
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
    let preserved = escape_rendered_data(&preserved);
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
    let preserved = escape_rendered_data(&preserved);
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
    let preserved = escape_rendered_data(&preserved);
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
    let preserved = escape_rendered_data(&preserved);
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
    let preserved = escape_rendered_data(&preserved);
    let mut second = tera1::Tera::default();
    second.register_function("os", |args: &HashMap<String, JsonValue>| {
        Ok(args["linux"].clone())
    });
    let mut second = TeraEngine::V1(Box::new(second));
    assert_eq!(second.render_str(&preserved, &ctx).unwrap(), value);
}
#[test]
fn test_escape_template_syntax_leaves_plain_text_alone() {
    assert_eq!(escape_template_syntax("a}}b%}c"), "a}}b%}c");
    assert_eq!(escape_template_syntax("{{x"), r#"{{ "{{" }}x"#);
}
