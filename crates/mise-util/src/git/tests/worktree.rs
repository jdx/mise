use super::*;

#[test]
fn in_linked_worktree_requires_real_worktree_metadata() {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().canonicalize().unwrap();

    // A real linked worktree, and a nested path inside it.
    let main = base.join("main");
    let wt = base.join("wt");
    std::fs::create_dir_all(main.join(".git/worktrees/wt")).unwrap();
    std::fs::create_dir_all(wt.join("packages/api")).unwrap();
    std::fs::write(main.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
    std::fs::write(main.join(".git/worktrees/wt/commondir"), "../..\n").unwrap();
    // git writes HEAD into the private dir as well, which is what a
    // submodule inside this worktree is anchored against below.
    std::fs::write(
        main.join(".git/worktrees/wt/HEAD"),
        "ref: refs/heads/main\n",
    )
    .unwrap();
    std::fs::write(
        wt.join(".git"),
        format!("gitdir: {}\n", main.join(".git/worktrees/wt").display()),
    )
    .unwrap();
    assert!(super::in_linked_worktree(&wt));
    assert!(super::in_linked_worktree(&wt.join("packages/api")));
    assert!(!super::in_linked_worktree(&main));
    assert!(!super::in_linked_worktree(&base));

    // A worktree of a bare repository still counts: it has no main checkout
    // to map onto, but it is a separate working copy.
    let bare = base.join("bare.git");
    let bare_wt = base.join("bare-wt");
    std::fs::create_dir_all(bare.join("worktrees/bare-wt")).unwrap();
    std::fs::create_dir_all(&bare_wt).unwrap();
    std::fs::write(bare.join("HEAD"), "ref: refs/heads/main\n").unwrap();
    std::fs::write(bare.join("worktrees/bare-wt/commondir"), "../..\n").unwrap();
    std::fs::write(
        bare_wt.join(".git"),
        format!("gitdir: {}\n", bare.join("worktrees/bare-wt").display()),
    )
    .unwrap();
    assert!(super::in_linked_worktree(&bare_wt));

    // `git clone --separate-git-dir` whose git dir happens to sit directly
    // under a directory named `worktrees`. The directory exists and is a
    // real git dir, so only the absence of `commondir` distinguishes it;
    // misreading it would move a single checkout off its base port.
    let sep = base.join("sep-checkout");
    let sep_git = base.join("worktrees/sep.git");
    std::fs::create_dir_all(sep_git.join("refs")).unwrap();
    std::fs::create_dir_all(&sep).unwrap();
    std::fs::write(sep_git.join("HEAD"), "ref: refs/heads/main\n").unwrap();
    std::fs::write(sep_git.join("config"), "[core]\n").unwrap();
    std::fs::write(sep.join(".git"), format!("gitdir: {}\n", sep_git.display())).unwrap();
    assert!(!super::in_linked_worktree(&sep));

    // A hand-made private dir under `worktrees/` whose `commondir` names
    // nothing real. The file exists, so only resolving it rejects this.
    let forged = base.join("forged");
    let forged_git = base.join("fake/worktrees/forged");
    std::fs::create_dir_all(&forged_git).unwrap();
    std::fs::create_dir_all(&forged).unwrap();
    std::fs::write(forged_git.join("commondir"), "../../nonexistent\n").unwrap();
    std::fs::write(
        forged.join(".git"),
        format!("gitdir: {}\n", forged_git.display()),
    )
    .unwrap();
    assert!(!super::in_linked_worktree(&forged));

    // Pointing it at a real repository elsewhere is still not a worktree:
    // the pointer must lead back to the dir the private dir sits under, so
    // borrowing an unrelated repo's metadata confers nothing.
    std::fs::create_dir_all(base.join("fake/other-repo")).unwrap();
    std::fs::write(base.join("fake/other-repo/HEAD"), "ref: refs/heads/main\n").unwrap();
    std::fs::write(forged_git.join("commondir"), "../../other-repo\n").unwrap();
    assert!(!super::in_linked_worktree(&forged));

    // Correct target, but no git metadata there yet.
    std::fs::write(forged_git.join("commondir"), "../..\n").unwrap();
    assert!(!super::in_linked_worktree(&forged));

    // Both conditions met is what finally makes it a worktree.
    std::fs::write(base.join("fake/HEAD"), "ref: refs/heads/main\n").unwrap();
    assert!(super::in_linked_worktree(&forged));

    // A submodule points under `modules/`, and a pruned worktree marker
    // names a directory that no longer exists.
    let sub = base.join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(
        sub.join(".git"),
        format!("gitdir: {}\n", main.join(".git/modules/sub").display()),
    )
    .unwrap();
    assert!(!super::in_linked_worktree(&sub));
    let pruned = base.join("pruned");
    std::fs::create_dir_all(&pruned).unwrap();
    std::fs::write(
        pruned.join(".git"),
        format!("gitdir: {}\n", main.join(".git/worktrees/gone").display()),
    )
    .unwrap();
    assert!(!super::in_linked_worktree(&pruned));

    // A submodule inside a linked worktree is still inside it. The same
    // submodule in two worktrees is two working copies, so the walk must
    // continue past its marker rather than calling it a primary checkout.
    let wt_sub = wt.join("sub");
    std::fs::create_dir_all(&wt_sub).unwrap();
    std::fs::write(
        wt_sub.join(".git"),
        format!(
            "gitdir: {}\n",
            main.join(".git/worktrees/wt/modules/sub").display()
        ),
    )
    .unwrap();
    // The marker names a git dir that does not exist yet, which says
    // nothing about what encloses this directory.
    assert!(!super::in_linked_worktree(&wt_sub));
    std::fs::create_dir_all(main.join(".git/worktrees/wt/modules/sub")).unwrap();
    assert!(super::in_linked_worktree(&wt_sub));

    // A submodule whose own path is `modules/foo` nests the component:
    // git stores it at `<enclosing>/modules/modules/foo`. The container
    // `.git/modules` has no HEAD, so only looking past it finds the real
    // enclosing git dir.
    let nested_name = wt.join("modules/foo");
    std::fs::create_dir_all(&nested_name).unwrap();
    std::fs::create_dir_all(main.join(".git/worktrees/wt/modules/modules/foo")).unwrap();
    std::fs::write(
        nested_name.join(".git"),
        format!(
            "gitdir: {}\n",
            main.join(".git/worktrees/wt/modules/modules/foo").display()
        ),
    )
    .unwrap();
    assert!(super::in_linked_worktree(&nested_name));

    // An unrelated repository whose git dir merely sits under a directory
    // named `modules` is not a submodule: the path before `modules` is not
    // a git dir, so it must not inherit the worktree's offset.
    let lookalike = wt.join("lookalike");
    let lookalike_git = base.join("plain/modules/repo");
    std::fs::create_dir_all(&lookalike_git).unwrap();
    std::fs::create_dir_all(&lookalike).unwrap();
    std::fs::write(lookalike_git.join("HEAD"), "ref: refs/heads/main\n").unwrap();
    std::fs::write(
        lookalike.join(".git"),
        format!("gitdir: {}\n", lookalike_git.display()),
    )
    .unwrap();
    assert!(!super::in_linked_worktree(&lookalike));

    // Give the path before `modules` git metadata and it becomes a real
    // submodule layout, which does follow its worktree.
    std::fs::write(base.join("plain/HEAD"), "ref: refs/heads/main\n").unwrap();
    assert!(super::in_linked_worktree(&lookalike));

    // An independent repository nested in a worktree is its own single
    // copy, so it stops the walk like a nested `.git` directory would.
    let nested = wt.join("vendored");
    let nested_git = base.join("vendored-git");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::create_dir_all(&nested_git).unwrap();
    std::fs::write(nested_git.join("HEAD"), "ref: refs/heads/main\n").unwrap();
    std::fs::write(
        nested.join(".git"),
        format!("gitdir: {}\n", nested_git.display()),
    )
    .unwrap();
    assert!(!super::in_linked_worktree(&nested));
}

#[test]
fn worktree_main_checkout_equivalent() {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().canonicalize().unwrap();
    let main = base.join("main");
    let wt = base.join("wt");
    std::fs::create_dir_all(main.join(".git/worktrees/wt")).unwrap();
    std::fs::create_dir_all(wt.join("sub")).unwrap();
    std::fs::write(main.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
    std::fs::write(main.join(".git/worktrees/wt/commondir"), "../..\n").unwrap();
    std::fs::write(
        wt.join(".git"),
        format!("gitdir: {}\n", main.join(".git/worktrees/wt").display()),
    )
    .unwrap();

    // worktree root and nested paths map to the main checkout
    assert_eq!(super::main_checkout_equivalent(&wt), Some(main.clone()));
    assert_eq!(
        super::main_checkout_equivalent(&wt.join("sub/mise.toml")),
        Some(main.join("sub/mise.toml"))
    );
    // main checkout and non-repo paths do not map
    assert_eq!(super::main_checkout_equivalent(&main), None);
    assert_eq!(super::main_checkout_equivalent(&base), None);

    // worktree of a bare repo does not map
    let bare = base.join("bare.git");
    let bare_wt = base.join("bare-wt");
    std::fs::create_dir_all(bare.join("worktrees/bare-wt")).unwrap();
    std::fs::create_dir_all(&bare_wt).unwrap();
    std::fs::write(bare.join("HEAD"), "ref: refs/heads/main\n").unwrap();
    std::fs::write(bare.join("worktrees/bare-wt/commondir"), "../..\n").unwrap();
    std::fs::write(
        bare_wt.join(".git"),
        format!("gitdir: {}\n", bare.join("worktrees/bare-wt").display()),
    )
    .unwrap();
    assert_eq!(super::main_checkout_equivalent(&bare_wt), None);

    // a submodule also uses a `.git` file but is not a linked worktree:
    // its configs must not inherit the parent checkout's trust
    let subm = main.join("subm");
    std::fs::create_dir_all(main.join(".git/modules/subm")).unwrap();
    std::fs::create_dir_all(&subm).unwrap();
    std::fs::write(
        subm.join(".git"),
        format!("gitdir: {}\n", main.join(".git/modules/subm").display()),
    )
    .unwrap();
    assert_eq!(super::main_checkout_equivalent(&subm), None);
    assert_eq!(
        super::main_checkout_equivalent(&subm.join("mise.toml")),
        None
    );

    // a submodule checked out inside a linked worktree maps through the
    // outer worktree to the same submodule path in the main checkout
    let wt_subm = wt.join("subm");
    std::fs::create_dir_all(&wt_subm).unwrap();
    std::fs::write(
        wt_subm.join(".git"),
        format!("gitdir: {}\n", main.join(".git/modules/subm").display()),
    )
    .unwrap();
    assert_eq!(
        super::main_checkout_equivalent(&wt_subm.join("mise.toml")),
        Some(subm.join("mise.toml"))
    );
}

#[test]
fn git_commands_ignore_inherited_work_tree() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    let cache = tmp.path().join("cache");
    let work_tree = tmp.path().join("work-tree");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::create_dir_all(&work_tree).unwrap();

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
    std::fs::write(src.join("file.txt"), "hello\n").unwrap();
    git_in(&src, &["add", "file.txt"]);
    git_in(
        &src,
        &[
            "-c",
            "user.email=t@t",
            "-c",
            "user.name=t",
            "commit",
            "-q",
            "-m",
            "main",
        ],
    );
    let url = format!("file://{}", src.display());
    let clone = Command::new("git")
        .args(["clone", "-q", &url])
        .arg(&cache)
        .output()
        .expect("spawn git clone");
    assert!(
        clone.status.success(),
        "git clone failed: {}",
        String::from_utf8_lossy(&clone.stderr)
    );
    std::fs::remove_file(cache.join("file.txt")).unwrap();

    let output = sanitize_git_env(
        git_cmd!(&cache, "checkout", "--force", "HEAD")
            .env("GIT_WORK_TREE", &work_tree)
            .env("GIT_INDEX_FILE", work_tree.join("index")),
    )
    .stderr_to_stdout()
    .stdout_capture()
    .unchecked()
    .run()
    .expect("run git checkout");
    assert!(
        output.status.success(),
        "git checkout failed: {}",
        String::from_utf8_lossy(&output.stdout)
    );

    assert!(cache.join("file.txt").exists());
    assert!(!work_tree.join("file.txt").exists());
    assert!(!work_tree.join("index").exists());

    let clone_cache = tmp.path().join("clone-cache");
    sanitize_git_cmd_runner(
        CmdLineRunner::new("git")
            .arg("clone")
            .arg("-q")
            .arg(&url)
            .arg(&clone_cache)
            .env("GIT_WORK_TREE", &work_tree),
    )
    .execute()
    .expect("git clone should ignore inherited GIT_WORK_TREE");

    assert!(clone_cache.join("file.txt").exists());
    assert!(!work_tree.join("file.txt").exists());
}
