---
description: "Use mise to select Bazel from a project's .bazelversion file."
---

# Bazel Cookbook

Use mise to install the Bazel version declared in a project's `.bazelversion` file.
This avoids repeating the version in `mise.toml`.

## Use a project's `.bazelversion`

Enable [idiomatic version files](/configuration.html#idiomatic-version-files) for `bazel`:

```sh
mise settings add idiomatic_version_file_enable_tools bazel
```

For example, a project can select Bazel 7.2.1 with this file:

```text [.bazelversion]
7.2.1
```

From that project, install the selected tools and check Bazel:

```sh
mise install
mise exec -- bazel --version
```

Use `mise tool bazel --requested` to inspect the version request read by mise.
If the project also configures Bazel in `mise.toml`, remove that duplicate entry to let
`.bazelversion` select the version.

## Supported values

mise reads only the first line of `.bazelversion`. It accepts concrete release versions,
including release candidates and prereleases:

| Value                                                    | Read by mise? |
| -------------------------------------------------------- | ------------- |
| `7.2.1`                                                  | Yes           |
| `8.0.0rc1`                                               | Yes           |
| `5.0.0-pre.20210317.1`                                   | Yes           |
| `latest`, `latest-1`, `last_green`, `last_rc`, `rolling` | No            |
| `8.x`, `8.*`                                             | No            |
| A commit hash or `<FORK>/<VERSION>`                      | No            |

Unsupported values supply no version request, even if a later line contains a supported version.
To select Bazel independently of such a file, configure it explicitly:

```sh
mise use bazel@7.2.1
```

## Using Bazelisk instead

Bazelisk is a launcher that reads `.bazelversion` itself and manages the corresponding Bazel
installation. If your project relies on Bazelisk's version selectors, install Bazelisk with mise
and let it interpret the file:

```sh
mise use bazelisk
mise exec -- bazelisk --version
```

The version in `.bazelversion` applies to **Bazel**, not Bazelisk. Enabling idiomatic version
files for `bazelisk` does not select a Bazelisk version from that file.
