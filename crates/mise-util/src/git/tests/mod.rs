use super::*;
use crate::cmd::CmdLineRunner;
use std::process::Command;

#[test]
fn sha_detection() {
    assert!(looks_like_sha("0123456789abcdef0123456789abcdef01234567"));
    assert!(looks_like_sha(
        "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
    ));
    assert!(!looks_like_sha("main"));
    assert!(!looks_like_sha("v1.2.3"));
    assert!(!looks_like_sha("abcdef1")); // short SHA not supported
    assert!(!looks_like_sha(""));
    assert!(!looks_like_sha("g123456789abcdef0123456789abcdef01234567")); // non-hex
}

#[test]
fn abbreviated_sha_detection() {
    assert!(super::looks_like_abbreviated_sha("1f22e02"));
    assert!(super::looks_like_abbreviated_sha(
        "1f22e025e8c0d77ee3176102"
    ));
    assert!(!super::looks_like_abbreviated_sha("cafe")); // too short to be a git abbreviation
    assert!(!super::looks_like_abbreviated_sha("main"));
    assert!(!super::looks_like_abbreviated_sha("v1.2.3"));
    assert!(!super::looks_like_abbreviated_sha(
        "1f22e025e8c0d77ee3176102c26a3fd6fd770cb5"
    ));
}

#[test]
fn remote_ref_parser_prefers_branches_over_tags() {
    let output = "\
aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\trefs/heads/release
bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\trefs/tags/release
";
    assert_eq!(
        super::remote_ref_kind(output, "refs/heads/release", "refs/tags/release"),
        Some(super::RemoteRefKind::Branch)
    );
    assert_eq!(
        super::remote_ref_kind(output, "refs/heads/missing", "refs/tags/release"),
        Some(super::RemoteRefKind::Tag)
    );
    assert_eq!(
        super::remote_ref_kind(output, "refs/heads/missing", "refs/tags/missing"),
        None
    );
}

#[test]
fn reads_files_from_the_merge_base_and_head() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let git = |args: &[&str]| {
        let output = Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().to_string()
    };
    git(&["-c", "init.defaultBranch=main", "init", "-q"]);
    git(&["config", "user.email", "test@example.com"]);
    git(&["config", "user.name", "Test"]);
    std::fs::write(root.join("lockfile"), "before\n").unwrap();
    git(&["add", "lockfile"]);
    git(&["commit", "-q", "-m", "before"]);
    let base = git(&["rev-parse", "HEAD"]);
    std::fs::write(root.join("lockfile"), "after\n").unwrap();
    git(&["commit", "-q", "-am", "after"]);
    let head = git(&["rev-parse", "HEAD"]);

    let repo = Git::new(root);
    assert_eq!(repo.merge_base(&base, &head).unwrap(), base);
    assert_eq!(
        repo.file_at_revision(&base, std::path::Path::new("lockfile"))
            .unwrap()
            .as_deref(),
        Some("before\n")
    );
    assert_eq!(
        repo.file_at_revision(&head, std::path::Path::new("lockfile"))
            .unwrap()
            .as_deref(),
        Some("after\n")
    );
    assert_eq!(
        repo.file_at_revision(&head, std::path::Path::new("missing"))
            .unwrap(),
        None
    );
}

#[test]
fn update_resolves_short_branches_and_tags() {
    let tmp = tempfile::tempdir().unwrap();
    let origin = tmp.path().join("origin");
    std::fs::create_dir_all(&origin).unwrap();

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
        String::from_utf8(out.stdout).unwrap().trim().to_string()
    };
    git_in(&origin, &["-c", "init.defaultBranch=main", "init", "-q"]);
    git_in(&origin, &["config", "user.email", "test@example.com"]);
    git_in(&origin, &["config", "user.name", "Test"]);
    // A global tag.gpgsign would turn the lightweight tag below into one that needs a message.
    git_in(&origin, &["config", "tag.gpgsign", "false"]);

    std::fs::write(origin.join("version"), "release\n").unwrap();
    git_in(&origin, &["add", "version"]);
    git_in(&origin, &["commit", "-q", "-m", "release"]);
    let release_sha = git_in(&origin, &["rev-parse", "HEAD"]);
    git_in(&origin, &["branch", "release-branch"]);
    git_in(&origin, &["branch", "collision"]);
    git_in(&origin, &["tag", "lightweight-v1"]);
    git_in(
        &origin,
        &["tag", "-a", "annotated-v1", "-m", "annotated-v1"],
    );

    std::fs::write(origin.join("version"), "tag-collision\n").unwrap();
    git_in(&origin, &["commit", "-q", "-am", "tag collision"]);
    let tag_collision_sha = git_in(&origin, &["rev-parse", "HEAD"]);
    git_in(&origin, &["tag", "-a", "collision", "-m", "collision"]);

    std::fs::write(origin.join("version"), "main\n").unwrap();
    git_in(&origin, &["commit", "-q", "-am", "main"]);

    let cases = [
        ("release-branch", release_sha.as_str()),
        ("lightweight-v1", release_sha.as_str()),
        ("annotated-v1", release_sha.as_str()),
        ("refs/tags/annotated-v1", release_sha.as_str()),
        (release_sha.as_str(), release_sha.as_str()),
        ("collision", release_sha.as_str()),
        ("refs/tags/collision", tag_collision_sha.as_str()),
    ];
    let url = format!("file://{}", origin.display());
    for (index, (selector, expected_sha)) in cases.into_iter().enumerate() {
        let clone = tmp.path().join(format!("clone-{index}"));
        git_in(tmp.path(), &["clone", "-q", &url, clone.to_str().unwrap()]);
        if selector == "annotated-v1" {
            // A stale local branch must not override a remote tag after the
            // remote branch has disappeared.
            git_in(&clone, &["branch", "annotated-v1"]);
        }

        Git::new(&clone)
            .update(Some(selector.to_string()))
            .unwrap_or_else(|err| panic!("update {selector} failed: {err:#}"));
        assert_eq!(
            git_in(&clone, &["rev-parse", "HEAD"]),
            expected_sha,
            "selector {selector} checked out the wrong commit"
        );
    }

    let clone = tmp.path().join("clone-update-tag-full-ref");
    git_in(tmp.path(), &["clone", "-q", &url, clone.to_str().unwrap()]);
    Git::new(&clone)
        .update_tag("refs/tags/annotated-v1".to_string())
        .unwrap_or_else(|err| panic!("update_tag with full ref failed: {err:#}"));
    assert_eq!(git_in(&clone, &["rev-parse", "HEAD"]), release_sha);
}

#[test]
fn works_when_the_remote_is_not_named_origin() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("source");
    std::fs::create_dir_all(&source).unwrap();
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
        String::from_utf8(out.stdout).unwrap().trim().to_string()
    };
    git_in(&source, &["-c", "init.defaultBranch=main", "init", "-q"]);
    git_in(&source, &["config", "user.email", "test@example.com"]);
    git_in(&source, &["config", "user.name", "Test"]);
    git_in(&source, &["config", "commit.gpgsign", "false"]);
    std::fs::write(source.join("version"), "one\n").unwrap();
    git_in(&source, &["add", "version"]);
    git_in(&source, &["commit", "-q", "-m", "one"]);
    git_in(&source, &["branch", "other"]);

    let url = format!("file://{}", source.display());
    let clone = tmp.path().join("clone");
    git_in(
        tmp.path(),
        &[
            "clone",
            "-q",
            "-o",
            "upstream",
            &url,
            clone.to_str().unwrap(),
        ],
    );
    git_in(&clone, &["config", "user.email", "test@example.com"]);
    git_in(&clone, &["config", "user.name", "Test"]);

    std::fs::write(source.join("version"), "two\n").unwrap();
    git_in(&source, &["commit", "-q", "-am", "two"]);
    let new_sha = git_in(&source, &["rev-parse", "HEAD"]);

    let git = Git::new(&clone);
    assert_eq!(git.get_remote_url().as_deref(), Some(url.as_str()));
    assert_eq!(
        git.remote_sha("main").unwrap().as_deref(),
        Some(new_sha.as_str())
    );
    git.update(None).unwrap();
    assert_eq!(git_in(&clone, &["rev-parse", "HEAD"]), new_sha);
    git.update(Some("other".to_string())).unwrap();
    assert_eq!(
        git_in(&clone, &["rev-parse", "--abbrev-ref", "HEAD"]),
        "other"
    );
}

mod worktree;

#[test]
fn test_pick_remote_name() {
    assert_eq!(pick_remote_name(""), "origin");
    assert_eq!(pick_remote_name("origin\n"), "origin");
    assert_eq!(pick_remote_name("upstream\n"), "upstream");
    assert_eq!(pick_remote_name("fork\norigin\n"), "origin");
    assert_eq!(pick_remote_name("upstream\nfork\n"), "upstream");
}
