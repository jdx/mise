---
description: "Move tools off the deprecated ubi backend to github, gitlab or http before mise 2027.1.0."
---

# Migrating off the ubi backend <Badge type="danger" text="deprecated" />

The `ubi` backend installed single executables from GitHub and GitLab releases
with the [ubi](https://github.com/houseabsolute/ubi) library. It is deprecated:
move each `ubi:` tool to the [github](/dev-tools/backends/github.html),
[gitlab](/dev-tools/backends/gitlab.html) or
[http](/dev-tools/backends/http.html) backend.

::: warning Deprecated
Since 2026.4.0, mise prints a deprecation warning, once per run, when it lists
versions for or installs a `ubi:` tool. The ubi backend will be removed in mise
2027.1.0; migrate before you upgrade to that release.
:::

## Migrate a tool {#usage}

Migrate one tool at a time, and keep the old installation until the replacement
works.

1. Change the backend prefix:

   | ubi entry                                   | Replacement                                                                                |
   | ------------------------------------------- | ------------------------------------------------------------------------------------------ |
   | `ubi:owner/repo`                            | `github:owner/repo`                                                                        |
   | `ubi:owner/repo` with `provider = "gitlab"` | `gitlab:owner/repo`                                                                        |
   | `ubi:https://...`, a direct download URL    | An `http:` tool with that [`url`](/dev-tools/backends/http.html#usage) and a version label |

2. Replace the tool options using the [option mapping](#option-mapping).
3. Install the tool and run it:

   ```sh
   mise install
   mise exec -- <command> --version
   ```

4. If you use a lockfile, run `mise lock` to write entries for the new tool.
   Then remove the old installation with
   `mise uninstall --all ubi:owner/repo`.

A tool that named its executable with `exe` usually needs no option on the new
backend, because the github backend extracts the archive and finds the
executable:

```toml
[tools]
# was: "ubi:BurntSushi/ripgrep" = { version = "14.1.1", exe = "rg" }
"github:BurntSushi/ripgrep" = "14.1.1"
```

To keep a renamed executable, rename it by its file name:

```toml
[tools]
# was: "ubi:cli/cli" = { version = "latest", exe = "gh", rename_exe = "gh-cli" }
"github:cli/cli" = { version = "latest", rename_exe = { "gh" = "gh-cli" } }
```

A GitLab tool moves to the gitlab backend:

```toml
[tools]
# was: "ubi:gitlab-org/cli" = { version = "latest", exe = "glab", provider = "gitlab" }
"gitlab:gitlab-org/cli" = "latest"
```

## Option mapping

| ubi option       | Replacement                                                                                                                                                                                  |
| ---------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `exe`            | Usually remove it: the github and gitlab backends extract the whole archive and find the executables. To expose only some, use [`filter_bins`](/dev-tools/backends/github.html#filter-bins). |
| `rename_exe`     | [`rename_exe`](/dev-tools/backends/github.html#rename-exe). When `exe` named a file other than the repository name, use the table form, such as `rename_exe = { "gh" = "gh-cli" }`.          |
| `matching`       | [`matching`](/dev-tools/backends/github.html#matching), which applies before platform detection; see [Behavior differences](#behavior-differences).                                          |
| `matching_regex` | [`matching_regex`](/dev-tools/backends/github.html#matching-regex)                                                                                                                           |
| `provider`       | The `github:` or `gitlab:` prefix                                                                                                                                                            |
| `api_url`        | [`api_url`](/dev-tools/backends/github.html#api-url)                                                                                                                                         |
| `extract_all`    | Remove it: these backends always extract the whole archive.                                                                                                                                  |
| `bin_path`       | [`bin_path`](/dev-tools/backends/github.html#bin-path). The github backend strips a single top-level directory unless `bin_path` is set, so check the path against the archive.              |
| `tag_regex`      | No equivalent. [`version_prefix`](/dev-tools/backends/github.html#version-prefix) keeps only tags that start with a fixed prefix.                                                            |

## Behavior differences

| Behavior                      | ubi                                                                                                | github and gitlab backends                                                                                                 |
| ----------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| `matching`                    | Breaks a tie between assets that already match your platform, and is ignored when only one matches | Filters assets before platform detection; a value that matches no asset for your platform is an error                      |
| What is installed             | One executable, unless `extract_all` is set                                                        | The whole archive; the directories with executables go on `PATH`, so other executables in the archive can appear there     |
| Repositories without releases | Lists Git tags as versions                                                                         | Lists only releases with assets, so a repository that publishes only tags has no versions                                  |
| Verification                  | Records a checksum of the installed executable                                                     | Checks the release's digest or checksum file and GitHub artifact attestations, and records the download URL in `mise.lock` |

Neither backend keeps two entries for the same `owner/repo` apart:
`ubi:owner/repo[matching=a]` and `ubi:owner/repo[matching=b]` resolve to one
tool, and only the first entry is installed. To install several binaries from
one release, give each its own
[tool alias](/dev-tools/backends/github.html#multiple-assets-from-the-same-release)
that points at `github:owner/repo`.

## Reference for existing configurations {#tool-options}

Until you migrate, these options still apply to `ubi:` tools:

- `exe`: the executable to extract from the archive. Defaults to the
  repository name.
- `rename_exe`: the name to give the extracted executable.
- `matching`: text to look for in asset names when several assets match your
  OS and architecture, such as `gnu`, `musl` or `msvc`. ubi ignores it when only
  one asset matches.
- `matching_regex`: a regular expression that asset names must match before
  OS and architecture matching. ubi reports an error when nothing matches.
- `provider`: `github` (the default) or `gitlab`.
- `api_url`: the provider's API URL, for a self-hosted instance. Set `provider`
  too, and a token in `MISE_GITHUB_ENTERPRISE_TOKEN` or
  `MISE_GITLAB_ENTERPRISE_TOKEN`.
- `extract_all`: `true` extracts every file in the archive instead of one
  executable. It ignores `exe` and `rename_exe`.
- `bin_path`: the directory in the install that holds the executables, used with
  `extract_all`.
- `tag_regex`: a regular expression that release tags must match to be listed
  as versions.

A tool is written as `ubi:owner/repo`, `ubi:owner/repo@1.25.1`, or a direct
download URL such as
`ubi:https://github.com/goreleaser/goreleaser/releases/download/v1.16.2/goreleaser_Darwin_arm64.tar.gz`,
which has only the version `latest`.

mise puts `bin_path` on `PATH` when it is set. Otherwise it uses the install
directory when `extract_all` is set, then the install's `bin/` directory if it
exists, and the install directory last.

`tag_regex` is matched against the whole tag, including a leading `v`.
`cargo-bins/cargo-binstall` publishes releases for several crates, and this
keeps only the `v1.2.3`-style tags:

```sh
mise use 'ubi:cargo-bins/cargo-binstall[tag_regex=^v\d+\.]'
mise ls-remote 'ubi:cargo-bins/cargo-binstall[tag_regex=^v\d+\.]'
```

## Troubleshooting

If ubi picks the wrong asset, cannot find the executable or lists unrelated
versions, migrate the tool. The github backend's
[`matching`](/dev-tools/backends/github.html#matching),
[`asset_pattern`](/dev-tools/backends/github.html#asset-pattern),
[`version_prefix`](/dev-tools/backends/github.html#version-prefix) and
[`rename_exe`](/dev-tools/backends/github.html#rename-exe) cover these cases.

Implementation: [`src/backend/ubi.rs`](https://github.com/jdx/mise/blob/main/src/backend/ubi.rs).
