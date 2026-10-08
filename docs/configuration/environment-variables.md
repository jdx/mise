---
description: "Look up the MISE_* environment variables that are not settings: directories, logging, per-tool version overrides, and terminal output."
socialDescription: "Look up MISE_* variables that are not settings: directories, logging, and tool overrides."
---

# MISE\_\* environment variables

Every [setting](/configuration/settings.html) except the `write_targets` tables
also has a `MISE_*` environment variable, listed in its entry on the Settings
page; for example, `MISE_JOBS=4`
sets [`jobs`](/configuration/settings.html#jobs). This page lists the variables
that are not settings, plus a few hidden settings that are only useful as
variables.

Set them in the environment that starts mise, such as your shell startup file
or a CI job.

## Directories {#directories}

Each of these moves one of mise's directories. [Directories](/directories.html)
lists their defaults and what each one holds.

| Variable                 | Directory                                                                            |
| ------------------------ | ------------------------------------------------------------------------------------ |
| `MISE_CONFIG_DIR`        | [Global config](/directories.html#config-mise)                                       |
| `MISE_DATA_DIR`          | [Installed tools and plugins](/directories.html#local-share-mise)                    |
| `MISE_CACHE_DIR`         | [Cache](/directories.html#cache-mise)                                                |
| `MISE_STATE_DIR`         | [Local state](/directories.html#local-state-mise)                                    |
| `MISE_TMP_DIR`           | [Temporary files](/directories.html)                                                 |
| `MISE_SYSTEM_CONFIG_DIR` | [System config](/directories.html#system-config); `MISE_SYSTEM_DIR` is an older name |
| `MISE_INSTALLS_DIR`      | [Installed versions](/directories.html#local-share-mise-installs)                    |
| `MISE_INSTALL_STORE_DIR` | [Install store](/directories.html#local-share-mise-installs) for the identity layout |
| `MISE_PLUGINS_DIR`       | [Plugins](/directories.html#local-share-mise-plugins)                                |
| `MISE_DOWNLOADS_DIR`     | [Downloads](/directories.html#local-share-mise-downloads)                            |
| `MISE_SYSTEM_DATA_DIR`   | [System installs and shims](/directories.html#system-installs-and-shims)             |

## Config files {#config-files}

### `MISE_<TOOL>_VERSION` {#mise-tool-version}

Selects a version of one tool and overrides every config file for the commands
that see it:

```sh
MISE_NODE_VERSION=22 mise exec -- node --version
# v22.x.x
```

Write the tool name in upper case with `-` changed to `_`, as in
`MISE_LS_LINT_VERSION` for `ls-lint`. Separate several versions with spaces.
[`mise shell`](/cli/shell.html) sets this variable for the current shell
session. See [Version requests and version files](/dev-tools/versions.html) for
the accepted versions.

### `MISE_NO_CONFIG` {#mise-no-config}

Set to `1` to load no config files at all, the same as the `--no-config` flag.

### Settings for config files

mise reads these settings before any `mise.toml`, because they control which
config files it loads or, for `global_config_root`, the
[config root](/configuration.html#config-root) of the global config. Some of
them work only as environment variables; the last column says where else you
can set each one. See
[Settings with restricted locations](/configuration/settings.html#settings-with-restricted-locations).

| Variable                                | Setting                                                                                                        | Also works in                                                  |
| --------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------- |
| `MISE_ENV`                              | [`env`](/configuration/settings.html#env), the [config environments](/configuration/environments.html) to load | [`.miserc.toml`](/configuration.html#miserc) and the `-E` flag |
| `MISE_AUTO_ENV`                         | [`auto_env`](/configuration/settings.html#auto_env)                                                            | `.miserc.toml`                                                 |
| `MISE_ENV_CONF_D`                       | [`env_conf_d`](/configuration/settings.html#env_conf_d)                                                        | `.miserc.toml`                                                 |
| `MISE_CEILING_PATHS`                    | [`ceiling_paths`](/configuration/settings.html#ceiling_paths)                                                  | `.miserc.toml`                                                 |
| `MISE_IGNORED_CONFIG_PATHS`             | [`ignored_config_paths`](/configuration/settings.html#ignored_config_paths)                                    | `.miserc.toml`                                                 |
| `MISE_OVERRIDE_CONFIG_FILENAMES`        | [`override_config_filenames`](/configuration/settings.html#override_config_filenames)                          | `.miserc.toml`                                                 |
| `MISE_OVERRIDE_TOOL_VERSIONS_FILENAMES` | [`override_tool_versions_filenames`](/configuration/settings.html#override_tool_versions_filenames)            | `.miserc.toml`                                                 |
| `MISE_GLOBAL_CONFIG_FILE`               | [`global_config_file`](/configuration/settings.html#global_config_file); `MISE_CONFIG_FILE` is an older name   | Nowhere else                                                   |
| `MISE_GLOBAL_CONFIG_ROOT`               | [`global_config_root`](/configuration/settings.html#global_config_root)                                        | Nowhere else                                                   |
| `MISE_SYSTEM_CONFIG_FILE`               | [`system_config_file`](/configuration/settings.html#system_config_file)                                        | Nowhere else                                                   |
| `MISE_DEFAULT_CONFIG_FILENAME`          | [`default_config_filename`](/configuration/settings.html#default_config_filename)                              | Nowhere else                                                   |
| `MISE_DEFAULT_TOOL_VERSIONS_FILENAME`   | [`default_tool_versions_filename`](/configuration/settings.html#default_tool_versions_filename)                | Nowhere else                                                   |

You can also set these under `[settings]` in a config file:

| Variable                    | Setting                                                                                                      |
| --------------------------- | ------------------------------------------------------------------------------------------------------------ |
| `MISE_TRUSTED_CONFIG_PATHS` | [`trusted_config_paths`](/configuration/settings.html#trusted_config_paths), in global or system config only |
| `MISE_ENV_FILE`             | [`env_file`](/configuration/settings.html#env_file)                                                          |
| `MISE_NO_ENV`               | [`no_env`](/configuration/settings.html#no_env), the same as `--no-env`                                      |
| `MISE_NO_HOOKS`             | [`no_hooks`](/configuration/settings.html#no_hooks), the same as `--no-hooks`                                |

## Logging and output {#logging}

### `MISE_DEBUG` and `MISE_TRACE` {#mise-debug}

Set `MISE_DEBUG=1` for debug logs, or `MISE_TRACE=1` for trace logs, which are
more detailed. They match the `-v` and `-vv` flags. Use them, not `RUST_LOG`,
when you report a problem.

### `MISE_LOG_LEVEL` {#mise-log-level}

Sets the log level: `trace`, `debug`, `info` (the default), `warn`, or `error`.
[`MISE_QUIET=1`](/configuration/settings.html#quiet) also sets the level to
`error`, and it hides task headers and progress as well. The hidden
`--log-level` flag takes the same levels.

### `MISE_LOG_FILE` {#mise-log-file}

Also writes logs to this file, appending to it and creating its directory if
needed:

```sh
MISE_LOG_FILE=~/mise.log mise install
```

### `MISE_LOG_FILE_LEVEL` {#mise-log-file-level}

Sets the level for the log file, so you can keep detailed logs without
cluttering the terminal. It takes the same values as `MISE_LOG_LEVEL` and
defaults to the terminal's level.

### `MISE_LOG_HTTP` {#mise-log-http}

Set to `1` to print every HTTP request mise sends, after
[URL replacements](/url-replacements.html), with its response status:

```text
GET https://mise-versions.jdx.dev/data/jq.toml 200 OK
```

### `MISE_LOG_VERBOSE_DEPS` {#mise-log-verbose-deps}

mise always drops debug and trace logs from noisy libraries such as `h2`,
`hyper`, `reqwest`, and `rustls`, which log every HTTP/2 frame or socket read.
Set this to `1` to let them through; it is the only way to see them, even with
`MISE_TRACE=1` or `-vv`.

### `MISE_TIMINGS` {#mise-timings}

Set to `1` to print how long each step of a command takes, or `2` for a detailed
breakdown with cumulative times. See
[Troubleshooting](/troubleshooting.html#slow-shell-prompts) for profiling a slow
prompt.

### `MISE_FRIENDLY_ERROR` {#mise-friendly-error}

mise prints short error messages unless debug logging is on. Set this to `0` to
print the full error report, with its cause chain and the source location, or
to `1` to keep short messages even with debug logging.

### `MISE_TERM_WIDTH` {#mise-term-width}

Sets the terminal width mise uses for tables and lists, such as `mise ls`.
Without it, mise uses `COLUMNS` and then the width it detects. Set it in CI and
other non-interactive environments where detection returns a bad value, such as
CircleCI, which reports a width of `0`. The value is used exactly, so you can
also force a narrower width:

```sh
MISE_TERM_WIDTH=120 mise ls
```

### Settings for output

| Variable       | Setting                                           |
| -------------- | ------------------------------------------------- |
| `MISE_QUIET`   | [`quiet`](/configuration/settings.html#quiet)     |
| `MISE_VERBOSE` | [`verbose`](/configuration/settings.html#verbose) |
| `MISE_COLOR`   | [`color`](/configuration/settings.html#color)     |
| `MISE_RAW`     | [`raw`](/configuration/settings.html#raw)         |
| `MISE_YES`     | [`yes`](/configuration/settings.html#yes)         |

## Shells {#shells}

### `MISE_FISH_AUTO_ACTIVATE` {#mise-fish-auto-activate}

Homebrew and other packages install a fish `vendor_conf.d` script that activates
mise automatically. Set this to `0` to turn that off:

```fish
set -Ux MISE_FISH_AUTO_ACTIVATE 0
```

See [Shell setup](/shell-setup.html#fish).

## Networking {#networking}

### `MISE_LIST_ALL_VERSIONS` {#mise-list-all-versions}

Set to `1` to read every page of a repository's GitHub, GitLab, or Forgejo
releases or tags instead of stopping early. It applies to every backend and core
tool that lists versions from those forges, such as the github and aqua backends
and Go. Use it when an older release is missing from `mise ls-remote`; listing
takes longer and uses more API requests.

### Tokens

`MISE_GITHUB_TOKEN`, `MISE_GITLAB_TOKEN`, `MISE_FORGEJO_TOKEN`, and the other
token variables are described in
[GitHub, GitLab, and Forgejo tokens](/dev-tools/github-tokens.html).

### Settings for networking

| Variable                | Setting                                                             |
| ----------------------- | ------------------------------------------------------------------- |
| `MISE_OFFLINE`          | [`offline`](/configuration/settings.html#offline)                   |
| `MISE_PREFER_OFFLINE`   | [`prefer_offline`](/configuration/settings.html#prefer_offline)     |
| `MISE_HTTP_TIMEOUT`     | [`http_timeout`](/configuration/settings.html#http_timeout)         |
| `MISE_URL_REPLACEMENTS` | [`url_replacements`](/configuration/settings.html#url_replacements) |

## Variables mise sets

mise also sets variables for the commands it runs, such as `MISE_ENV` and the
`MISE_TASK_*` variables in tasks. See
[Task environment](/tasks/running-tasks.html#task-environment) and
[Hooks](/hooks.html).
