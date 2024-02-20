use crate::cli::prelude::*;
use eyre::Result;
use predicates::prelude::*;

// From e2e/test_exec
#[test]
fn test_exec_change_directory() -> Result<()> {
    mise! {
        given_environment!(has_root_files direnv_fixture());
        when!(
            given!(args "exec", "-C", "$ROOT/direnv", "--", "pwd");
            should!(output_exactly "$ROOT/direnv\n"),
            should!(succeed),
        ),
        when!(
            given!(args "exec", "-C", "./direnv", "--", "pwd");
            should!(output_exactly "$ROOT/direnv\n"),
            should!(succeed),
        ),
        when!(
            given!(args "exec", "-C", "$ROOT/direnv", "--", "pwd");
            should!(output_exactly "$ROOT/direnv\n"),
            should!(succeed),
        ),
    }
}

fn direnv_fixture() -> File {
    File {
        path: "direnv/empty".into(),
        content: String::new(),
    }
}

// From e2e/test_erlang
#[test]
#[ignore]
fn test_exec_erlang() -> Result<()> {
    mise! {
        when!(
            given!(env_var "MISE_EXPERIMENTAL", "1"),
            given!(
                args
                "x",
                "erlang@24.3.4.9",
                "--",
                "erl",
                "-eval",
                "erlang:display(erlang:system_info(otp_release)),
                halt().",
                "-noshell",
            );
            should!(output "24"),
            should!(succeed),
        ),
    }
}
