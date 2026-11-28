---
description: "The ubi backend was removed from mise. Move ubi: tools to the github, gitlab or http backend."
---

# Migrating off the ubi backend <Badge type="danger" text="removed" />

The `ubi` backend installed single executables from GitHub and GitLab releases
with the [ubi](https://github.com/houseabsolute/ubi) library. It was deprecated
in mise 2026.4.0, and mise releases after 2026.11.14 no longer include it.
Move each `ubi:` tool to the [github](/dev-tools/backends/github.html),
[gitlab](/dev-tools/backends/gitlab.html) or
[http](/dev-tools/backends/http.html) backend.

mise does not translate `ubi:` entries for you, because the options don't carry
over one for one and the new backend installs the tool in a different
directory. A config, lockfile or command that still names a `ubi:` tool fails
to install it, with an error that names the replacement:

```text
ubi:cli/cli: the ubi backend was removed from mise. Use github:cli/cli (gitlab:cli/cli for a GitLab project) instead. See https://mise.jdx.dev/dev-tools/backends/ubi.html for how ubi options map to it
```

Other commands skip the tool and print the same message as a warning.

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
   A tool you configure by its registry name (`gh = "2"`) whose `mise.lock`
   entries record a `ubi:` backend keeps failing until you delete those
   entries; `mise lock` then records the registry's backend. The old installation stays in mise's data directory; delete its
   `installs/ubi-owner-repo` directory when you no longer need it.

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
| Verification                  | Records a checksum of the installed executable, or of the whole install with `extract_all`         | Checks the release's digest or checksum file and GitHub artifact attestations, and records the download URL in `mise.lock` |

Neither backend keeps two entries for the same `owner/repo` apart:
`ubi:owner/repo[matching=a]` and `ubi:owner/repo[matching=b]` resolve to one
tool, and only the first entry is installed. To install several binaries from
one release, give each its own
[tool alias](/dev-tools/backends/github.html#multiple-assets-from-the-same-release)
that points at `github:owner/repo`.
