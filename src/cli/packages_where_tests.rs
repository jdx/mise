use super::*;
use std::path::Path;

#[test]
fn packages_where_output_preserves_valid_utf8_path_exactly() {
    let path = Path::new("/prefix with spaces/opt/widget");
    assert_eq!(
        system::r#where::path_for_output(path).unwrap(),
        "/prefix with spaces/opt/widget"
    );
}

#[test]
fn packages_where_output_rejects_cr_and_lf() {
    for prefix in ["/prefix\n/opt/widget", "/prefix\r/opt/widget"] {
        let error = system::r#where::path_for_output(Path::new(prefix)).unwrap_err();
        assert!(format!("{error:#}").contains("UTF-8"));
    }
}

#[cfg(unix)]
#[test]
fn packages_where_output_rejects_non_utf8_without_filesystem_access() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let path = PathBuf::from(OsString::from_vec(b"/prefix-\xff/opt/widget".to_vec()));
    let error = system::r#where::path_for_output(&path).unwrap_err();
    assert!(format!("{error:#}").contains("UTF-8"));
}

#[test]
fn packages_where_classifier_accepts_global_and_parent_flags() {
    for args in [
        vec!["mise", "bootstrap", "packages", "where", "brew:widget"],
        vec![
            "mise",
            "--quiet",
            "--cd",
            ".",
            "bootstrap",
            "packages",
            "where",
            "brew:widget",
        ],
        vec![
            "mise",
            "bootstrap",
            "--yes",
            "--only",
            "packages",
            "packages",
            "where",
            "brew:widget",
        ],
        vec![
            "mise",
            "bootstrap",
            "packages",
            "where",
            "--cd",
            ".",
            "--quiet",
            "brew:widget",
        ],
        vec![
            "mise",
            "bootstrap",
            "packages",
            "where",
            "brew:widget",
            "--cd=.",
            "--quiet",
        ],
        vec![
            "mise",
            "-C.",
            "bootstrap",
            "packages",
            "where",
            "--",
            "brew:widget",
        ],
        vec![
            "mise",
            "--env",
            "bootstrap",
            "bootstrap",
            "packages",
            "where",
            "brew:widget",
        ],
        vec![
            "mise",
            "bootstrap",
            "--from",
            "packages",
            "packages",
            "where",
            "brew:widget",
        ],
    ] {
        let argv: Vec<String> = args.iter().map(ToString::to_string).collect();
        assert!(is_packages_where_query(&argv), "{args:?}");
        let cli = parse_cli(&args).unwrap_or_else(|error| panic!("{args:?}: {error:?}"));
        let Some(Commands::Bootstrap(bootstrap)) = cli.command else {
            panic!("expected bootstrap for {args:?}");
        };
        assert!(bootstrap.is_packages_where(), "{args:?}");
    }
}

#[test]
fn packages_where_recognition_precedes_query_argument_validation() {
    for args in [
        vec!["mise", "bootstrap", "packages", "where"],
        vec![
            "mise",
            "bootstrap",
            "packages",
            "where",
            "--unknown-query-flag",
        ],
        vec![
            "mise",
            "bootstrap",
            "packages",
            "where",
            "--unknown-query-flag",
            "brew:widget",
        ],
        vec![
            "mise",
            "bootstrap",
            "packages",
            "where",
            "brew:widget",
            "--unknown-query-flag",
        ],
        vec![
            "mise",
            "bootstrap",
            "packages",
            "where",
            "brew:widget",
            "brew:extra",
        ],
        vec!["mise", "bootstrap", "packages", "where", "--help"],
        vec!["mise", "bootstrap", "packages", "where", "--cd"],
        vec!["mise", "--cd", "/missing", "bootstrap", "packages", "where"],
        vec!["mise", "bootstrap", "--only=packages", "packages", "where"],
        vec!["mise", "-q", "bootstrap", "-y", "packages", "where"],
    ] {
        let argv: Vec<String> = args.iter().map(ToString::to_string).collect();
        assert!(is_packages_where_query(&argv), "{args:?}");
    }
}

#[test]
fn packages_where_recognizer_distinguishes_flag_values_tasks_exec_and_separators() {
    for args in [
        vec!["mise"],
        vec!["mise", "bootstrap", "packages"],
        vec!["mise", "bootstrap", "packages", "status"],
        vec![
            "mise",
            "--env",
            "bootstrap",
            "packages",
            "where",
            "brew:widget",
        ],
        vec![
            "mise",
            "--env=bootstrap",
            "packages",
            "where",
            "brew:widget",
        ],
        vec!["mise", "-Ebootstrap", "packages", "where", "brew:widget"],
        vec![
            "mise",
            "--cd",
            "bootstrap",
            "packages",
            "where",
            "brew:widget",
        ],
        vec![
            "mise",
            "bootstrap",
            "--from",
            "packages",
            "where",
            "brew:widget",
        ],
        vec![
            "mise",
            "bootstrap",
            "--from=packages",
            "where",
            "brew:widget",
        ],
        vec![
            "mise",
            "bootstrap",
            "--only",
            "packages",
            "where",
            "brew:widget",
        ],
        vec![
            "mise",
            "exec",
            "--",
            "bootstrap",
            "packages",
            "where",
            "brew:widget",
        ],
        vec![
            "mise",
            "x",
            "--",
            "bootstrap",
            "packages",
            "where",
            "brew:widget",
        ],
        vec![
            "mise",
            "run",
            "bootstrap",
            "packages",
            "where",
            "brew:widget",
        ],
        vec![
            "mise",
            "task-name",
            "bootstrap",
            "packages",
            "where",
            "brew:widget",
        ],
        vec![
            "mise",
            "--",
            "bootstrap",
            "packages",
            "where",
            "brew:widget",
        ],
        vec![
            "mise",
            "bootstrap",
            "--",
            "packages",
            "where",
            "brew:widget",
        ],
        vec![
            "mise",
            "bootstrap",
            "packages",
            "--",
            "where",
            "brew:widget",
        ],
    ] {
        let argv: Vec<String> = args.iter().map(ToString::to_string).collect();
        assert!(!is_packages_where_query(&argv), "{args:?}");
    }
}

#[test]
fn packages_where_classifier_distinguishes_other_bootstrap_commands() {
    for args in [
        vec!["mise", "bootstrap"],
        vec!["mise", "bootstrap", "packages", "status"],
        vec!["mise", "bootstrap", "packages", "apply", "brew:widget"],
        vec!["mise", "bootstrap", "status"],
        vec!["mise", "bootstrap", "--from", "where", "packages", "status"],
    ] {
        let cli = parse_cli(&args).unwrap_or_else(|error| panic!("{args:?}: {error:?}"));
        let Some(Commands::Bootstrap(bootstrap)) = cli.command else {
            panic!("expected bootstrap for {args:?}");
        };
        assert!(!bootstrap.is_packages_where(), "{args:?}");
    }
}

#[test]
fn packages_where_parser_requires_exactly_one_package_and_valid_flags() {
    for suffix in [
        vec![],
        vec!["brew:widget", "brew:another"],
        vec!["brew:widget", "--json"],
        vec!["brew:widget", "--install"],
        vec!["brew:widget", "--version", "1"],
        vec!["brew:widget", "--unknown-query-flag"],
    ] {
        let mut args = vec!["mise", "bootstrap", "packages", "where"];
        args.extend(suffix);
        assert!(parse_cli(&args).is_err(), "{args:?}");
    }
}
