use insta::assert_debug_snapshot;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use tempfile::TempDir;

use super::*;

#[test]
fn test_plugin_entry_to_url_keeps_ref_outside_shorthand_expansion() {
    assert_eq!(
        plugin_entry_to_url("owner/repo#v1.2.0"),
        "https://github.com/owner/repo.git#v1.2.0"
    );
    assert_eq!(
        plugin_entry_to_url("owner/repo"),
        "https://github.com/owner/repo.git"
    );
    assert_eq!(
        plugin_entry_to_url("https://example.com/repo.git#main"),
        "https://example.com/repo.git#main"
    );
    for entry in [
        "git::ssh://git@example.com:7999/org/repo.git//plugin?ref=v1.2.0",
        "git::https://example.com/org/repo.git//plugin?ref=v1.2.0",
        "git::https://example.com/org/repo.git//plugin",
    ] {
        assert_eq!(plugin_entry_to_url(entry), entry);
    }
}

#[test]
fn test_resolve_task_template_tracks_definition_sources() -> Result<()> {
    let project_root = PathBuf::from("/workspace/packages/app");
    let template_source = PathBuf::from("/workspace/mise.toml");
    let defaults_source = PathBuf::from("/workspace/mise.local.toml");
    let mut definitions = TaskDefinitions::default();
    definitions.templates.insert(
        "build-template".to_string(),
        SourcedTaskTemplate {
            template: TaskTemplate {
                run: vec![RunEntry::Script("echo build".to_string())],
                ..Default::default()
            },
            source: template_source.clone(),
        },
    );
    definitions.workspace_defaults = Some(WorkspaceTaskDefaults {
        project_roots: BTreeSet::from([project_root.clone()]),
        tasks: IndexMap::from([(
            "build".to_string(),
            SourcedTaskTemplate {
                template: TaskTemplate {
                    description: "default description".to_string(),
                    ..Default::default()
                },
                source: defaults_source.clone(),
            },
        )]),
    });
    let primary_source = project_root.join("mise.toml");
    let mut task = Task {
        name: "build".to_string(),
        extends: Some("build-template".to_string()),
        config_source: primary_source.clone(),
        config_root: Some(project_root),
        ..Default::default()
    };

    resolve_task_template(&mut task, &definitions)?;

    assert_eq!(
        task.config_sources(),
        vec![
            primary_source.as_path(),
            template_source.as_path(),
            defaults_source.as_path(),
        ]
    );
    Ok(())
}

#[test]
fn test_task_config_rust_cache_is_a_default() {
    let rust_default = Some(TaskRustCacheConfig::default());
    let mut inherited = Task::default();
    apply_task_config_rust_cache_default(&mut inherited, &rust_default);
    assert_eq!(inherited.rust_cache, rust_default);

    let mut disabled = Task {
        rust_cache: Some(TaskRustCacheConfig { enabled: false }),
        ..Default::default()
    };
    apply_task_config_rust_cache_default(&mut disabled, &rust_default);
    assert_eq!(
        disabled.rust_cache,
        Some(TaskRustCacheConfig { enabled: false })
    );
}

#[test]
fn test_collect_task_files_skips_dangling_symlinks() -> Result<()> {
    let tmp = TempDir::new()?;
    let task = tmp.path().join("task");
    fs::write(&task, "echo ok")?;
    std::os::unix::fs::symlink("missing", tmp.path().join("dangling"))?;

    assert_eq!(collect_task_files(tmp.path(), &[])?, vec![task]);

    Ok(())
}

#[test]
fn test_collect_task_files_prunes_excluded_directories() -> Result<()> {
    let tmp = TempDir::new()?;
    let excluded = tmp.path().join("excluded");
    fs::create_dir(&excluded)?;
    fs::write(excluded.join("pyproject.toml"), "[project]")?;
    let task = tmp.path().join("task");
    fs::write(&task, "echo ok")?;

    assert_eq!(
        collect_task_files(tmp.path(), std::slice::from_ref(&excluded))?,
        vec![task]
    );

    Ok(())
}

#[test]
fn test_collect_task_files_follows_valid_directory_symlinks() -> Result<()> {
    let tmp = TempDir::new()?;
    let target = tmp.path().join("target");
    fs::create_dir(&target)?;
    fs::write(target.join("task"), "echo ok")?;
    let root = tmp.path().join("root");
    fs::create_dir(&root)?;
    std::os::unix::fs::symlink(&target, root.join("linked"))?;

    assert_eq!(
        collect_task_files(&root, &[])?,
        vec![root.join("linked/task")]
    );

    Ok(())
}

#[test]
fn test_collect_task_files_skips_links_back_into_the_walk() -> Result<()> {
    let tmp = TempDir::new()?;
    let root = tmp.path().join("tasks");
    fs::create_dir(&root)?;
    let task = root.join("healthy");
    fs::write(&task, "echo ok")?;

    // The control: the same tree one link earlier. Without it, "the loop changed nothing"
    // is not something this test can claim.
    let before = collect_task_files(&root, &[])?;
    assert_eq!(before, vec![task]);

    // A `current`-style link pointing at the directory being walked. Nothing under it is
    // lost -- it is the same directory, already visited -- so it must not fail the walk.
    std::os::unix::fs::symlink(&root, root.join("current"))?;

    assert_eq!(collect_task_files(&root, &[])?, before);

    Ok(())
}

#[test]
fn test_collect_task_files_preserves_unresolvable_symlink_errors() -> Result<()> {
    // Renamed from `..._preserves_symlink_loop_errors`: what it pins is a link that cannot be
    // resolved at all. `fs::metadata` fails with ELOOP before walkdir gets as far as its own
    // ancestor check, so this arrives as an io error rather than a `loop_ancestor` one and is
    // deliberately still fatal -- unlike the case above, nothing was reached through it.
    let tmp = TempDir::new()?;
    std::os::unix::fs::symlink("loop", tmp.path().join("loop"))?;

    assert!(collect_task_files(tmp.path(), &[]).is_err());

    Ok(())
}

#[tokio::test]
async fn test_load() {
    let config = Config::reset().await.unwrap();
    assert_debug_snapshot!(config);
}

#[tokio::test]
async fn test_reset_reloads_settings() {
    let _settings = crate::test::SettingsGuard::lock();
    Settings::reset(None);
    let before = Settings::get();

    Config::reset().await.unwrap();
    let after = Settings::get();

    assert!(!Arc::ptr_eq(&before, &after));
}

#[test]
fn test_expand_task_inputs_supports_nested_groups() -> Result<()> {
    let task_inputs = ResolvedTaskInputs {
        input_groups: IndexMap::from([
            (
                "shared".to_string(),
                (
                    vec![
                        "Cargo.toml".to_string(),
                        "src/**/*.rs".to_string(),
                        "\\!important.txt".to_string(),
                    ],
                    PathBuf::from("/workspace"),
                ),
            ),
            (
                "production".to_string(),
                (
                    vec![
                        "@group:shared".to_string(),
                        "!src/**/*_test.rs".to_string(),
                        "src/**/*.rs".to_string(),
                    ],
                    PathBuf::from("/project"),
                ),
            ),
        ]),
        ..Default::default()
    };
    let root = Path::new("/workspace");

    assert_eq!(
        expand_task_inputs(
            &["local.txt".to_string(), "@group:production".to_string()],
            &task_inputs,
            root,
            "build",
            &mut vec![],
            false,
        )?,
        vec![
            "local.txt",
            "/workspace/Cargo.toml",
            "/workspace/src/**/*.rs",
            "/workspace/!important.txt",
            "!/project/src/**/*_test.rs",
            "/project/src/**/*.rs",
        ]
    );
    Ok(())
}

#[test]
fn test_expand_task_inputs_rejects_unknown_and_cyclic_groups() {
    let task_inputs = ResolvedTaskInputs {
        input_groups: IndexMap::from([
            (
                "a".to_string(),
                (vec!["@group:b".to_string()], PathBuf::from("/workspace")),
            ),
            (
                "b".to_string(),
                (vec!["@group:a".to_string()], PathBuf::from("/workspace")),
            ),
        ]),
        ..Default::default()
    };

    let unknown = expand_task_inputs(
        &["@group:missing".to_string()],
        &task_inputs,
        Path::new("/workspace"),
        "build",
        &mut vec![],
        false,
    )
    .unwrap_err();
    assert!(
        unknown
            .to_string()
            .contains("undefined input group \"missing\"")
    );

    let cycle = expand_task_inputs(
        &["@group:a".to_string()],
        &task_inputs,
        Path::new("/workspace"),
        "build",
        &mut vec![],
        false,
    )
    .unwrap_err();
    assert!(cycle.to_string().contains("a -> b -> a"));
}

#[test]
fn test_dir_is_in_enclosing_monorepo() {
    // https://github.com/jdx/mise/discussions/11276: a worktree checked out inside
    // the main checkout, both setting monorepo_root = true.
    let selected = Path::new("/repo/.worktrees/wt");
    let enclosing = vec![PathBuf::from("/repo")];

    // the enclosing root itself, and anything between it and the selected root
    assert!(dir_is_in_enclosing_monorepo(
        Path::new("/repo"),
        selected,
        &enclosing
    ));
    assert!(dir_is_in_enclosing_monorepo(
        Path::new("/repo/.worktrees"),
        selected,
        &enclosing
    ));

    // the selected root and its subprojects
    assert!(!dir_is_in_enclosing_monorepo(
        selected, selected, &enclosing
    ));
    assert!(!dir_is_in_enclosing_monorepo(
        Path::new("/repo/.worktrees/wt/packages/api"),
        selected,
        &enclosing
    ));

    // above the enclosing root: still inherits normally
    assert!(!dir_is_in_enclosing_monorepo(
        Path::new("/"),
        selected,
        &enclosing
    ));
    assert!(!dir_is_in_enclosing_monorepo(
        Path::new("/home/user"),
        selected,
        &enclosing
    ));

    // no enclosing monorepo: nothing is ever skipped
    assert!(!dir_is_in_enclosing_monorepo(
        Path::new("/repo"),
        selected,
        &[]
    ));
}

#[test]
#[cfg(unix)]
fn test_dir_is_in_enclosing_monorepo_across_symlinked_prefix() {
    // `dir` comes from the cwd walk and the roots from a config's project_root, so
    // they can be different symlink spellings of the same path (Fedora Atomic's
    // /home -> /var/home). The raw prefix check misses; the resolved one must not.
    let tmp = TempDir::new().unwrap();
    let real = tmp.path().join("real");
    let selected = real.join("repo/.worktrees/wt");
    fs::create_dir_all(&selected).unwrap();
    let link = tmp.path().join("link");
    std::os::unix::fs::symlink(&real, &link).unwrap();

    let enclosing = vec![real.join("repo")];

    // the enclosing root, reached through the symlinked prefix
    let dir = link.join("repo");
    assert!(
        !dir.starts_with(&enclosing[0]),
        "raw check should miss here"
    );
    assert!(dir_is_in_enclosing_monorepo(&dir, &selected, &enclosing));

    // the selected root through that same prefix is still not skipped
    let dir = link.join("repo/.worktrees/wt");
    assert!(!dir_is_in_enclosing_monorepo(&dir, &selected, &enclosing));
}

#[test]
fn test_config_set_contains_matches_symlinked_prefix() {
    // Regression for https://github.com/jdx/mise/discussions/10483:
    // a global config discovered via a symlinked path prefix (e.g. Fedora
    // Atomic's `/home` -> `/var/home`) must still be recognized as global.
    let tmp = TempDir::new().unwrap();
    let real_dir = tmp.path().join("real");
    fs::create_dir_all(&real_dir).unwrap();
    let real_file = real_dir.join("config.toml");
    fs::write(&real_file, "").unwrap();
    // Symlinked alias of the dir, mimicking `/home` -> `/var/home`.
    let link_dir = tmp.path().join("link");
    std::os::unix::fs::symlink(&real_dir, &link_dir).unwrap();
    let aliased_file = link_dir.join("config.toml");

    let mut set = IndexSet::new();
    set.insert(real_file.clone());

    // exact match (fast path)
    assert!(config_set_contains(&set, &real_file));
    // same file reached via a symlinked prefix — false before the fix
    assert!(config_set_contains(&set, &aliased_file));
    // an unrelated path is not a member
    assert!(!config_set_contains(&set, &real_dir.join("other.toml")));
}

#[test]
fn test_has_mise_config_with_glob_filenames() -> Result<()> {
    let tmp = TempDir::new()?;
    for pattern in [
        ".config/mise/conf.d/*.toml",
        ".mise/conf.d/*.toml",
        "mise/conf.d/*.toml",
    ] {
        let confd = tmp.path().join(pattern.trim_end_matches("/*.toml"));
        fs::create_dir_all(&confd)?;
        fs::write(confd.join("tools.toml"), "[tools]\n")?;

        assert!(has_mise_config_with_filenames(
            tmp.path(),
            &[pattern.to_string()]
        ));
    }

    Ok(())
}

#[test]
fn test_project_visible_mise_conf_d_precedence() -> Result<()> {
    let tmp = TempDir::new()?;
    let confd = tmp.path().join("mise/conf.d");
    fs::create_dir_all(&confd)?;
    fs::write(confd.join("01-base.toml"), "[env]\nORDER = 'base'\n")?;
    fs::write(
        confd.join("02-override.toml"),
        "[env]\nORDER = 'override'\n",
    )?;
    fs::write(confd.join(".hidden.toml"), "[env]\nORDER = 'hidden'\n")?;
    fs::write(
        tmp.path().join("mise/config.toml"),
        "[env]\nORDER = 'config'\n",
    )?;

    let filenames = vec![
        "mise/conf.d/*.toml".to_string(),
        "mise/config.toml".to_string(),
    ];
    let paths = config_paths_in_dir_with_filenames(tmp.path(), &filenames);
    let relative = paths
        .iter()
        .map(|path| path.strip_prefix(tmp.path()).unwrap())
        .collect_vec();
    assert_eq!(
        relative,
        vec![
            Path::new("mise/config.toml"),
            Path::new("mise/conf.d/02-override.toml"),
            Path::new("mise/conf.d/01-base.toml"),
        ]
    );

    Ok(())
}

#[test]
fn test_project_conf_d_dotted_fragments_remain_unconditional() -> Result<()> {
    let tmp = TempDir::new()?;
    let confd = tmp.path().join(".mise/conf.d");
    fs::create_dir_all(&confd)?;
    for filename in [
        "01-base.toml",
        "02-local.local.toml",
        "03-dev.dev.toml",
        "04-dev-local.dev.local.toml",
        "05-ci.ci.toml",
    ] {
        fs::write(confd.join(filename), "[env]\n")?;
    }

    let filenames = vec![".mise/conf.d/*.toml".to_string()];
    let paths = config_paths_in_dir_with_filenames(tmp.path(), &filenames);
    let relative = paths
        .iter()
        .map(|path| path.strip_prefix(tmp.path()).unwrap())
        .collect_vec();
    assert_eq!(
        relative,
        vec![
            Path::new(".mise/conf.d/05-ci.ci.toml"),
            Path::new(".mise/conf.d/04-dev-local.dev.local.toml"),
            Path::new(".mise/conf.d/03-dev.dev.toml"),
            Path::new(".mise/conf.d/02-local.local.toml"),
            Path::new(".mise/conf.d/01-base.toml"),
        ]
    );

    assert!(is_environment_conf_d_file(&confd.join("03-dev.dev.toml")));
    assert_eq!(
        conf_d_file_environment(&confd.join("03-dev.dev.toml")),
        Some(("dev", false))
    );
    assert!(is_environment_conf_d_file(
        &confd.join("04-dev-local.dev.local.toml")
    ));
    assert_eq!(
        conf_d_file_environment(&confd.join("module.qa.prod.local.toml")),
        Some(("qa.prod", true))
    );
    assert!(!is_environment_conf_d_file(
        &confd.join("02-local.local.toml")
    ));

    Ok(())
}

#[test]
fn test_prefer_windows_file_task_siblings_keeps_windows_native_script() {
    let file_tasks = vec![
        Task {
            name: "pkl:gen".to_string(),
            config_source: PathBuf::from("mise-tasks/pkl/gen"),
            ..Default::default()
        },
        Task {
            name: "pkl:gen.ps1".to_string(),
            config_source: PathBuf::from("mise-tasks/pkl/gen.ps1"),
            ..Default::default()
        },
    ];

    let tasks = prefer_windows_file_task_siblings_inner(file_tasks);

    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].name, "pkl:gen");
    assert_eq!(
        tasks[0].config_source,
        PathBuf::from("mise-tasks/pkl/gen.ps1")
    );
}

#[test]
fn test_prefer_windows_file_task_siblings_ignores_non_windows_extension() {
    let file_tasks = vec![
        Task {
            name: "hello".to_string(),
            config_source: PathBuf::from("mise-tasks/hello"),
            ..Default::default()
        },
        Task {
            name: "hello.sh".to_string(),
            config_source: PathBuf::from("mise-tasks/hello.sh"),
            ..Default::default()
        },
    ];

    let names = prefer_windows_file_task_siblings_inner(file_tasks)
        .into_iter()
        .map(|task| task.name)
        .collect_vec();

    assert_eq!(names, vec!["hello", "hello.sh"]);
}

#[test]
fn test_prefer_windows_file_task_siblings_preserves_toml_overlay_stem() {
    let file_tasks = vec![
        Task {
            name: "hello".to_string(),
            config_source: PathBuf::from("mise-tasks/hello"),
            file: Some(PathBuf::from("mise-tasks/hello")),
            ..Default::default()
        },
        Task {
            name: "hello.ps1".to_string(),
            config_source: PathBuf::from("mise-tasks/hello.ps1"),
            file: Some(PathBuf::from("mise-tasks/hello.ps1")),
            ..Default::default()
        },
    ];
    let config_tasks = vec![Task {
        name: "hello".to_string(),
        description: "windows task metadata".to_string(),
        ..Default::default()
    }];

    let tasks = merge_file_and_config_tasks(
        prefer_windows_file_task_siblings_inner(file_tasks),
        config_tasks,
    );

    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].name, "hello");
    assert_eq!(
        tasks[0].config_source,
        PathBuf::from("mise-tasks/hello.ps1")
    );
    assert_eq!(tasks[0].description, "windows task metadata");
}

/// A file task built the way discovery builds one: the name carries the
/// script's extension and `file` points at the script.
fn file_task(name: &str) -> Task {
    Task {
        name: name.to_string(),
        config_source: PathBuf::from(format!("mise-tasks/{name}")),
        file: Some(PathBuf::from(format!("mise-tasks/{name}"))),
        name_from_file: true,
        ..Default::default()
    }
}

/// A `[tasks.<name>]` block carrying no command of its own.
fn inline_overlay(name: &str) -> Task {
    Task {
        name: name.to_string(),
        config_source: PathBuf::from("mise.toml"),
        ..Default::default()
    }
}

fn inline_task(name: &str, run: &str) -> Task {
    Task {
        run: vec![RunEntry::Script(run.to_string())],
        ..inline_overlay(name)
    }
}

#[test]
fn test_stripped_name_block_overlays_file_task() {
    let tasks = merge_file_and_config_tasks(
        vec![file_task("hello.sh")],
        vec![Task {
            name: "hello".to_string(),
            description: "overlaid".to_string(),
            ..Default::default()
        }],
    );

    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].name, "hello.sh");
    assert_eq!(tasks[0].file, Some(PathBuf::from("mise-tasks/hello.sh")));
    assert_eq!(tasks[0].description, "overlaid");
}

#[test]
fn test_stripped_name_block_overlays_every_file_task_sharing_the_stem() {
    let tasks = merge_file_and_config_tasks(
        vec![file_task("hello.sh"), file_task("hello.js")],
        vec![Task {
            name: "hello".to_string(),
            description: "overlaid".to_string(),
            ..Default::default()
        }],
    );

    assert_eq!(tasks.len(), 2);
    for task in &tasks {
        assert_eq!(task.description, "overlaid");
    }
}

#[test]
fn test_stripped_name_block_defers_to_an_inline_command_with_the_same_name() {
    // `mise.local.toml` contributes metadata, `mise.toml` the command. The
    // metadata block must overlay that inline base, not the file task, or
    // the base below it is dropped. The command also claims `hello.sh`,
    // since `[tasks.hello]` names that script.
    let tasks = merge_file_and_config_tasks(
        vec![file_task("hello.sh")],
        vec![
            Task {
                name: "hello".to_string(),
                description: "from local".to_string(),
                ..Default::default()
            },
            Task {
                name: "hello".to_string(),
                run: vec![RunEntry::Script("echo inline".to_string())],
                ..Default::default()
            },
        ],
    );

    let inline = tasks.iter().find(|t| t.name == "hello").unwrap();
    assert_eq!(inline.description, "from local");
    assert_eq!(
        inline.run,
        vec![RunEntry::Script("echo inline".to_string())]
    );
    assert!(
        !tasks.iter().any(|t| t.name == "hello.sh"),
        "the command claims the script its name reaches"
    );
}

#[test]
fn test_stripped_name_overlay_matches_the_block_name_against_the_stem_only() {
    // "my.app" is a task name that happens to contain a dot, not `my` with
    // an extension, so it must not reach `my.sh`.
    let tasks = merge_file_and_config_tasks(
        vec![file_task("my.sh")],
        vec![Task {
            name: "my.app".to_string(),
            description: "unrelated".to_string(),
            ..Default::default()
        }],
    );

    let names = tasks.iter().map(|t| t.name.as_str()).sorted().collect_vec();
    assert_eq!(names, vec!["my.app", "my.sh"]);
}

#[test]
fn test_dependency_group_is_the_base_when_no_command_shares_its_name() {
    // Nothing below contributes a command, so the group is the base the
    // metadata-only block above it overlays. It used to be dropped, leaving
    // the name a task with no executable content.
    let tasks = merge_file_and_config_tasks(
        vec![],
        vec![
            Task {
                name: "hello".to_string(),
                description: "from local".to_string(),
                ..Default::default()
            },
            Task {
                name: "hello".to_string(),
                depends: vec!["lint".to_string().into()],
                ..Default::default()
            },
        ],
    );

    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].description, "from local");
    assert_eq!(
        tasks[0]
            .depends
            .iter()
            .map(|d| d.task.as_str())
            .collect_vec(),
        vec!["lint"]
    );
}

#[test]
fn test_dependency_group_stays_an_overlay_on_a_lower_precedence_command() {
    // The documented inline layering rule: a block with only `depends` on
    // top of a command-bearing block of the same name contributes its
    // dependency to that command rather than replacing it.
    let tasks = merge_file_and_config_tasks(
        vec![],
        vec![
            Task {
                name: "x".to_string(),
                depends: vec!["a".to_string().into()],
                ..Default::default()
            },
            Task {
                name: "x".to_string(),
                run: vec![RunEntry::Script("echo x".to_string())],
                ..Default::default()
            },
        ],
    );

    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].run, vec![RunEntry::Script("echo x".to_string())]);
    assert_eq!(
        tasks[0]
            .depends
            .iter()
            .map(|d| d.task.as_str())
            .collect_vec(),
        vec!["a"]
    );
}

#[test]
fn test_mixed_overlay_spellings_keep_the_higher_precedence_block() {
    // `[tasks.hello]` and `[tasks."hello.sh"]` reach the same file task
    // under different names, so the lower-precedence spelling must neither
    // overwrite the higher one nor survive as a task shadowing the script.
    for (high, low) in [("hello.sh", "hello"), ("hello", "hello.sh")] {
        let tasks = merge_file_and_config_tasks(
            vec![file_task("hello.sh")],
            vec![
                Task {
                    name: high.to_string(),
                    description: "high".to_string(),
                    ..Default::default()
                },
                Task {
                    name: low.to_string(),
                    description: "low".to_string(),
                    ..Default::default()
                },
            ],
        );

        assert_eq!(tasks.len(), 1, "{high} over {low}");
        assert_eq!(tasks[0].name, "hello.sh", "{high} over {low}");
        assert_eq!(tasks[0].description, "high", "{high} over {low}");
    }
}

#[test]
fn test_every_dependency_field_stays_an_overlay_on_a_lower_command() {
    // Guards the split between `metadata_only` and
    // `task_has_executable_content`. Folding them together drops the
    // command here, so each dependency field is checked, not just
    // `depends`.
    for field in ["depends", "depends_post", "wait_for"] {
        let mut overlay = Task {
            name: "x".to_string(),
            ..Default::default()
        };
        let dep = vec!["a".to_string().into()];
        match field {
            "depends" => overlay.depends = dep,
            "depends_post" => overlay.depends_post = dep,
            _ => overlay.wait_for = dep,
        }

        let tasks = merge_file_and_config_tasks(
            vec![],
            vec![
                overlay,
                Task {
                    name: "x".to_string(),
                    run: vec![RunEntry::Script("echo x".to_string())],
                    ..Default::default()
                },
            ],
        );

        assert_eq!(tasks.len(), 1, "{field}");
        assert_eq!(
            tasks[0].run,
            vec![RunEntry::Script("echo x".to_string())],
            "{field} dropped the command"
        );
        let kept = match field {
            "depends" => &tasks[0].depends,
            "depends_post" => &tasks[0].depends_post,
            _ => &tasks[0].wait_for,
        };
        assert_eq!(
            kept.iter().map(|d| d.task.as_str()).collect_vec(),
            vec!["a"],
            "{field} dropped the dependency"
        );
    }
}

#[test]
fn test_wait_for_alone_overlays_the_file_task() {
    // `wait_for` orders tasks something else already scheduled and adds
    // none of its own, so a block carrying only `wait_for` is metadata.
    // Treating it as executable content left it shadowing the script as a
    // task that runs nothing -- the failure this branch exists to remove.
    let tasks = merge_file_and_config_tasks(
        vec![file_task("hello.sh")],
        vec![Task {
            name: "hello".to_string(),
            wait_for: vec!["other".to_string().into()],
            ..Default::default()
        }],
    );

    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].name, "hello.sh");
    assert_eq!(tasks[0].file, Some(PathBuf::from("mise-tasks/hello.sh")));
    assert_eq!(
        tasks[0]
            .wait_for
            .iter()
            .map(|d| d.task.as_str())
            .collect_vec(),
        vec!["other"]
    );
}

/// The name a block uses must not change what it does: `[tasks.hello]` and
/// `[tasks."hello.sh"]` are two spellings of one script.
fn merge_under_both_spellings(mut block: Task) -> Vec<Task> {
    let mut out = vec![];
    for spelling in ["hello", "hello.sh"] {
        block.name = spelling.to_string();
        let tasks = merge_file_and_config_tasks(vec![file_task("hello.sh")], vec![block.clone()]);
        assert_eq!(
            tasks.len(),
            1,
            "[tasks.\"{spelling}\"] did not overlay the script"
        );
        assert_eq!(tasks[0].name, "hello.sh", "{spelling}");
        assert_eq!(
            tasks[0].file,
            Some(PathBuf::from("mise-tasks/hello.sh")),
            "{spelling} lost the script"
        );
        out.push(tasks[0].clone());
    }
    out
}

#[test]
fn test_both_spellings_overlay_a_dependency_onto_the_script() {
    for task in merge_under_both_spellings(Task {
        depends: vec!["lint".to_string().into()],
        ..Default::default()
    }) {
        assert_eq!(
            task.depends.iter().map(|d| d.task.as_str()).collect_vec(),
            vec!["lint"]
        );
    }
}

#[test]
fn test_both_spellings_overlay_depends_post_onto_the_script() {
    for task in merge_under_both_spellings(Task {
        depends_post: vec!["post".to_string().into()],
        ..Default::default()
    }) {
        assert_eq!(
            task.depends_post
                .iter()
                .map(|d| d.task.as_str())
                .collect_vec(),
            vec!["post"]
        );
    }
}

#[test]
fn test_both_spellings_overlay_metadata_onto_the_script() {
    for task in merge_under_both_spellings(Task {
        description: "overlaid".to_string(),
        ..Default::default()
    }) {
        assert_eq!(task.description, "overlaid");
    }
}

#[test]
fn test_only_the_highest_precedence_block_reaches_the_script() {
    // A script takes one definition, not an accumulation: lower-precedence
    // blocks contribute nothing, not even additive fields (#11103). Because
    // the two spellings name one script they compete for that single slot,
    // so this holds across a mix of them just as it does for a repeat of
    // one.
    for (high, low) in [
        ("hello", "hello.sh"),
        ("hello.sh", "hello"),
        ("hello", "hello"),
        ("hello.sh", "hello.sh"),
    ] {
        let tasks = merge_file_and_config_tasks(
            vec![file_task("hello.sh")],
            vec![
                Task {
                    name: high.to_string(),
                    description: "high".to_string(),
                    ..Default::default()
                },
                Task {
                    name: low.to_string(),
                    description: "low".to_string(),
                    depends: vec!["lint".to_string().into()],
                    ..Default::default()
                },
            ],
        );

        let label = format!("{high} over {low}");
        assert_eq!(tasks.len(), 1, "{label}");
        assert_eq!(tasks[0].name, "hello.sh", "{label}");
        assert_eq!(tasks[0].description, "high", "{label}");
        assert!(
            tasks[0].depends.is_empty(),
            "{label} let a lower-precedence block contribute"
        );
    }
}

#[test]
fn test_an_inline_command_elsewhere_keeps_the_name_for_itself() {
    // `[tasks.hello] run = ...` makes `hello` a task of its own, so a
    // metadata block of that name layers onto it rather than onto the
    // script -- and the script it names is claimed by that command.
    let tasks = merge_file_and_config_tasks(
        vec![file_task("hello.sh")],
        vec![
            Task {
                name: "hello".to_string(),
                description: "from local".to_string(),
                ..Default::default()
            },
            Task {
                name: "hello".to_string(),
                run: vec![RunEntry::Script("echo inline".to_string())],
                ..Default::default()
            },
        ],
    );

    let inline = tasks.iter().find(|t| t.name == "hello").unwrap();
    assert_eq!(inline.description, "from local");
    assert_eq!(
        inline.run,
        vec![RunEntry::Script("echo inline".to_string())]
    );
    assert!(
        !tasks.iter().any(|t| t.name == "hello.sh"),
        "the command claims the script its name reaches"
    );
}

#[test]
fn test_stripped_name_overlay_keeps_the_monorepo_path_prefix() {
    let tasks = merge_file_and_config_tasks(
        vec![file_task("//projects/my.app:build.sh")],
        vec![Task {
            name: "//projects/my.app:build".to_string(),
            description: "overlaid".to_string(),
            ..Default::default()
        }],
    );

    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].name, "//projects/my.app:build.sh");
    assert_eq!(tasks[0].description, "overlaid");
}

#[test]
fn test_inline_command_replaces_a_matching_file_task() {
    let tasks = merge_file_and_config_tasks(
        vec![file_task("hello.sh")],
        vec![inline_task("hello.sh", "echo inline")],
    );

    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].name, "hello.sh");
    assert_eq!(
        tasks[0].run,
        vec![RunEntry::Script("echo inline".to_string())]
    );
    assert_eq!(tasks[0].file, None);
}

#[test]
fn test_inline_file_key_replaces_a_matching_file_task() {
    let redirected = Task {
        file: Some(PathBuf::from("scripts/other.sh")),
        ..inline_overlay("hello.sh")
    };

    let tasks = merge_file_and_config_tasks(vec![file_task("hello.sh")], vec![redirected]);

    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].file, Some(PathBuf::from("scripts/other.sh")));
}

/// A script is in the running because some config's `task_config.includes`
/// named its directory, so a block from a config below that one decorates
/// the script instead of taking it over.
#[test]
fn test_a_lower_precedence_command_still_only_overlays_a_file_task() {
    let script = Task {
        config_precedence: 0,
        ..file_task("hello.sh")
    };
    let block = Task {
        config_precedence: 1,
        description: "from a lower config".to_string(),
        ..inline_task("hello.sh", "echo inline")
    };

    let tasks = merge_file_and_config_tasks(vec![script], vec![block]);

    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].file, Some(PathBuf::from("mise-tasks/hello.sh")));
    assert_eq!(tasks[0].description, "from a lower config");
}

/// A command under the stem claims the scripts that stem names, so the two
/// spellings do the same thing. `mise tasks ls` shows the stem and
/// `mise run` takes it, so a script left beside the command would be
/// shadowed by exact-name matching anyway (#10393).
#[test]
fn test_a_command_under_the_stem_claims_the_scripts_it_names() {
    use crate::task::GetMatchingExt;

    let tasks = merge_file_and_config_tasks(
        vec![file_task("hello.sh")],
        vec![inline_task("hello", "echo inline")],
    )
    .into_iter()
    .map(|task| (task.name.clone(), task))
    .collect::<BTreeMap<_, _>>();

    assert_eq!(tasks.keys().collect_vec(), vec!["hello"]);

    let matches = tasks.get_matching("hello").unwrap();

    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].run, vec![RunEntry::Script("echo inline".into())]);
}

/// A stem names every script sharing it, and a command may have claimed
/// only some. A block written with that stem still configures the rest, as
/// well as the task the claimed one became.
#[test]
fn test_a_stem_block_still_reaches_the_siblings_a_command_left() {
    let tasks = merge_file_and_config_tasks(
        vec![file_task("hello.sh"), file_task("hello.js")],
        vec![
            Task {
                description: "stem metadata".to_string(),
                config_precedence: 0,
                ..inline_overlay("hello")
            },
            Task {
                config_precedence: 1,
                ..inline_task("hello.sh", "echo inline")
            },
        ],
    )
    .into_iter()
    .map(|task| (task.name.clone(), task))
    .collect::<BTreeMap<_, _>>();

    assert_eq!(tasks.keys().collect_vec(), vec!["hello.js", "hello.sh"]);
    assert_eq!(
        tasks["hello.sh"].description, "stem metadata",
        "the claimant should have the stem block's metadata"
    );
    assert_eq!(
        tasks["hello.js"].description, "stem metadata",
        "the sibling the command left should have it too"
    );
    assert_eq!(
        tasks["hello.js"].file,
        Some(PathBuf::from("mise-tasks/hello.js"))
    );
}

/// A command that loses the slot claims nothing. Letting it claim a script
/// the winner's name never reached would take that script away and leave
/// nothing running it, since the block itself is then dropped.
#[test]
fn test_a_command_that_loses_the_slot_leaves_sibling_scripts_alone() {
    let tasks = merge_file_and_config_tasks(
        vec![file_task("hello.sh"), file_task("hello.js")],
        vec![
            Task {
                config_precedence: 0,
                ..inline_task("hello.sh", "echo exact")
            },
            Task {
                config_precedence: 1,
                ..inline_task("hello", "echo stem")
            },
        ],
    );

    assert_eq!(
        tasks.iter().map(|t| t.name.as_str()).sorted().collect_vec(),
        vec!["hello.js", "hello.sh"],
        "the sibling the winning name never reached must survive"
    );
    let claimed = tasks.iter().find(|t| t.name == "hello.sh").unwrap();
    assert_eq!(
        claimed.run,
        vec![RunEntry::Script("echo exact".to_string())]
    );
    let sibling = tasks.iter().find(|t| t.name == "hello.js").unwrap();
    assert_eq!(sibling.file, Some(PathBuf::from("mise-tasks/hello.js")));
}

/// A command that cannot outrank the scripts its stem reaches decorates
/// them, exactly as one written with a script's full name does. Left as a
/// task of its own it would shadow them under `mise run <stem>`, which is
/// the takeover the precedence rule just refused.
#[test]
fn test_a_refused_stem_command_decorates_instead_of_shadowing() {
    let script = Task {
        config_precedence: 0,
        ..file_task("hello.sh")
    };
    let block = Task {
        config_precedence: 1,
        description: "from a lower config".to_string(),
        ..inline_task("hello", "echo inline")
    };

    let tasks = merge_file_and_config_tasks(vec![script], vec![block]);

    assert_eq!(tasks.len(), 1, "the command stood up a task of its own");
    assert_eq!(tasks[0].name, "hello.sh");
    assert_eq!(tasks[0].file, Some(PathBuf::from("mise-tasks/hello.sh")));
    assert_eq!(tasks[0].description, "from a lower config");
}

/// Two spellings of one script are one definition, so a second command
/// under the other spelling loses the slot rather than standing up a task
/// beside the one that claimed the script (#11103).
#[test]
fn test_a_second_command_under_the_other_spelling_loses_the_slot() {
    for (high, low) in [("hello", "hello.sh"), ("hello.sh", "hello")] {
        let tasks = merge_file_and_config_tasks(
            vec![file_task("hello.sh")],
            vec![
                Task {
                    config_precedence: 0,
                    ..inline_task(high, "echo high")
                },
                Task {
                    config_precedence: 1,
                    ..inline_task(low, "echo low")
                },
            ],
        );

        assert_eq!(tasks.len(), 1, "[tasks.{low}] stood up a task of its own");
        assert_eq!(
            tasks[0].run,
            vec![RunEntry::Script("echo high".to_string())],
            "[tasks.{high}] should have kept the slot"
        );
    }
}

/// The stem can name more than one script, and the command speaks for all
/// of them. The full name stays the narrower claim.
#[test]
fn test_a_command_under_the_stem_claims_every_script_sharing_it() {
    let scripts = || vec![file_task("hello.sh"), file_task("hello.js")];

    let under_stem =
        merge_file_and_config_tasks(scripts(), vec![inline_task("hello", "echo inline")]);
    assert_eq!(
        under_stem.iter().map(|t| t.name.as_str()).collect_vec(),
        vec!["hello"],
        "the stem claims both scripts"
    );

    let under_full_name =
        merge_file_and_config_tasks(scripts(), vec![inline_task("hello.sh", "echo inline")]);
    assert_eq!(
        under_full_name
            .iter()
            .map(|t| t.name.as_str())
            .sorted()
            .collect_vec(),
        vec!["hello.js", "hello.sh"],
        "the full name claims only the script it spells"
    );
}

/// On Windows a `.ps1` paired with a POSIX sibling is renamed to the bare
/// stem, so the block that replaces it is the one spelled with that stem.
#[test]
fn test_inline_command_replaces_a_renamed_windows_file_task() {
    let file_tasks = prefer_windows_file_task_siblings_inner(vec![
        file_task("hello.sh"),
        file_task("hello.ps1"),
    ]);

    let tasks = merge_file_and_config_tasks(file_tasks, vec![inline_task("hello", "echo inline")]);

    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].name, "hello");
    assert_eq!(
        tasks[0].run,
        vec![RunEntry::Script("echo inline".to_string())]
    );
}

#[test]
fn test_prefer_windows_file_task_siblings_keeps_exact_stem_for_matching() {
    use crate::task::GetMatchingExt;

    let file_tasks = vec![
        Task {
            name: "hello".to_string(),
            config_source: PathBuf::from("mise-tasks/hello"),
            ..Default::default()
        },
        Task {
            name: "hello.ps1".to_string(),
            config_source: PathBuf::from("mise-tasks/hello.ps1"),
            ..Default::default()
        },
        Task {
            name: "hello.sh".to_string(),
            config_source: PathBuf::from("mise-tasks/hello.sh"),
            ..Default::default()
        },
    ];
    let tasks = prefer_windows_file_task_siblings_inner(file_tasks)
        .into_iter()
        .map(|task| (task.name.clone(), task))
        .collect::<BTreeMap<_, _>>();

    let matches = tasks.get_matching("hello").unwrap();

    assert_eq!(matches.len(), 1);
    assert_eq!(
        matches[0].config_source,
        PathBuf::from("mise-tasks/hello.ps1")
    );
}

#[test]
fn test_prefer_windows_file_task_siblings_keeps_ambiguous_windows_siblings() {
    let file_tasks = vec![
        Task {
            name: "hello".to_string(),
            config_source: PathBuf::from("mise-tasks/hello"),
            ..Default::default()
        },
        Task {
            name: "hello.ps1".to_string(),
            config_source: PathBuf::from("mise-tasks/hello.ps1"),
            ..Default::default()
        },
        Task {
            name: "hello.cmd".to_string(),
            config_source: PathBuf::from("mise-tasks/hello.cmd"),
            ..Default::default()
        },
    ];

    let tasks = prefer_windows_file_task_siblings_inner(file_tasks);
    let task_names = tasks.iter().map(|task| task.name.as_str()).collect_vec();
    let task_sources = tasks
        .iter()
        .map(|task| task.config_source.as_path())
        .collect_vec();

    assert_eq!(task_names, vec!["hello", "hello.ps1", "hello.cmd"]);
    assert_eq!(
        task_sources,
        vec![
            Path::new("mise-tasks/hello"),
            Path::new("mise-tasks/hello.ps1"),
            Path::new("mise-tasks/hello.cmd"),
        ]
    );
}

#[test]
fn test_prefer_windows_file_task_siblings_scopes_to_source_family() {
    let file_tasks = vec![
        Task {
            name: "build".to_string(),
            config_source: PathBuf::from("included-tasks/build"),
            ..Default::default()
        },
        Task {
            name: "build.ps1".to_string(),
            config_source: PathBuf::from("mise-tasks/build.ps1"),
            ..Default::default()
        },
    ];

    let tasks = prefer_windows_file_task_siblings_inner(file_tasks);
    let task_names = tasks.iter().map(|task| task.name.as_str()).collect_vec();
    let task_sources = tasks
        .iter()
        .map(|task| task.config_source.as_path())
        .collect_vec();

    assert_eq!(task_names, vec!["build", "build.ps1"]);
    assert_eq!(
        task_sources,
        vec![
            Path::new("included-tasks/build"),
            Path::new("mise-tasks/build.ps1"),
        ]
    );
}

/// The case this function exists for.
///
/// `name_from_path` keeps the extension, so these arrive as `build.sh` and `build.ps1`; both
/// reduce to `build` once the extension is stripped, which is why they collide.
#[test]
fn test_prefer_windows_file_task_siblings_takes_over_a_posix_extension() {
    let file_tasks = vec![
        Task {
            name: "build.sh".to_string(),
            config_source: PathBuf::from("mise-tasks/build.sh"),
            ..Default::default()
        },
        Task {
            name: "build.ps1".to_string(),
            config_source: PathBuf::from("mise-tasks/build.ps1"),
            ..Default::default()
        },
    ];

    let tasks = prefer_windows_file_task_siblings_inner(file_tasks);

    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].name, "build");
    assert_eq!(
        tasks[0].config_source,
        PathBuf::from("mise-tasks/build.ps1")
    );
}

/// A dot inside the task name is not the extension.
///
/// `build.release.sh` is named `build.release.sh`, and `strip_task_extension` takes off only
/// the last extension — so the pair keys as `build.release` and the survivor is renamed to
/// `build.release`, not to `build`. Getting that wrong would leave `mise run build.release`
/// with nothing to resolve.
#[test]
fn test_prefer_windows_file_task_siblings_keeps_a_dotted_task_name() {
    let file_tasks = vec![
        Task {
            name: "build.release.sh".to_string(),
            config_source: PathBuf::from("mise-tasks/build.release.sh"),
            ..Default::default()
        },
        Task {
            name: "build.release.ps1".to_string(),
            config_source: PathBuf::from("mise-tasks/build.release.ps1"),
            ..Default::default()
        },
    ];

    let tasks = prefer_windows_file_task_siblings_inner(file_tasks);

    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].name, "build.release");
    assert_eq!(
        tasks[0].config_source,
        PathBuf::from("mise-tasks/build.release.ps1")
    );
}

/// A control for the test above: taking over requires something to take over *from*. A `.sh`
/// with no Windows sibling is the common shape on a project that never had a Windows script,
/// and it has to keep working — dropping it would break every such project on Windows.
#[test]
fn test_prefer_windows_file_task_siblings_keeps_a_lone_posix_extension() {
    let file_tasks = vec![Task {
        name: "build.sh".to_string(),
        config_source: PathBuf::from("mise-tasks/build.sh"),
        ..Default::default()
    }];

    let tasks = prefer_windows_file_task_siblings_inner(file_tasks);

    assert_eq!(tasks.len(), 1);
    // Name untouched as well as kept: renaming it to `build` here would invent a task the
    // POSIX-only project never had.
    assert_eq!(tasks[0].name, "build.sh");
    assert_eq!(tasks[0].config_source, PathBuf::from("mise-tasks/build.sh"));
}

/// The other control: two POSIX files are not a platform pair, so neither is Windows' to
/// prefer. Widening the POSIX side must not turn "same stem" alone into a reason to delete.
#[test]
fn test_prefer_windows_file_task_siblings_keeps_two_posix_files() {
    let file_tasks = vec![
        Task {
            name: "build".to_string(),
            config_source: PathBuf::from("mise-tasks/build"),
            ..Default::default()
        },
        Task {
            name: "build.sh".to_string(),
            config_source: PathBuf::from("mise-tasks/build.sh"),
            ..Default::default()
        },
    ];

    let tasks = prefer_windows_file_task_siblings_inner(file_tasks);

    assert_eq!(tasks.len(), 2);
}

/// Ambiguity is unchanged by the widening: with two Windows siblings there is no basis for
/// choosing, so the `.sh` survives alongside them rather than being deleted for nothing.
#[test]
fn test_prefer_windows_file_task_siblings_keeps_posix_when_windows_is_ambiguous() {
    let file_tasks = vec![
        Task {
            name: "build.sh".to_string(),
            config_source: PathBuf::from("mise-tasks/build.sh"),
            ..Default::default()
        },
        Task {
            name: "build.ps1".to_string(),
            config_source: PathBuf::from("mise-tasks/build.ps1"),
            ..Default::default()
        },
        Task {
            name: "build.cmd".to_string(),
            config_source: PathBuf::from("mise-tasks/build.cmd"),
            ..Default::default()
        },
    ];

    let tasks = prefer_windows_file_task_siblings_inner(file_tasks);

    assert_eq!(tasks.len(), 3);
    // Names untouched too: nothing was chosen, so nothing should have been renamed.
    assert_eq!(
        tasks.iter().map(|task| task.name.as_str()).collect_vec(),
        vec!["build.sh", "build.ps1", "build.cmd"]
    );
}

#[test]
fn test_env_config_patterns() {
    let patterns = |local| env_config_patterns_with_conf_d("linux", local, true);
    assert_eq!(
        patterns(false)
            .into_iter()
            .chain(patterns(true))
            .collect_vec(),
        vec![
            ".config/mise/conf.d/*.linux.toml",
            ".config/mise/conf.d/*/mise.linux.toml",
            ".config/mise/config.linux.toml",
            ".config/mise.linux.toml",
            "mise/conf.d/*.linux.toml",
            "mise/conf.d/*/mise.linux.toml",
            "mise/config.linux.toml",
            "mise.linux.toml",
            ".mise/conf.d/*.linux.toml",
            ".mise/conf.d/*/mise.linux.toml",
            ".mise/config.linux.toml",
            ".mise.linux.toml",
            ".config/mise/conf.d/*.linux.local.toml",
            ".config/mise/conf.d/*/mise.linux.local.toml",
            ".config/mise/config.linux.local.toml",
            ".config/mise.linux.local.toml",
            "mise/conf.d/*.linux.local.toml",
            "mise/conf.d/*/mise.linux.local.toml",
            "mise/config.linux.local.toml",
            "mise.linux.local.toml",
            ".mise/conf.d/*.linux.local.toml",
            ".mise/conf.d/*/mise.linux.local.toml",
            ".mise/config.linux.local.toml",
            ".mise.linux.local.toml",
        ]
    );
    // Folder fragments are new, so their environment files are never
    // subject to the env_conf_d migration.
    assert!(
        [false, true]
            .into_iter()
            .flat_map(|local| env_config_patterns_with_conf_d("linux", local, false))
            .collect_vec()
            .iter()
            .filter(|pattern| pattern.contains("conf.d"))
            .all(|pattern| pattern.contains("conf.d/*/mise."))
    );
}

#[test]
fn test_glob_skips_patterns_under_missing_directories() -> Result<()> {
    let missing = TempDir::new()?;
    assert!(glob(missing.path(), ".config/mise/conf.d/*.toml")?.is_empty());
    assert!(glob(missing.path(), ".mise/conf.d/*/mise.toml")?.is_empty());

    // glob caches by directory and pattern, so the matching case needs a
    // directory of its own.
    let tmp = TempDir::new()?;
    let confd = tmp.path().join(".mise/conf.d");
    fs::create_dir_all(confd.join("node"))?;
    fs::write(confd.join("tools.toml"), "")?;
    fs::write(confd.join("node/mise.toml"), "")?;
    assert_eq!(
        glob(tmp.path(), ".mise/conf.d/*.toml")?,
        vec![confd.join("tools.toml")]
    );
    assert_eq!(
        glob(tmp.path(), ".mise/conf.d/*/mise.toml")?,
        vec![confd.join("node/mise.toml")]
    );
    // A malformed pattern is an error even when its directory is missing.
    assert!(glob(missing.path(), "missing/[unclosed/*.toml").is_err());
    // A pattern with no wildcard names a file, not a directory to check.
    fs::write(tmp.path().join("mise.toml"), "")?;
    assert_eq!(
        glob(tmp.path(), "mise.toml")?,
        vec![tmp.path().join("mise.toml")]
    );
    Ok(())
}

#[test]
fn test_env_config_patterns_escape_glob_characters() -> Result<()> {
    let tmp = TempDir::new()?;
    let confd = tmp.path().join(".mise/conf.d");
    fs::create_dir_all(&confd)?;
    fs::write(confd.join("tools.qa*.toml"), "[env]\n")?;
    fs::write(confd.join("tools.qa1.toml"), "[env]\n")?;

    let pattern = env_config_patterns_with_conf_d("qa*", false, true)
        .into_iter()
        .find(|pattern| pattern.starts_with(".mise/conf.d/"))
        .unwrap();
    let matches = glob(tmp.path(), &pattern)?;
    assert_eq!(matches, vec![confd.join("tools.qa*.toml")]);

    Ok(())
}

#[test]
fn test_env_config_patterns_with_non_star_glob_characters() -> Result<()> {
    for env_name in ["qa?", "qa[1]", "qa]"] {
        let tmp = TempDir::new()?;
        let path = tmp.path().join(format!("mise.{env_name}.toml"));
        fs::write(&path, "[env]\n")?;

        let patterns = env_config_patterns(env_name, false);
        assert!(config_paths_in_dir_with_filenames(tmp.path(), &patterns).contains(&path));
        assert!(has_config_file_with_filenames(tmp.path(), &patterns));
    }

    Ok(())
}

#[test]
fn test_config_filenames_rank_local_above_environment() -> Result<()> {
    let tmp = TempDir::new()?;
    for name in [
        "mise.toml",
        "mise.test.toml",
        "mise.local.toml",
        "mise.test.local.toml",
        ".config/mise/config.toml",
        ".config/mise/config.test.toml",
        ".config/mise/config.local.toml",
        ".config/mise/config.test.local.toml",
    ] {
        let path = tmp.path().join(name);
        fs::create_dir_all(path.parent().unwrap())?;
        fs::write(path, "")?;
    }
    // Unit tests override the default filenames, so pass the relevant ones.
    let filenames = layer_config_filenames(
        [".config/mise/config.toml", "mise.toml"],
        [".config/mise/config.local.toml", "mise.local.toml"],
        &["test".to_string()],
    );
    // Highest precedence first.
    assert_eq!(
        config_paths_in_dir_with_filenames(tmp.path(), &filenames)
            .iter()
            .map(|path| path.strip_prefix(tmp.path()).unwrap().to_string_lossy())
            .collect_vec(),
        vec![
            "mise.test.local.toml",
            ".config/mise/config.test.local.toml",
            "mise.local.toml",
            ".config/mise/config.local.toml",
            "mise.test.toml",
            ".config/mise/config.test.toml",
            "mise.toml",
            ".config/mise/config.toml",
        ]
    );
    Ok(())
}

#[test]
fn test_should_warn_auto_env() {
    let v = |s: &str| versions::Versioning::new(s).unwrap();
    // before the warning window: never warn
    assert!(!should_warn_auto_env(&v("2026.6.2"), None, false));
    // inside the warning window: warn only when the user hasn't decided
    assert!(should_warn_auto_env(&v("2026.12.0"), None, false));
    assert!(should_warn_auto_env(&v("2027.5.9"), None, false));
    assert!(!should_warn_auto_env(&v("2026.12.0"), Some(false), false));
    assert!(!should_warn_auto_env(&v("2026.12.0"), Some(true), true));
    // auto envs already active (e.g. opted in): nothing to warn about
    assert!(!should_warn_auto_env(&v("2026.12.0"), None, true));
    // default flipped on: warning is obsolete
    assert!(!should_warn_auto_env(&v("2027.6.0"), None, true));
    assert!(!should_warn_auto_env(&v("2027.6.0"), Some(false), false));
}

#[test]
fn test_monorepo_lockfile_rollout() {
    let v = |s: &str| versions::Versioning::new(s).unwrap();

    assert!(!monorepo_lockfile_enabled_for_version(
        &v("2026.6.15"),
        None
    ));
    assert!(monorepo_lockfile_enabled_for_version(
        &v("2026.6.15"),
        Some(true)
    ));
    assert!(!monorepo_lockfile_enabled_for_version(
        &v("2027.6.0"),
        Some(false)
    ));
    assert!(monorepo_lockfile_enabled_for_version(&v("2027.6.0"), None));

    assert!(!should_warn_monorepo_lockfile_default(
        &v("2026.11.9"),
        None,
        true,
        true
    ));
    assert!(should_warn_monorepo_lockfile_default(
        &v("2026.12.0"),
        None,
        true,
        true
    ));
    assert!(should_warn_monorepo_lockfile_default(
        &v("2027.5.9"),
        None,
        true,
        true
    ));
    assert!(!should_warn_monorepo_lockfile_default(
        &v("2026.12.0"),
        None,
        false,
        true
    ));
    assert!(!should_warn_monorepo_lockfile_default(
        &v("2026.12.0"),
        None,
        true,
        false
    ));
    assert!(!should_warn_monorepo_lockfile_default(
        &v("2026.12.0"),
        Some(true),
        true,
        true
    ));
    assert!(!should_warn_monorepo_lockfile_default(
        &v("2026.12.0"),
        Some(false),
        true,
        true
    ));
    assert!(!should_warn_monorepo_lockfile_default(
        &v("2027.6.0"),
        None,
        true,
        true
    ));
}

#[tokio::test]
async fn test_get_tool_opts_with_overrides_keeps_inline_opts_with_config_entry() -> Result<()> {
    crate::toolset::install_state::init().await?;

    let source = crate::toolset::ToolSource::MiseToml(PathBuf::from("mise.toml"));
    let resolved_ba = Arc::new(BackendArg::from("github:jdx/mise-test-fixtures"));
    let config_opts =
        crate::toolset::parse_tool_options("api_url=https://config.example/api/v3,foo=config");
    let mut trs = ToolRequestSet::new();
    trs.add_version(
        crate::toolset::ToolRequest::new_with_options(
            resolved_ba,
            "1.0.0",
            config_opts,
            source.clone(),
        )?,
        &source,
    );

    let mut repo_urls = HashMap::new();
    repo_urls.insert(
        "tiny".to_string(),
        "github:jdx/mise-test-fixtures".to_string(),
    );
    let config = Config {
        tera_ctx: BASE_CONTEXT.clone(),
        config_files: Default::default(),
        bootstrap_config_maps: vec![],
        bootstrap_dry_run_unresolved_vars: Default::default(),
        env: OnceCell::new(),
        env_with_sources: OnceCell::new(),
        shorthands: get_shorthands(&Settings::get()),
        hooks: OnceCell::new(),
        tasks_cache: Arc::new(DashMap::new()),
        workspace_project_graph_cache: Mutex::new(None),
        daemons: Default::default(),
        tool_request_set: OnceCell::new(),
        toolset: OnceCell::new(),
        all_aliases: Default::default(),
        aliases: Default::default(),
        project_root: Default::default(),
        repo_urls,
        shell_aliases: Default::default(),
        tera_files: Default::default(),
        vars: Default::default(),
        vars_results: OnceCell::new(),
        lockfile_discovery: Default::default(),
    };
    config.tool_request_set.set(trs).ok();
    let config = Arc::new(config);
    let ba = Arc::new(BackendArg::new_raw(
        "tiny".to_string(),
        Some("github:jdx/mise-test-fixtures".to_string()),
        "jdx/mise-test-fixtures".to_string(),
        Some(crate::toolset::parse_tool_options(
            "api_url=https://inline.example/api/v3",
        )),
        crate::args::BackendResolution::new(true),
    ));

    let opts = config.get_tool_opts_with_overrides(&ba).await?;

    assert_eq!(opts.get("api_url"), Some("https://inline.example/api/v3"));
    assert_eq!(opts.get("foo"), Some("config"));
    Ok(())
}

#[tokio::test]
async fn test_get_tool_opts_with_overrides_keeps_inline_opts_without_config_entry() -> Result<()> {
    let config = Config::reset().await?;
    let ba = Arc::new(BackendArg::from(
        "tiny[api_url=https://inline.example/api/v3]",
    ));

    let opts = config.get_tool_opts_with_overrides(&ba).await?;

    assert_eq!(opts.get("api_url"), Some("https://inline.example/api/v3"));
    Ok(())
}

#[tokio::test]
async fn pipx_aliases_keep_their_tool_spelling() -> Result<()> {
    crate::backend::load_tools().await?;
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("mise.toml");
    fs::write(
        &path,
        r#"
[alias."pipx:black"]
versions = { stable = "24.10.0" }
[alias.formatter]
backend = "pipx:black"
"#,
    )?;
    let mut files: ConfigMap = Default::default();
    files.insert(
        path.clone(),
        Arc::new(config_file::mise_toml::MiseToml::from_file(&path)?),
    );
    let all_aliases = load_aliases(&files)?;
    assert_eq!(
        all_aliases["formatter"].backend.as_deref(),
        Some("pipx:black")
    );
    let config = Config {
        tera_ctx: BASE_CONTEXT.clone(),
        config_files: Default::default(),
        bootstrap_config_maps: vec![],
        bootstrap_dry_run_unresolved_vars: Default::default(),
        env: OnceCell::new(),
        env_with_sources: OnceCell::new(),
        shorthands: get_shorthands(&Settings::get()),
        hooks: OnceCell::new(),
        tasks_cache: Arc::new(DashMap::new()),
        workspace_project_graph_cache: Mutex::new(None),
        daemons: Default::default(),
        tool_request_set: OnceCell::new(),
        toolset: OnceCell::new(),
        all_aliases,
        aliases: Default::default(),
        project_root: Default::default(),
        repo_urls: Default::default(),
        shell_aliases: Default::default(),
        tera_files: Default::default(),
        vars: Default::default(),
        vars_results: OnceCell::new(),
        lockfile_discovery: Default::default(),
    };
    let legacy = crate::backend::get(&BackendArg::from("pipx:black")).unwrap();
    let preferred = crate::backend::get(&BackendArg::from("pypi:black")).unwrap();
    assert_eq!(config.resolve_alias(&legacy, "stable").await?, "24.10.0");
    assert_eq!(config.resolve_alias(&preferred, "stable").await?, "stable");
    Ok(())
}

#[tokio::test]
async fn test_resolve_tool_opts_tracks_alias_config_and_inline_sources() -> Result<()> {
    crate::toolset::install_state::init().await?;

    let source = crate::toolset::ToolSource::MiseToml(PathBuf::from("mise.toml"));
    let config_ba = Arc::new(BackendArg::from("tiny"));
    let config_opts = crate::toolset::parse_tool_options("asset_pattern=config-pattern,bar=config");
    let mut trs = ToolRequestSet::new();
    trs.add_version(
        crate::toolset::ToolRequest::new_with_options(
            config_ba,
            "1.0.0",
            config_opts,
            source.clone(),
        )?,
        &source,
    );

    let mut all_aliases = AliasMap::default();
    all_aliases.insert(
        "tiny".to_string(),
        Alias {
            backend: Some(
                "github:jdx/mise-test-fixtures[api_url=https://alias.example/api/v3,asset_pattern=alias-pattern,foo=alias]"
                    .to_string(),
            ),
            versions: Default::default(),
        },
    );
    let config = Config {
        tera_ctx: BASE_CONTEXT.clone(),
        config_files: Default::default(),
        bootstrap_config_maps: vec![],
        bootstrap_dry_run_unresolved_vars: Default::default(),
        env: OnceCell::new(),
        env_with_sources: OnceCell::new(),
        shorthands: get_shorthands(&Settings::get()),
        hooks: OnceCell::new(),
        tasks_cache: Arc::new(DashMap::new()),
        workspace_project_graph_cache: Mutex::new(None),
        daemons: Default::default(),
        tool_request_set: OnceCell::new(),
        toolset: OnceCell::new(),
        all_aliases,
        aliases: Default::default(),
        project_root: Default::default(),
        repo_urls: Default::default(),
        shell_aliases: Default::default(),
        tera_files: Default::default(),
        vars: Default::default(),
        vars_results: OnceCell::new(),
        lockfile_discovery: Default::default(),
    };
    config.tool_request_set.set(trs).ok();
    let config = Arc::new(config);
    let ba = Arc::new(BackendArg::from(
        "tiny[api_url=https://inline.example/api/v3]",
    ));

    let resolved = config.resolve_tool_opts_with_overrides(&ba).await?;
    let opts = resolved.effective();

    assert_eq!(opts.get("api_url"), Some("https://inline.example/api/v3"));
    assert_eq!(opts.get("asset_pattern"), Some("config-pattern"));
    assert_eq!(opts.get("foo"), Some("alias"));
    assert_eq!(opts.get("bar"), Some("config"));
    assert_eq!(
        resolved.source_for_key("api_url"),
        Some(crate::toolset::ToolOptionSource::InlineBackendArg)
    );
    assert_eq!(
        resolved.source_for_key("asset_pattern"),
        Some(crate::toolset::ToolOptionSource::Config)
    );
    assert_eq!(
        resolved.source_for_key("foo"),
        Some(crate::toolset::ToolOptionSource::BackendAlias)
    );
    Ok(())
}

#[tokio::test]
async fn test_resolve_tool_opts_prefers_config_over_install_manifest_opts() -> Result<()> {
    crate::toolset::install_state::init().await?;

    let source = crate::toolset::ToolSource::MiseToml(PathBuf::from("mise.toml"));
    let config_ba = Arc::new(BackendArg::from("http:manifest-opts"));
    let config_opts =
        crate::toolset::parse_tool_options("version_json_path=.current,config_only=true");
    let mut trs = ToolRequestSet::new();
    trs.add_version(
        crate::toolset::ToolRequest::new_with_options(
            config_ba,
            "1.0.0",
            config_opts,
            source.clone(),
        )?,
        &source,
    );

    let mut manifest_opts = BTreeMap::new();
    manifest_opts.insert(
        "version_json_path".to_string(),
        toml::Value::String(".manifest".to_string()),
    );
    manifest_opts.insert(
        "manifest_only".to_string(),
        toml::Value::String("true".to_string()),
    );
    let ba = Arc::new(BackendArg::from(
        crate::toolset::install_state::InstallStateTool {
            short: "http:manifest-opts".to_string(),
            full: Some("http:manifest-opts".to_string()),
            versions: vec!["1.0.0".to_string()],
            explicit_backend: true,
            opts: manifest_opts,
            installs_path: None,
        },
    ));

    let config = Config {
        tera_ctx: BASE_CONTEXT.clone(),
        config_files: Default::default(),
        bootstrap_config_maps: vec![],
        bootstrap_dry_run_unresolved_vars: Default::default(),
        env: OnceCell::new(),
        env_with_sources: OnceCell::new(),
        shorthands: get_shorthands(&Settings::get()),
        hooks: OnceCell::new(),
        tasks_cache: Arc::new(DashMap::new()),
        workspace_project_graph_cache: Mutex::new(None),
        daemons: Default::default(),
        tool_request_set: OnceCell::new(),
        toolset: OnceCell::new(),
        all_aliases: Default::default(),
        aliases: Default::default(),
        project_root: Default::default(),
        repo_urls: Default::default(),
        shell_aliases: Default::default(),
        tera_files: Default::default(),
        vars: Default::default(),
        vars_results: OnceCell::new(),
        lockfile_discovery: Default::default(),
    };
    config.tool_request_set.set(trs).ok();
    let config = Arc::new(config);

    let resolved = config.resolve_tool_opts_with_overrides(&ba).await?;
    let opts = resolved.effective();

    assert_eq!(opts.get("version_json_path"), Some(".current"));
    assert_eq!(
        resolved.source_for_key("version_json_path"),
        Some(crate::toolset::ToolOptionSource::Config)
    );
    assert_eq!(opts.get("manifest_only"), Some("true"));
    assert_eq!(
        resolved.source_for_key("manifest_only"),
        Some(crate::toolset::ToolOptionSource::InstallManifest)
    );
    assert_eq!(opts.get("config_only"), Some("true"));
    assert_eq!(
        resolved.source_for_key("config_only"),
        Some(crate::toolset::ToolOptionSource::Config)
    );
    Ok(())
}

#[tokio::test]
async fn test_resolve_tool_opts_prefers_s3_listing_config_over_install_manifest_opts() -> Result<()>
{
    crate::toolset::install_state::init().await?;

    let source = crate::toolset::ToolSource::MiseToml(PathBuf::from("mise.toml"));
    let config_ba = Arc::new(BackendArg::from("s3:manifest-opts"));
    let config_opts =
        crate::toolset::parse_tool_options("version_prefix=current/,version_regex=current-(.*)");
    let mut trs = ToolRequestSet::new();
    trs.add_version(
        crate::toolset::ToolRequest::new_with_options(
            config_ba,
            "1.0.0",
            config_opts,
            source.clone(),
        )?,
        &source,
    );

    let mut manifest_opts = BTreeMap::new();
    manifest_opts.insert(
        "version_prefix".to_string(),
        toml::Value::String("manifest/".to_string()),
    );
    manifest_opts.insert(
        "version_regex".to_string(),
        toml::Value::String("manifest-(.*)".to_string()),
    );
    manifest_opts.insert(
        "endpoint".to_string(),
        toml::Value::String("https://manifest.example".to_string()),
    );
    let ba = Arc::new(BackendArg::from(
        crate::toolset::install_state::InstallStateTool {
            short: "s3:manifest-opts".to_string(),
            full: Some("s3:manifest-opts".to_string()),
            versions: vec!["1.0.0".to_string()],
            explicit_backend: true,
            opts: manifest_opts,
            installs_path: None,
        },
    ));

    let config = Config {
        tera_ctx: BASE_CONTEXT.clone(),
        config_files: Default::default(),
        bootstrap_config_maps: vec![],
        bootstrap_dry_run_unresolved_vars: Default::default(),
        env: OnceCell::new(),
        env_with_sources: OnceCell::new(),
        shorthands: get_shorthands(&Settings::get()),
        hooks: OnceCell::new(),
        tasks_cache: Arc::new(DashMap::new()),
        workspace_project_graph_cache: Mutex::new(None),
        daemons: Default::default(),
        tool_request_set: OnceCell::new(),
        toolset: OnceCell::new(),
        all_aliases: Default::default(),
        aliases: Default::default(),
        project_root: Default::default(),
        repo_urls: Default::default(),
        shell_aliases: Default::default(),
        tera_files: Default::default(),
        vars: Default::default(),
        vars_results: OnceCell::new(),
        lockfile_discovery: Default::default(),
    };
    config.tool_request_set.set(trs).ok();
    let config = Arc::new(config);

    let resolved = config.resolve_tool_opts_with_overrides(&ba).await?;
    let opts = resolved.effective();

    assert_eq!(opts.get("version_prefix"), Some("current/"));
    assert_eq!(
        resolved.source_for_key("version_prefix"),
        Some(crate::toolset::ToolOptionSource::Config)
    );
    assert_eq!(opts.get("version_regex"), Some("current-(.*)"));
    assert_eq!(
        resolved.source_for_key("version_regex"),
        Some(crate::toolset::ToolOptionSource::Config)
    );
    assert_eq!(opts.get("endpoint"), Some("https://manifest.example"));
    assert_eq!(
        resolved.source_for_key("endpoint"),
        Some(crate::toolset::ToolOptionSource::InstallManifest)
    );
    Ok(())
}

#[tokio::test]
async fn test_resolve_tool_opts_prefers_env_backend_override_over_alias_opts() -> Result<()> {
    unsafe {
        std::env::set_var("MISE_BACKENDS_ENV_OPTS_TEST", "github:env/repo[foo=env]");
    }

    let result = async {
        let mut all_aliases = AliasMap::default();
        all_aliases.insert(
            "env-opts-test".to_string(),
            Alias {
                backend: Some("github:alias/repo[foo=alias,bar=alias]".to_string()),
                versions: Default::default(),
            },
        );
        let config = Config {
            tera_ctx: BASE_CONTEXT.clone(),
            config_files: Default::default(),
            bootstrap_config_maps: vec![],
            bootstrap_dry_run_unresolved_vars: Default::default(),
            env: OnceCell::new(),
            env_with_sources: OnceCell::new(),
            shorthands: get_shorthands(&Settings::get()),
            hooks: OnceCell::new(),
            tasks_cache: Arc::new(DashMap::new()),
            workspace_project_graph_cache: Mutex::new(None),
            daemons: Default::default(),
            tool_request_set: OnceCell::new(),
            toolset: OnceCell::new(),
            all_aliases,
            aliases: Default::default(),
            project_root: Default::default(),
            repo_urls: Default::default(),
            shell_aliases: Default::default(),
            tera_files: Default::default(),
            vars: Default::default(),
            vars_results: OnceCell::new(),
            lockfile_discovery: Default::default(),
        };
        config.tool_request_set.set(ToolRequestSet::new()).ok();
        let config = Arc::new(config);
        let ba = Arc::new(BackendArg::from("env-opts-test"));

        let resolved = config.resolve_tool_opts_with_overrides(&ba).await?;
        let opts = resolved.effective();

        assert_eq!(ba.full(), "github:env/repo[foo=env]");
        assert_eq!(opts.get("foo"), Some("env"));
        assert_eq!(opts.get("bar"), None);
        assert_eq!(
            resolved.source_for_key("foo"),
            Some(crate::toolset::ToolOptionSource::BackendAlias)
        );
        Ok(())
    }
    .await;

    unsafe {
        std::env::remove_var("MISE_BACKENDS_ENV_OPTS_TEST");
    }

    result
}

#[tokio::test]
async fn test_monorepo_union_tool_request_set_preserves_matching_tools() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let root = temp_dir.path();
    let api = root.join("apps/api");
    let web = root.join("apps/web");
    fs::create_dir_all(&api)?;
    fs::create_dir_all(&web)?;

    let root_config = root.join(".test.mise.toml");
    fs::write(
        &root_config,
        r#"
monorepo_root = true

[monorepo]
config_roots = ["apps/api", "apps/web"]
"#,
    )?;
    fs::write(
        api.join(".test.mise.toml"),
        r#"
[tools]
"github:jdx/mise-test-fixtures" = "1"
"#,
    )?;
    fs::write(
        web.join(".test.mise.toml"),
        r#"
[tools]
"github:jdx/mise-test-fixtures" = "2"
"#,
    )?;

    let mut config_files: ConfigMap = Default::default();
    config_files.insert(
        root_config.clone(),
        Arc::new(config_file::mise_toml::MiseToml::from_file(&root_config)?),
    );
    let config = Config {
        tera_ctx: BASE_CONTEXT.clone(),
        config_files,
        bootstrap_config_maps: vec![],
        bootstrap_dry_run_unresolved_vars: Default::default(),
        env: OnceCell::new(),
        env_with_sources: OnceCell::new(),
        shorthands: get_shorthands(&Settings::get()),
        hooks: OnceCell::new(),
        tasks_cache: Arc::new(DashMap::new()),
        workspace_project_graph_cache: Mutex::new(None),
        daemons: Default::default(),
        tool_request_set: OnceCell::new(),
        toolset: OnceCell::new(),
        all_aliases: Default::default(),
        aliases: Default::default(),
        project_root: Default::default(),
        repo_urls: Default::default(),
        shell_aliases: Default::default(),
        tera_files: Default::default(),
        vars: Default::default(),
        vars_results: OnceCell::new(),
        lockfile_discovery: Default::default(),
    };
    let config = Arc::new(config);

    assert_eq!(
        config.monorepo_config_root_dirs_with_filenames(&DEFAULT_CONFIG_FILENAMES)?,
        vec![api, web]
    );
    let trs = config.monorepo_union_tool_request_set().await?;
    let fixture_versions = trs
        .iter()
        .find(|(ba, _, _)| ba.short.contains("mise-test-fixtures"))
        .map(|(_, requests, _)| {
            requests
                .iter()
                .map(|request| request.version().to_string())
                .collect_vec()
        })
        .unwrap_or_default();

    assert_eq!(fixture_versions, vec!["1", "2"]);
    Ok(())
}

#[tokio::test]
async fn test_load_all_config_files_skips_directories() -> Result<()> {
    let _config = Config::get().await?;
    let temp_dir = TempDir::new()?;
    let temp_path = temp_dir.path();

    let sub_dir = temp_path.join("subdir");
    fs::create_dir(&sub_dir)?;

    let file1_path = temp_path.join("config1.toml");
    let file2_path = temp_path.join("config2.toml");
    File::create(&file1_path)?;
    File::create(&file2_path)?;

    fs::write(&file1_path, "key1 = 'value1'")?;
    fs::write(&file2_path, "key2 = 'value2'")?;

    let config_filenames = vec![file1_path.clone(), file2_path.clone(), sub_dir.clone()];
    let idiomatic_filenames = BTreeMap::new();

    let result = load_all_config_files(&config_filenames, &idiomatic_filenames).await?;

    // the result should have only two entries for the files, the directory should not be present
    assert_eq!(result.len(), 2);

    // Check that the directory is not in the result
    assert!(result.contains_key(&file1_path));
    assert!(result.contains_key(&file2_path));
    assert!(!result.contains_key(&sub_dir));

    Ok(())
}

#[test]
fn test_matching_idiomatic_tools_uses_relative_paths() {
    let idiomatic_filenames = BTreeMap::from([
        (
            "subdir/tool.json".to_string(),
            vec!["nested-tool".to_string()],
        ),
        ("tool.json".to_string(), vec!["root-tool".to_string()]),
    ]);

    assert_eq!(
        matching_idiomatic_tools(
            Path::new("/tmp/project/subdir/tool.json"),
            &idiomatic_filenames
        ),
        vec!["nested-tool"]
    );
}

#[test]
fn test_idiomatic_version_file_disabled_is_tool_specific() {
    let disabled = BTreeSet::from([
        "aqua:owner/tool:tool.json".to_string(),
        "node:package.json".to_string(),
        "python:.python-version".to_string(),
    ]);

    assert!(idiomatic_version_file_disabled(
        &disabled,
        "aqua:owner/tool",
        "tool.json"
    ));
    assert!(idiomatic_version_file_disabled(
        &disabled,
        "node",
        "package.json"
    ));
    assert!(!idiomatic_version_file_disabled(
        &disabled,
        "pnpm",
        "package.json"
    ));
    assert!(!idiomatic_version_file_disabled(
        &disabled, "node", ".nvmrc"
    ));
}

#[tokio::test]
async fn test_get_repo_url_ssh() -> Result<()> {
    let config = Config::reset().await?;
    let urls = [
        "ssh://git@gitlab.dev/mobile/asdf-gitique.git",
        "git@github.com:user/repo.git",
        "git://example.com/repo.git",
        "http://example.com/repo.git",
        "https://example.com/repo.git",
    ];

    for url in urls {
        assert!(
            config.get_repo_url(url).is_some(),
            "URL should be considered valid: {url}"
        );
    }
    Ok(())
}

#[test]
fn test_get_repo_url_preserves_explicit_local_paths() {
    let temp = tempfile::tempdir().unwrap();
    let local_asdf = temp
        .path()
        .join("plugins/local-asdf")
        .to_string_lossy()
        .into_owned();
    let local_vfox = temp
        .path()
        .join("plugins/local-vfox")
        .to_string_lossy()
        .into_owned();
    let repo_urls = HashMap::from([
        ("local-asdf".to_string(), local_asdf.clone()),
        ("vfox:local-vfox".to_string(), local_vfox.clone()),
        ("remote-asdf".to_string(), "owner/asdf-plugin".to_string()),
        (
            "vfox:remote-vfox".to_string(),
            "owner/vfox-plugin".to_string(),
        ),
        (
            "asdf:explicit-asdf".to_string(),
            "owner/asdf-plugin".to_string(),
        ),
        (
            "vfox-backend:explicit-backend".to_string(),
            "owner/backend-plugin".to_string(),
        ),
    ]);
    let config = Config {
        tera_ctx: BASE_CONTEXT.clone(),
        config_files: Default::default(),
        bootstrap_config_maps: vec![],
        bootstrap_dry_run_unresolved_vars: Default::default(),
        env: OnceCell::new(),
        env_with_sources: OnceCell::new(),
        shorthands: get_shorthands(&Settings::get()),
        hooks: OnceCell::new(),
        tasks_cache: Arc::new(DashMap::new()),
        workspace_project_graph_cache: Mutex::new(None),
        daemons: Default::default(),
        tool_request_set: OnceCell::new(),
        toolset: OnceCell::new(),
        all_aliases: Default::default(),
        aliases: Default::default(),
        project_root: Default::default(),
        repo_urls,
        shell_aliases: Default::default(),
        tera_files: Default::default(),
        vars: Default::default(),
        vars_results: OnceCell::new(),
        lockfile_discovery: Default::default(),
    };

    assert_eq!(
        config.get_repo_url("local-asdf").as_deref(),
        Some(local_asdf.as_str())
    );
    assert_eq!(
        config.get_repo_url("local-vfox").as_deref(),
        Some(local_vfox.as_str())
    );
    assert_eq!(
        config.get_repo_url("remote-asdf").as_deref(),
        Some("https://github.com/owner/asdf-plugin.git")
    );
    assert_eq!(
        config.get_repo_url("remote-vfox").as_deref(),
        Some("https://github.com/owner/vfox-plugin.git")
    );
    assert_eq!(
        config.get_configured_plugin_type("local-vfox"),
        Some(PluginType::Vfox)
    );
    assert_eq!(
        config.get_configured_plugin_type("explicit-asdf"),
        Some(PluginType::Asdf)
    );
    assert_eq!(
        config.get_configured_plugin_type("explicit-backend"),
        Some(PluginType::VfoxBackend)
    );
    assert_eq!(config.get_configured_plugin_type("local-asdf"), None);
}

#[tokio::test]
async fn test_load_task_file_supports_per_task_vars() -> Result<()> {
    let config = Config::reset().await?;
    let temp_dir = TempDir::new()?;
    let tasks_toml = temp_dir.path().join("tasks.toml");
    fs::write(
        &tasks_toml,
        r#"
[build]
description = "{{vars.target}}"
run = "echo build"
vars = { target = "linux" }
"#,
    )?;

    let tasks = load_task_file(
        &config,
        &tasks_toml,
        temp_dir.path(),
        &ResolvedTaskConfig::default(),
        &TaskDefinitions::default(),
        None,
        None,
    )
    .await?;
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].name, "build");
    assert_eq!(tasks[0].description, "linux");
    Ok(())
}
