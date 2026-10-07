use super::*;

#[test]
fn test_tera_contrib_helpers() {
    assert_eq!(render("{{ 'hello' | b64_encode | b64_decode }}"), "hello");
    assert_eq!(
        render("{{ '2026-07-13' | date(format='%Y/%m/%d') }}"),
        "2026/07/13"
    );
    assert_eq!(render("{{ now() | date(format='%Y') | length }}"), "4");
    assert_eq!(
        render("{{ '2026-01-01' is before(other='2026-02-01') }}"),
        "true"
    );
    assert_eq!(
        render("{{ '2026-02-01' is after(other='2026-01-01') }}"),
        "true"
    );
    assert_eq!(render("{{ 1024 | filesize_format }}"), "1 KiB");
    assert_eq!(render("{{ 1024 | filesizeformat }}"), "1 KiB");
    assert_eq!(render("{{ 42 | format(spec='05') }}"), "00042");
    assert_eq!(render("{{ {'ok': true} | json_encode }}"), r#"{"ok":true}"#);
    let random = render("{{ get_random(start=10, end=20, seed='mise') }}")
        .parse::<i64>()
        .unwrap();
    assert!((10..20).contains(&random));
    assert_eq!(
        render("{{ [1, 2, 3] | shuffle(seed='mise') | length }}"),
        "3"
    );
    assert_eq!(
        render("{{ 'abc123' | regex_replace(pattern='[0-9]+', rep='') }}"),
        "abc"
    );
    assert_eq!(render("{{ 'abc123' is matching(pat='[0-9]+$') }}"), "true");
    assert_eq!(render("{{ '<b>x</b>' | striptags }}"), "x");
    assert_eq!(render("{{ '<p> </p>' | spaceless }}"), "<p></p>");
    assert_eq!(render("{{ 'Hello World' | slug }}"), "hello-world");
    assert_eq!(render("{{ 'Hello World' | slugify }}"), "hello-world");
    assert_eq!(render("{{ 'a/b c' | urlencode }}"), "a/b%20c");
    assert_eq!(render("{{ 'a/b c' | urlencode_strict }}"), "a%2Fb%20c");
}

#[tokio::test]
async fn test_config_root() {
    assert_eq!(render("{{config_root}}"), "/");
}
#[tokio::test]
async fn test_mise_env() {
    assert_eq!(render("{% if mise_env %}{{mise_env}}{% endif %}"), "");
}
#[tokio::test]
async fn test_cwd() {
    assert_eq!(render("{{cwd}}"), "/");
}
#[tokio::test]
async fn test_mise_bin() {
    assert_eq!(
        render("{{mise_bin}}"),
        env::current_exe()
            .unwrap()
            .into_os_string()
            .into_string()
            .unwrap()
    );
}
#[tokio::test]
async fn test_mise_pid() {
    let s = render("{{mise_pid}}");
    let pid = s.trim().parse::<u32>().unwrap();
    assert!(pid > 0);
}
#[tokio::test]
async fn test_xdg_cache_home() {
    let s = render("{{xdg_cache_home}}");
    assert_str_eq!(s, env::XDG_CACHE_HOME.to_string_lossy());
}
#[tokio::test]
#[cfg(unix)]
async fn test_xdg_config_home() {
    let s = render("{{xdg_config_home}}");
    assert!(s.ends_with("/.config")); // test dir is not deterministic
}
#[tokio::test]
#[cfg(unix)]
async fn test_xdg_data_home() {
    let s = render("{{xdg_data_home}}");
    assert!(s.ends_with("/.local/share")); // test dir is not deterministic
}
#[tokio::test]
#[cfg(unix)]
async fn test_xdg_state_home() {
    let s = render("{{xdg_state_home}}");
    assert!(s.ends_with("/.local/state")); // test dir is not deterministic
}
#[tokio::test]
async fn test_arch() {
    if cfg!(target_arch = "x86_64") {
        assert_eq!(render("{{arch()}}"), "x64");
    } else if cfg!(target_arch = "aarch64") {
        assert_eq!(render("{{arch()}}"), "arm64");
    } else {
        assert_eq!(render("{{arch()}}"), env::consts::ARCH);
    }
}
#[tokio::test]
async fn test_num_cpus() {
    let s = render("{{ num_cpus() }}");
    let num = s.parse::<u32>().unwrap();
    assert!(num > 0);
}
#[tokio::test]
async fn test_os() {
    if cfg!(target_os = "linux") {
        assert_eq!(render("{{os()}}"), "linux");
    } else if cfg!(target_os = "macos") {
        assert_eq!(render("{{os()}}"), "macos");
    } else if cfg!(target_os = "windows") {
        assert_eq!(render("{{os()}}"), "windows");
    }
}
#[tokio::test]
async fn test_os_family() {
    if cfg!(target_family = "unix") {
        assert_eq!(render("{{os_family()}}"), "unix");
    } else if cfg!(target_os = "windows") {
        assert_eq!(render("{{os_family()}}"), "windows");
    }
}
#[tokio::test]
async fn test_choice() {
    let result = render("{{choice(n=8, alphabet=\"abcdefgh\")}}");
    assert_eq!(result.trim().len(), 8);
}
#[tokio::test]
async fn test_haiku() {
    // Default: 2 words + number
    let result = render("{{haiku()}}");
    let parts: Vec<&str> = result.split('-').collect();
    assert_eq!(parts.len(), 3);
    assert!(!parts[0].is_empty());
    assert!(!parts[1].is_empty());
    assert!(parts[2].parse::<u32>().is_ok());

    // Custom: 3 words, no digits, underscore separator
    let result = render("{{haiku(words=3, digits=0, separator=\"_\")}}");
    let parts: Vec<&str> = result.split('_').collect();
    assert_eq!(parts.len(), 3);
    assert!(parts.iter().all(|p| p.parse::<u32>().is_err())); // no numbers
}
#[tokio::test]
async fn test_quote() {
    let template = "{{ \"quoted'str\" | quote }}";
    let expected = "'quoted'\\''str'";
    assert_eq!(render_v2(template), expected);
    assert_eq!(render_v1(template), expected);
}
#[tokio::test]
async fn test_quote_for_cmd() {
    for engine in [
        TeraEngine::V1(Box::new(get_tera_v1(None))),
        TeraEngine::V2(Box::new(get_tera_v2(None))),
    ] {
        let mut tera = quote_for_cmd(engine);
        let mut quote = |value: &str| {
            let mut ctx = BASE_CONTEXT.clone();
            ctx.insert("cwd", "/");
            ctx.insert("value", value);
            render_str(&mut tera, "{{ value | quote }}", &ctx).unwrap()
        };
        // cmd.exe reads neither `'` nor `\"` as a quote, and expands `%VAR%` inside quotes.
        assert_eq!(quote("my title"), r#""my title""#);
        assert_eq!(
            quote(r"C:\Program Files\node.exe"),
            r#""C:\Program Files\node.exe""#
        );
        assert_eq!(quote("it's"), "it's");
        assert_eq!(quote("50%"), "50^%");
        assert_eq!(quote("50% & more"), r#""50"^%" & more""#);
        // A backslash before a quote added around `%` is doubled for the child.
        assert_eq!(
            quote(r"C:\data\%cache folder"),
            r#""C:\data\\"^%"cache folder""#
        );
        assert_eq!(quote(r#"a" & b"#), r#"^"a\^" ^& b^""#);
    }
}
#[tokio::test]
async fn test_as_str() {
    assert_eq!(render("{{ true | as_str }}"), "true");
    assert_eq!(render("{{ \"hello\" | as_str }}"), "hello");
}
#[tokio::test]
async fn test_kebabcase() {
    let s = render("{{ \"thisFilter\" | kebabcase }}");
    assert_eq!(s, "this-filter");
}
#[tokio::test]
async fn test_lowercamelcase() {
    let s = render("{{ \"Camel-case\" | lowercamelcase }}");
    assert_eq!(s, "camelCase");
}
#[tokio::test]
async fn test_shoutykebabcase() {
    let s = render("{{ \"kebabCase\" | shoutykebabcase }}");
    assert_eq!(s, "KEBAB-CASE");
}
#[tokio::test]
async fn test_shoutysnakecase() {
    let s = render("{{ \"snakeCase\" | shoutysnakecase }}");
    assert_eq!(s, "SNAKE_CASE");
}
#[tokio::test]
async fn test_snakecase() {
    let s = render("{{ \"snakeCase\" | snakecase }}");
    assert_eq!(s, "snake_case");
}
#[tokio::test]
async fn test_uppercamelcase() {
    let s = render("{{ \"CamelCase\" | uppercamelcase }}");
    assert_eq!(s, "CamelCase");
}
#[tokio::test]
async fn test_hash() {
    // SHA256 of "foo" is 2c26b46b68ffc68ff99b453c1d30413413422d706483bfa0f98a5e886266e7ae
    let s = render("{{ \"foo\" | hash(len=8) }}");
    assert_eq!(s, "2c26b46b");
    // Test explicit sha256
    let s = render("{{ \"foo\" | hash(algorithm=\"sha256\", len=8) }}");
    assert_eq!(s, "2c26b46b");
    // Test blake3 - BLAKE3 of "foo" starts with 04e0bb39
    let s = render("{{ \"foo\" | hash(algorithm=\"blake3\", len=8) }}");
    assert_eq!(s, "04e0bb39");
}
#[tokio::test]
#[cfg(unix)]
async fn test_absolute() {
    let s = render("{{ \"/a/b/../c\" | absolute }}");
    assert_eq!(s, "/a/c");
    // relative path
    let s = render("{{ \"a/b/../c\" | absolute }}");
    assert!(s.ends_with("/a/c"));
}
#[tokio::test]
async fn test_dirname() {
    let s = render(r#"{{ "a/b/c" | dirname }}"#);
    assert_eq!(s, "a/b");
}
#[tokio::test]
async fn test_basename() {
    let s = render(r#"{{ "a/b/c" | basename }}"#);
    assert_eq!(s, "c");
}
#[tokio::test]
async fn test_extname() {
    let s = render(r#"{{ "a/b/c.txt" | extname }}"#);
    assert_eq!(s, "txt");
}
#[tokio::test]
async fn test_file_stem() {
    let s = render(r#"{{ "a/b/c.txt" | file_stem }}"#);
    assert_eq!(s, "c");
}
#[tokio::test]
#[cfg(unix)]
async fn test_join_path() {
    let s = render(r#"{{ ["..", "fixtures", "shorthands.toml"] | join_path }}"#);
    assert_eq!(s, "../fixtures/shorthands.toml");
}
#[tokio::test]
async fn test_semver_matching() {
    let s = render(
        r#"{% set p = "1.10.2" %}{% if p is semver_matching(requirement="^1.10.0") %} ok {% endif %}"#,
    );
    assert_eq!(s.trim(), "ok");
}
#[tokio::test]
#[cfg(unix)]
async fn test_read_file() {
    use std::fs;
    use tempfile::TempDir;

    // Create a temp directory and test file
    let temp_dir = TempDir::new().unwrap();
    let test_file_path = temp_dir.path().join("test.txt");
    fs::write(&test_file_path, "test content\nwith multiple lines").unwrap();

    // Test with the temp file
    let mut tera_ctx = BASE_CONTEXT.clone();
    tera_ctx.insert("config_root", &temp_dir.path().to_str().unwrap());
    tera_ctx.insert("cwd", temp_dir.path().to_str().unwrap());
    let mut tera = locked_tera(Some(temp_dir.path()));

    let s = render_str(&mut tera, r#"{{ read_file(path="test.txt") }}"#, &tera_ctx).unwrap();
    assert_eq!(s, "test content\nwith multiple lines");

    // Test with trim filter
    let s = render_str(
        &mut tera,
        r#"{{ read_file(path="test.txt") | trim }}"#,
        &tera_ctx,
    )
    .unwrap();
    assert_eq!(s, "test content\nwith multiple lines");
}
