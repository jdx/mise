pub use mise_util::git::*;

#[cfg(test)]
mod tests {
    use crate::config::{Settings, SettingsExt};
    use std::process::Command;

    use super::*;

    #[test]
    fn network_calls_reject_unsupported_plumbing_fields() {
        use super::{GitPlumbing, PlumbingCall};
        let temp = tempfile::tempdir().unwrap();
        let repo = GitPlumbing::new(temp.path().join("unused.git"));
        for call in [
            PlumbingCall::new(["fetch"]).work_tree(temp.path()),
            PlumbingCall::new(["fetch"]).index_file(temp.path()),
            PlumbingCall::new(["fetch"]).stdin(b"unexpected"),
        ] {
            assert!(
                repo.network_output(call)
                    .unwrap_err()
                    .to_string()
                    .contains("network Git calls do not accept")
            );
        }
    }

    /// Regression test for https://github.com/jdx/mise/discussions/9472:
    /// gix's `with_ref_name` panics ("we map by name only and have no
    /// object-id in refspec") when given a commit SHA. Our `clone()` must
    /// detect that case and fall back to a plain clone + checkout.
    ///
    /// Covers both the gix backend (where the panic originates) and a SHA
    /// reachable only from a non-default branch (so the clone must be full,
    /// not shallow, for the checkout to find the object).
    #[test]
    fn clone_by_sha_does_not_panic() {
        let _settings = crate::test::SettingsGuard::lock();
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        std::fs::create_dir_all(&src).unwrap();

        let git_in = |dir: &std::path::Path, args: &[&str]| {
            let out = Command::new("git")
                .args(args)
                .current_dir(dir)
                .output()
                .expect("spawn git");
            assert!(
                out.status.success(),
                "git {args:?} failed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            out
        };
        git_in(&src, &["-c", "init.defaultBranch=main", "init", "-q"]);
        git_in(
            &src,
            &[
                "-c",
                "user.email=t@t",
                "-c",
                "user.name=t",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                "main",
            ],
        );
        // Park the SHA we want to check out on a non-default branch, so the
        // test would fail if `clone()` did a shallow / single-branch clone.
        git_in(&src, &["checkout", "-q", "-b", "feature"]);
        git_in(
            &src,
            &[
                "-c",
                "user.email=t@t",
                "-c",
                "user.name=t",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                "feature",
            ],
        );
        let sha = String::from_utf8(git_in(&src, &["rev-parse", "HEAD"]).stdout)
            .unwrap()
            .trim()
            .to_string();
        assert_eq!(sha.len(), 40);
        // Move feature off HEAD so the SHA isn't on the default branch.
        git_in(&src, &["checkout", "-q", "main"]);

        let url = format!("file://{}", src.display());

        // gix path — the panic site. Settings::gix defaults to true, but make
        // it explicit so the test is robust to future default changes.
        Settings::override_with(|s| {
            s.gix = Some(true);
            s.libgit2 = Some(false);
        });
        let dst_gix = tmp.path().join("dst-gix");
        Git::new(&dst_gix)
            .clone(&url, CloneOptions::default().revision(&sha))
            .expect("gix clone with SHA must not panic and must succeed");
        let head = git_in(&dst_gix, &["rev-parse", "HEAD"]);
        assert_eq!(String::from_utf8(head.stdout).unwrap().trim(), sha);

        // Explicit revisions also accept an unambiguous abbreviated commit ID.
        let short_sha = &sha[..12];
        let dst_short = tmp.path().join("dst-short");
        Git::new(&dst_short)
            .clone(&url, CloneOptions::default().revision(short_sha))
            .expect("clone with abbreviated revision must succeed");
        let head = git_in(&dst_short, &["rev-parse", "HEAD"]);
        assert_eq!(String::from_utf8(head.stdout).unwrap().trim(), sha);

        let dst_invalid = tmp.path().join("dst-invalid");
        let err = Git::new(&dst_invalid)
            .clone(&url, CloneOptions::default().revision("deadbeef"))
            .expect_err("unknown revision must fail");
        assert!(format!("{err:#}").contains("deadbeef"));

        // CLI path — `git clone -b <sha>` is rejected; verify the SHA
        // bypass works there too.
        Settings::override_with(|s| {
            s.gix = Some(false);
            s.libgit2 = Some(false);
        });
        let dst_cli = tmp.path().join("dst-cli");
        Git::new(&dst_cli)
            .clone(&url, CloneOptions::default().branch(&sha))
            .expect("CLI clone with SHA must succeed");
        let head = git_in(&dst_cli, &["rev-parse", "HEAD"]);
        assert_eq!(String::from_utf8(head.stdout).unwrap().trim(), sha);
    }
}
