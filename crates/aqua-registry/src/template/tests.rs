use super::*;

fn hashmap(data: Vec<(&str, &str)>) -> HashMap<String, String> {
    data.iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

#[test]
fn test_render() {
    let tmpl = "Hello, {{.OS}}!";
    let ctx = hashmap(vec![("OS", "world")]);
    assert_eq!(render(tmpl, &ctx).unwrap(), "Hello, world!");
}

#[test]
fn test_render_semver_maven() {
    let tmpl = "https://archive.apache.org/dist/maven/maven-{{(semver .SemVer).Major}}/{{.SemVer}}/binaries/apache-maven-{{.SemVer}}-bin.tar.gz";
    let ctx = hashmap(vec![("SemVer", "3.9.11")]);
    assert_eq!(
        render(tmpl, &ctx).unwrap(),
        "https://archive.apache.org/dist/maven/maven-3/3.9.11/binaries/apache-maven-3.9.11-bin.tar.gz"
    );
}

#[test]
fn test_render_nested_semver_in_function() {
    // The semver function handles 'v' prefix internally, so (semver .Version).Major
    // correctly extracts "3" from "v3.9.11". Then trimV is called on "3" (no-op).
    let tmpl = "{{trimV (semver .Version).Major}}";
    let ctx = hashmap(vec![("Version", "v3.9.11")]);
    assert_eq!(render(tmpl, &ctx).unwrap(), "3");
}

#[test]
fn test_render_semver_handles_v_prefix() {
    // semver function automatically strips 'v' prefix - no need for trimV
    let tmpl = "{{semver .Version}}";
    let ctx = hashmap(vec![("Version", "v3.9.11")]);
    assert_eq!(render(tmpl, &ctx).unwrap(), "3.9.11");
}

#[test]
fn test_versioning_nth() {
    // Test the versions crate directly
    let v = Versioning::new("3.6.0").unwrap();
    assert_eq!(v.nth(0).unwrap_or(0), 3);
    assert_eq!(v.nth(1).unwrap_or(0), 6);
    assert_eq!(v.nth(2).unwrap_or(0), 0);
}

#[test]
fn test_two_semver_calls() {
    // Test calling semver twice in same template
    let tmpl = "{{(semver .Version).Major}}.{{(semver .Version).Minor}}";
    let ctx = hashmap(vec![("Version", "4.6.0")]);
    let result = render(tmpl, &ctx).unwrap();
    assert_eq!(result, "4.6", "Expected '4.6' but got '{}'", result);
}

#[test]
fn test_parse_second_semver() {
    // Debug: parse just the second semver call
    let tokens = lex("(semver .Version).Minor").unwrap();
    let ast = parse_tokens(&tokens).unwrap();

    // Should be: PropertyAccess(FuncCall("semver", [Var("Version")]), "Minor")
    if let Expr::PropertyAccess(inner, prop) = ast {
        assert_eq!(prop, "Minor");
        if let Expr::FuncCall(func, args) = *inner {
            assert_eq!(func, "semver");
            assert_eq!(args.len(), 1);
        } else {
            panic!("Inner should be FuncCall, got: {:?}", inner);
        }
    } else {
        panic!("Should be PropertyAccess, got: {:?}", ast);
    }
}

#[test]
fn test_semver_property_major() {
    let tmpl = "{{(semver .Version).Major}}";
    let ctx = hashmap(vec![("Version", "3.6.0")]);
    let result = render(tmpl, &ctx).unwrap();
    assert_eq!(result, "3");
}

#[test]
fn test_semver_property_minor() {
    let tmpl = "{{(semver .Version).Minor}}";
    let ctx = hashmap(vec![("Version", "3.6.0")]);
    let result = render(tmpl, &ctx).unwrap();
    assert_eq!(result, "6");
}

#[test]
fn test_render_blender_url() {
    // Exact pattern from blender registry with version 3.6.0 (failing case)
    let tmpl = "https://download.blender.org/release/Blender{{(semver .Version).Major}}.{{(semver .Version).Minor}}/blender-{{trimV .Version}}-linux-x64.tar.xz";
    let ctx = hashmap(vec![("Version", "3.6.0")]);
    let result = render(tmpl, &ctx).unwrap();
    assert_eq!(
        result,
        "https://download.blender.org/release/Blender3.6/blender-3.6.0-linux-x64.tar.xz"
    );
}

#[test]
fn test_render_blender_url_4_3() {
    // Test with 4.3.2
    let tmpl = "https://download.blender.org/release/Blender{{(semver .Version).Major}}.{{(semver .Version).Minor}}/blender-{{trimV .Version}}-linux-x64.tar.xz";
    let ctx = hashmap(vec![("Version", "4.3.2")]);
    let result = render(tmpl, &ctx).unwrap();
    assert_eq!(
        result,
        "https://download.blender.org/release/Blender4.3/blender-4.3.2-linux-x64.tar.xz"
    );
}

#[test]
fn test_render_semver_as_function_arg() {
    let tmpl = "{{title (semver .Version).Major}}";
    let ctx = hashmap(vec![("Version", "3.9.11")]);
    assert_eq!(render(tmpl, &ctx).unwrap(), "3");
}

#[test]
fn test_lex_semver_with_property() {
    let tokens = lex("(semver .Version).Major").unwrap();
    // Should be: LParen, Func(semver), Whitespace, Key(Version), RParen, Dot, Ident(Major)
    assert!(
        tokens.len() >= 6,
        "Expected at least 6 tokens, got {}: {:?}",
        tokens.len(),
        tokens
    );
}

#[test]
fn test_render_just_semver_paren() {
    let tmpl = "{{(semver .Version)}}";
    let ctx = hashmap(vec![("Version", "1.2.3")]);
    assert_eq!(render(tmpl, &ctx).unwrap(), "1.2.3");
}

macro_rules! parse_tests {
    ($($name:ident: $value:expr,)*) => {
        $(
            #[test]
            fn $name() {
                let (input, expected, ctx_data): (&str, &str, Vec<(&str, &str)>) = $value;
                let ctx = hashmap(ctx_data);
                let tmpl = format!("{{{{{}}}}}", input);
                assert_eq!(expected, render(&tmpl, &ctx).unwrap());
            }
        )*
    }}

parse_tests!(
    test_parse_key: (".OS", "world", vec![("OS", "world")]),
    test_parse_string: ("\"world\"", "world", vec![]),
    test_parse_title: (r#"title "world""#, "World", vec![]),
    test_parse_trimv: (r#"trimV "v1.0.0""#, "1.0.0", vec![]),
    test_parse_trim_prefix: (r#"trimPrefix "v" "v1.0.0""#, "1.0.0", vec![]),
    test_parse_trim_prefix2: (r#"trimPrefix "v" "1.0.0""#, "1.0.0", vec![]),
    test_parse_trim_suffix: (r#"trimSuffix "-v1.0.0" "foo-v1.0.0""#, "foo", vec![]),
    test_parse_pipe: (r#"trimPrefix "foo-" "foo-v1.0.0" | trimV"#, "1.0.0", vec![]),
    test_parse_multiple_pipes: (
        r#"trimPrefix "foo-" "foo-v1.0.0-beta" | trimSuffix "-beta" | trimV"#,
        "1.0.0",
        vec![],
    ),
    test_parse_replace: (r#"replace "foo" "bar" "foo-bar""#, "bar-bar", vec![]),
    test_parse_semver_major: (r#"(semver .Version).Major"#, "3", vec![("Version", "3.9.11")]),
    test_parse_semver_minor: (r#"(semver .Version).Minor"#, "9", vec![("Version", "3.9.11")]),
    test_parse_semver_patch: (r#"(semver .Version).Patch"#, "11", vec![("Version", "3.9.11")]),
    test_parse_semver_major_v_prefix: (r#"(semver .Version).Major"#, "1", vec![("Version", "v1.2.3")]),
    test_parse_semver_no_property: (r#"(semver .Version)"#, "1.2.3", vec![("Version", "1.2.3")]),
    test_parse_nested_semver_in_trimv: (r#"trimV (semver .Version).Major"#, "3", vec![("Version", "v3.9.11")]),
    test_parse_nested_semver_in_title: (r#"title (semver .Version).Minor"#, "9", vec![("Version", "3.9.11")]),
    test_parse_semver_standalone: (r#"semver .Version"#, "1.2.3", vec![("Version", "v1.2.3")]),
    test_parse_semver_standalone_no_v: (r#"semver .Version"#, "1.2.3", vec![("Version", "1.2.3")]),
);

#[test]
fn test_parse_err() {
    let ctx = HashMap::new();
    let result = render("{{foo}}", &ctx);
    assert!(result.is_err());
}

#[test]
fn test_lex() {
    assert_eq!(
        lex(r#"trimPrefix "foo-" "foo-v1.0.0" | trimV"#).unwrap(),
        vec![
            Token::Func("trimPrefix"),
            Token::Whitespace(" "),
            Token::String("foo-"),
            Token::Whitespace(" "),
            Token::String("foo-v1.0.0"),
            Token::Whitespace(" "),
            Token::Pipe,
            Token::Whitespace(" "),
            Token::Func("trimV"),
        ]
    );
}

#[test]
fn test_gradle_src_template() {
    // Test the gradle src template pattern: {{.AssetWithoutExt | trimSuffix "-bin"}}/bin/gradle
    // This tests that pipe expressions work correctly when preceded by whitespace
    let tmpl = r#"{{.AssetWithoutExt | trimSuffix "-bin"}}/bin/gradle"#;
    let ctx = hashmap(vec![("AssetWithoutExt", "gradle-8.14.3-bin")]);
    assert_eq!(render(tmpl, &ctx).unwrap(), "gradle-8.14.3/bin/gradle");
}

#[test]
fn test_render_vars_property_access() {
    let tmpl = "{{.Vars.channel}}";
    let ctx = hashmap(vec![("Vars.channel", "stable")]);
    assert_eq!(render(tmpl, &ctx).unwrap(), "stable");
}

#[test]
fn test_render_vars_property_in_function_arg() {
    let tmpl = "{{title .Vars.channel}}";
    let ctx = hashmap(vec![("Vars.channel", "stable")]);
    assert_eq!(render(tmpl, &ctx).unwrap(), "Stable");
}
