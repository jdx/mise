---
description: "Install the Bazel version a project pins in .bazelversion, or run Bazelisk, with mise."
---

# Bazel

Install the Bazel version declared in a project's `.bazelversion` file, so the
version is not repeated in `mise.toml`.

## Use a project's `.bazelversion` {#use-a-project-s-bazelversion}

Enable [idiomatic version files](/dev-tools/versions.html#idiomatic-version-files)
for `bazel` in the project:

```sh
mise settings add --local idiomatic_version_file_enable_tools bazel
```

`--local` writes the setting to the project's `mise.toml`, so everyone who
clones the project reads `.bazelversion`. Drop `--local` to enable it for all
your projects in your global config instead.

For example, a project selects Bazel 9.2.0 with this file:

```text [.bazelversion]
9.2.0
```

From that project, install the selected tools and check Bazel:

```sh
mise install
mise exec -- bazel --version
```

`mise tool bazel --requested` shows the version request mise read. If the
project also sets Bazel in `mise.toml`, remove that entry so `.bazelversion`
selects the version.

## Check which values mise reads {#supported-values}

mise reads only the first line of `.bazelversion`. It accepts concrete release
versions, including release candidates and prereleases:

| Value                                                    | Read by mise? |
| -------------------------------------------------------- | ------------- |
| `7.2.1`                                                  | Yes           |
| `8.0.0rc1`                                               | Yes           |
| `5.0.0-pre.20210317.1`                                   | Yes           |
| `latest`, `latest-1`, `last_green`, `last_rc`, `rolling` | No            |
| `8.x`, `8.*`                                             | No            |
| A commit hash or `<FORK>/<VERSION>`                      | No            |

An unsupported value gives no version request, even if a later line contains a
supported version. To select Bazel independently of such a file, set it in
`mise.toml`:

```sh
mise use bazel@9.2.0
```

## Use Bazelisk instead {#using-bazelisk-instead}

Bazelisk is a launcher that reads `.bazelversion` itself and downloads the
Bazel release it names. If your project relies on Bazelisk's version selectors,
such as `last_green`, install Bazelisk with mise and let it read the file:

```sh
mise use bazelisk
mise exec -- bazelisk --version
```

`.bazelversion` selects the Bazel version, not the Bazelisk version, so enabling
idiomatic version files for `bazelisk` has no effect.
