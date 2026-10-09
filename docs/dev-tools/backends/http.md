---
description: "Install a binary, script or archive from a URL, with URL templates, version lists and checksums."
---

# http backend

The `http` backend installs a binary, script or archive from a download URL. Use
it when a tool has no release backend, or for artifacts you host yourself. The
[s3 backend](/dev-tools/backends/s3.html) shares its download, verification and
extraction options.

## Usage

Replace the example URL with an artifact for your platform, then install it in
the current project:

```sh
mise use 'http:my-tool[url=https://example.com/releases/my-tool-v1.0.0.tar.gz]@1.0.0'
mise exec -- my-tool --version
```

Quote the argument so the shell does not expand the brackets. This writes the
URL and version to `mise.toml`. Add `-g` for a global tool.

```toml
[tools]
"http:my-tool" = { version = "1.0.0", url = "https://example.com/releases/my-tool-v1.0.0.tar.gz" }
```

The name after `http:` is your choice; it names the tool and its install
directory. Use `https://` URLs, and pin the artifact with a lockfile or a
[checksum](#verification).

::: v-pre
With a fixed URL, the version is only a label: changing it does not change what
is downloaded. To follow releases, put `{{ version }}` in the URL (see
[URL templates](#template-variables)). mise cannot find versions from a download
URL, so `latest` and `mise ls-remote` work only when you also set
[`version_list_url`](#version-list-url).
:::

## URL templates {#template-variables}

### `url` {#url-required}

The download URL: `https://`, `http://` or `file://`. Set it at the top level, or
per platform under `platforms.<os>-<arch>.url`; a tool needs one or the other.
The URL is a [Tera template](/templates.html). Write values in double braces:

```toml
[tools]
"http:my-tool" = { version = "1.0.0", url = "https://example.com/releases/my-tool-v{{ version }}.tar.gz" }
```

::: v-pre

- `{{ version }}`: the version being installed
- `{{ os() }}`: `linux`, `macos` or `windows`
- `{{ arch() }}`: `x64` or `arm64`
- `{{ os_family() }}`: `unix` or `windows`

:::

`os()` and `arch()` take keyword arguments that rename a value, for a project
that uses other names. HashiCorp uses `darwin` for macOS and `amd64` for x64:

```toml
[tools]
"http:sentinel" = {
  version = "0.26.3",
  url = 'https://releases.hashicorp.com/sentinel/{{ version }}/sentinel_{{ version }}_{{ os(macos="darwin") }}_{{ arch(x64="amd64") }}.zip',
}
```

This downloads `sentinel_0.26.3_darwin_arm64.zip` on macOS arm64,
`sentinel_0.26.3_darwin_amd64.zip` on macOS x64 and
`sentinel_0.26.3_linux_amd64.zip` on Linux x64. Use a single-quoted TOML string
when the template contains double quotes, as here.

::: v-pre
The single-brace placeholder `{version}` is deprecated: mise has warned about it
since 2026.3.0 and will stop accepting it in 2027.3.0. Write `{{ version }}`
instead.
:::

### Local files

A `file://` URL installs an archive that is already on disk, such as one
downloaded by hand on a restricted network. mise copies it instead of
downloading it, then verifies, extracts and links it the same way. Add a
`checksum` so a local file is verified too:

```toml
[tools]
"http:my-tool" = { version = "1.0.0", url = "file:///opt/archives/my-tool-v1.0.0-linux-x64.tar.gz", checksum = "sha256:REPLACE_WITH_THE_64_HEX_DIGIT_DIGEST" }
```

`mise.lock` records the URL as written, so a lockfile that names a local path
works only on machines that have the file at that path.

## Per-platform downloads {#platform-specific-urls}

When each platform needs a different download, set the URL under
`platforms.<os>-<arch>`:

```toml
[tools."http:my-tool"]
version = "1.0.0"

[tools."http:my-tool".platforms]
macos-x64 = { url = "https://example.com/releases/my-tool-v1.0.0-macos-x64.tar.gz" }
macos-arm64 = { url = "https://example.com/releases/my-tool-v1.0.0-macos-arm64.tar.gz" }
linux-x64 = { url = "https://example.com/releases/my-tool-v1.0.0-linux-x64.tar.gz" }
```

Platform keys are `<os>-<arch>` with mise's names, `linux`, `macos` or `windows`
and `x64` or `arm64`. mise also accepts `darwin`, `amd64`, `x86_64` and
`aarch64`, so `darwin-aarch64` means `macos-arm64`. On a platform with no URL,
the install fails with an error that lists the platforms that have one.

Other download options can be set per platform the same way: `checksum`,
`checksum_url`, `size`, `headers`, `format`, `strip_components`, `bin_path`,
`bin`, `rename_exe` and `windows_script_interpreter`. A platform value overrides
the top-level one:

```toml
[tools."http:my-tool".platforms]
macos-arm64 = {
  url = "https://example.com/releases/my-tool-v1.0.0-macos-arm64",
  checksum = "sha256:REPLACE_WITH_THE_64_HEX_DIGIT_DIGEST",
  format = "tar.xz",
}
```

## Version discovery

Set `version_list_url` so that `mise ls-remote`, `latest` and version prefixes
can find versions. mise keeps the order of the list and treats the last entry as
the newest. Versions that look like prereleases, such as `2.0.0-rc1`, are left
out unless the tool sets `prerelease = true`.

### `version_list_url` {#version-list-url}

A URL that returns the list of versions. It is fetched with the tool's
[`headers`](#headers).

```toml
[tools."http:my-tool"]
version = "latest"
url = "https://example.com/releases/my-tool-v{{ version }}.tar.gz"
version_list_url = "https://example.com/releases/versions.txt"
```

Without [`version_regex`](#version-regex) or
[`version_json_path`](#version-json-path), mise reads these formats and removes
a leading `v` from each version:

- Plain text with one version per line; blank lines and lines that start with
  `#` are skipped
- A JSON array of strings: `["1.0.0", "1.1.0", "2.0.0"]`
- A JSON array of objects with a `version`, `tag_name`, `name`, `tag` or `v`
  field: `[{"version": "1.0.0"}, {"tag_name": "v2.0.0"}]`
- A JSON object with a `versions`, `releases` or `tags` array:
  `{"versions": ["1.0.0", "2.0.0"]}`

For releases hosted on GitHub, use the
[github backend](/dev-tools/backends/github.html) instead.

### `version_regex` {#version-regex}

A regular expression that extracts versions from the response, such as an HTML
index. mise uses the first capture group, or the whole match when there is
none:

```toml
[tools."http:my-tool"]
version = "latest"
url = "https://example.com/releases/my-tool-v{{ version }}.tar.gz"
version_list_url = "https://example.com/releases/"
version_regex = 'my-tool-v(\d+\.\d+\.\d+)\.tar\.gz'
```

When `version_json_path` is also set, mise uses the JSON path and ignores the
regex.

### `version_json_path` {#version-json-path}

A jq-like path that extracts versions from a JSON response:

```toml
[tools."http:my-tool"]
version = "latest"
url = "https://example.com/releases/my-tool-v{{ version }}.tar.gz"
version_list_url = "https://example.com/api/releases"
version_json_path = ".[].tag_name"
```

| Path              | Selects                                       |
| ----------------- | --------------------------------------------- |
| `.`               | The root value                                |
| `.[]`             | Each element of an array                      |
| `.[].field`       | A field of each array element                 |
| `.field`          | A field of an object                          |
| `.field[]`        | Each element of an array in a field           |
| `.field.subfield` | A nested field, such as `.data.versions[]`    |
| `.[?field=value]` | The array elements whose field equals a value |

A filter keeps one channel of an API that returns several, such as Flutter's
stable releases:

```toml
version_json_path = ".releases[?channel=stable].version"
```

### `version_expr` {#version-expr}

An [expr-lang](https://expr-lang.org/) expression for a rule that a regex or a
JSON path cannot express. It receives the response as `body` and the values that
`version_regex` or `version_json_path` extracted as `versions`, and returns an
array of version strings. Its result becomes the version list.

```toml
[tools."http:my-tool"]
version = "latest"
url = "https://example.com/releases/my-tool-v{{ version }}.tar.gz"
version_list_url = "https://example.com/versions.txt"
version_expr = 'filter(split(body, "\n"), # != "")'
```

Other examples:

```toml
# keys of a JSON object, as in {"versions": {"1.0.0": {}, "2.0.0": {}}}
version_expr = 'keys(fromJSON(body).versions)'
```

```toml
# sort with mise's version-aware comparator
version_expr = 'fromJSON(body) | map({ trimPrefix(#.tag_name, "v") }) | sortVersions()'
```

All [expr-lang built-in functions](https://expr-lang.org/docs/language-definition)
are available, such as `fromJSON`, `keys`, `filter`, `map` and `split`. mise adds
`sortVersions(array)` for version-aware ordering. Prefer
[`version_order = "semver"`](#version-order) when the versions follow semantic
versioning, and use `sortVersions()` when the expression needs a sorted value
along the way.

### `version_order` {#version-order}

Many JSON indexes list versions newest first, which makes the last entry, and so
`latest`, the oldest. Set `version_order = "semver"` to order versions that
follow semantic versioning by precedence instead:

```toml
[tools."http:my-tool"]
version = "latest"
version_order = "semver"
url = "https://example.com/my-tool-{{ version }}.tar.gz"
version_list_url = "https://example.com/my-tool/releases.json"
version_json_path = ".[].version"
```

See [version ordering](/dev-tools/versions.html#version-ordering).

## Verification

With [lockfiles](/dev-tools/mise-lock.html) enabled, mise records a checksum and
size on the first install and checks them on every later one. `checksum` and
`size` pin values without a lockfile, and `checksum_url` lets `mise lock` record
published checksums for every platform.

### `checksum`

The expected digest of the download, as `<algorithm>:<hash>` with an algorithm
such as `sha256`, `sha512` or `blake3`. Take it from a source you trust:

```toml
[tools."http:my-tool"]
version = "1.0.0"
url = "https://example.com/releases/my-tool-v1.0.0.tar.gz"
checksum = "sha256:REPLACE_WITH_THE_64_HEX_DIGIT_DIGEST"
```

A checksum describes one artifact, so set it [per platform](#platform-specific-urls)
when each platform downloads a different file.

### `checksum_url` {#checksum-url}

The URL of a published checksum source. With it,
[`mise lock`](/dev-tools/mise-lock.html) records checksums for every target
platform, including platforms other than the one you run it on, without
downloading the artifacts. One machine can then write a complete cross-platform
lockfile.

::: v-pre
`checksum_url` is a template, like `url`, and can be set per platform. It can
point at an individual checksum file such as `<artifact>.sha256`, holding only
the hash or `<hash>  <filename>`; at a `SHASUMS` file with one
`<hash>  <filename>` line per platform, where mise finds the artifact's file
name; or at a manifest such as JSON, read with [`checksum_expr`](#checksum-expr).
For checksum files, mise takes the algorithm from the file name (`*.sha512`,
`SHA512SUMS`, `*.md5`, `*.b3`) and uses sha256 otherwise.
:::

```toml
# individual checksum file (one per artifact)
[tools."http:my-tool"]
version = "1.0.0"
url = "https://example.com/releases/my-tool-{{ version }}-{{ os() }}-{{ arch() }}.tar.gz"
checksum_url = "https://example.com/releases/my-tool-{{ version }}-{{ os() }}-{{ arch() }}.tar.gz.sha256"

# SHASUMS (one file lists every platform)
[tools."http:other-tool"]
version = "1.0.0"
url = 'https://example.com/{{ version }}/other_{{ version }}_{{ os(macos="darwin") }}_{{ arch(x64="amd64") }}.zip'
checksum_url = 'https://example.com/{{ version }}/other_{{ version }}_SHASUMS'
```

### `checksum_expr` {#checksum-expr}

An [expr-lang](https://expr-lang.org) expression that extracts the checksum from
a manifest fetched from `checksum_url`. It receives `body` (the manifest),
`version`, `os`, `arch`, `url` (the artifact URL for the target platform) and
`filename`. It must return an `<algorithm>:<hash>` string, so build the prefix
in the expression: prepend a literal when the algorithm is fixed
(`"sha256:" + entry.hash`), or read it from the manifest when it varies
(`entry.algo + ":" + entry.hash`).

```toml
[tools."http:my-tool"]
version = "1.10.0"
checksum_url = "https://example.com/versions.json"
# find the file whose url equals the artifact url, return sha256:<hash>
checksum_expr = '"sha256:" + filter(fromJSON(body)[version + ""].files, { #.url == url })[0].sha256'

[tools."http:my-tool".platforms]
linux-x64 = { url = "https://example.com/my-tool-{{ version }}-linux-x86_64.tar.gz" }
macos-arm64 = { url = "https://example.com/my-tool-{{ version }}-macos-arm64.tar.gz" }
```

::: tip expr-lang gotchas
Write the predicate as `{ #... }` with a space after `{`, because `{#` starts a
Tera comment. To index a map by a value known only at run time, force
evaluation with `[version + ""]`: a bare `[version]` is read as the literal key
`"version"`.
:::

### `size`

The expected size of the download in bytes. The install fails when the size
differs. It catches truncated downloads, but it does not replace a checksum.

```toml
[tools."http:my-tool"]
version = "1.0.0"
url = "https://example.com/releases/my-tool-v1.0.0.tar.gz"
size = "12345678"
```

## Authentication

For HTTP Basic auth, add the host to your netrc file; see the
[`netrc`](/configuration/settings.html#netrc) setting. Use `headers` for a token
or an API key.

### `headers`

Extra request headers for the download and for the
[`version_list_url`](#version-list-url) and [`checksum_url`](#checksum-url)
requests. Use it for a server that needs a bearer token, such as an OCI blob on
`ghcr.io`, or an API key header, such as Artifactory's. Values are templates,
so a secret can come from the environment:

```toml
[tools."http:polaris"]
version = "0.9.2"
# ghcr.io accepts a base64-encoded GitHub token as a bearer token
headers = { Authorization = "Bearer {{ env.GHCR_TOKEN | b64_encode }}" }

[tools."http:polaris".platforms]
linux-x64 = { url = "https://ghcr.io/v2/acme/polaris/blobs/sha256:...", format = "tar.gz" }
macos-arm64 = { url = "https://ghcr.io/v2/acme/polaris/blobs/sha256:...", format = "tar.gz" }
```

mise redacts header values from debug output, and headers do not change where a
tool is installed. A `headers` entry replaces any token mise would otherwise
send to the same host.

mise sends headers only to the host in `url`. When the server redirects to a
different host, port or scheme, mise drops every header for the rest of the
redirect chain, which is what GHCR's signed blob URLs need. Headers survive a
redirect within the same origin.

### `headers_forward` {#headers-forward}

Names the other hosts each header may follow a redirect to. Use it when a server
hands off to another host that also needs the credential, such as a CDN. A
header with no entry is never forwarded, so each secret reaches only the hosts
you list for it:

```toml
[tools."http:my-tool"]
version = "1.0.0"
url = "https://releases.example.com/my-tool-{{ version }}.tar.gz"
headers = { X-Api-Key = "{{ env.RELEASES_KEY }}", Authorization = "Bearer {{ env.RELEASES_TOKEN }}" }
headers_forward = { X-Api-Key = ["cdn.example.com", "*.assets.example.com"] }
```

Here `X-Api-Key` also goes to `cdn.example.com` and to any subdomain of
`assets.example.com`, while `Authorization` stays with `releases.example.com`.

- Each value is a host or a list of hosts: an exact host name, or `*.` and a
  suffix for its subdomains (`*.example.com` matches `a.example.com` but not
  `example.com`). A bare `*` and ports are not accepted.
- Forwarding never steps down to plain HTTP. A chain that is already plain HTTP,
  because `url` is an `http://` URL, may stay that way.
- A header dropped at one hop is not restored by a later hop, even when that
  host is listed.
- Every name must also appear in `headers`, so a typo is an error instead of a
  silent no-op.

## Extracting and naming executables

### `format`

The archive format, such as `tar.gz`, `tar.xz` or `zip`. Without it, mise
detects the format from the final URL after redirects, then from the configured
URL, so an extensionless download endpoint that redirects to a `.tar.gz` file
still works. Set it when neither URL has a useful extension, or to override the
detected format:

```toml
[tools."http:my-tool"]
version = "1.0.0"
url = "https://example.com/releases/my-tool-v1.0.0"
format = "tar.xz"
```

### `strip_components` {#strip-components}

The number of leading directories to remove when extracting an archive:

```toml
[tools."http:my-tool"]
version = "1.0.0"
url = "https://example.com/releases/my-tool-v1.0.0.tar.gz"
strip_components = 1
```

When neither `strip_components` nor `bin_path` is set, mise removes one level by
itself if the archive holds a single top-level directory and no files, as
ripgrep's archives do (`ripgrep-14.1.1-x86_64-unknown-linux-musl/rg`).

### `bin_path` {#bin-path}

The directory, relative to the install directory, that holds the executables,
or where mise places a single downloaded file. It is a template, and setting it
turns off the automatic stripping.

::: v-pre
For an archive laid out as `my-tool-1.0.0/bin/my-tool`, remove the outer
directory as below, or keep it with `bin_path = "my-tool-{{ version }}/bin"`.
:::

```toml
[tools."http:my-tool"]
version = "1.0.0"
url = "https://example.com/releases/my-tool-v1.0.0.tar.gz"
strip_components = 1
bin_path = "bin"
```

When `bin_path` is not set, mise puts `bin/` in the install directory on `PATH`
if it exists, otherwise the `bin/` directory of every immediate subdirectory
that has one, and otherwise the install directory.

### `bin`

The name for a downloaded single file, raw or compressed such as `.gz` or
`.xz`. It can include a directory, such as `bin/my-tool`. mise already removes OS
and architecture suffixes from single-file downloads, so
`docker-compose-linux-x86_64` installs as `docker-compose` with no option. The
http backend ignores `bin` for archives; use [`rename_exe`](#rename-exe) there.

```toml
[tools."http:my-tool"]
version = "1.0.0"
url = "https://example.com/releases/{{ version }}/my-tool-linux-x86_64"
bin = "mt" # install the file as mt instead of my-tool
```

### `rename_exe` {#rename-exe}

Renames an executable after an archive is extracted, for example to give a
kubectl plugin the name kubectl expects. The string form renames the tool's main
executable: the one named after the tool, else one whose name contains it, else
the first executable mise finds in `bin_path`, `bin/` or the install
directory.

```toml
[tools."http:openunison-cli"]
version = "1.0.0"
url = "https://nexus.tremolo.io/repository/openunison-cli/openunison-cli-v{{ version }}-linux.zip"
rename_exe = "kubectl-openunison-cli"
```

To rename several executables from one archive, use the table form. Each key is
an exact file name or a glob, and each value is the new name:

```toml
[tools."http:mytool"]
version = "1.0.0"
url = "https://example.com/mytool-v{{ version }}-linux.zip"
rename_exe = { "mytool-*" = "mytool", "myhelper-*" = "myhelper" }
```

### `windows_script_interpreter` {#windows-script-interpreter}

On Windows, for a raw download such as a script, writes `<file>.cmd` next to it.
The launcher runs the script with this interpreter, so you can call the script
by name. The value must be a plain executable name on `PATH`, such as `node` or
`python`. It has no effect on other platforms, where the script needs its own
shebang line, or on archives and compressed files.

```toml
[tools."http:my-script"]
version = "1.0.0"
url = "https://example.com/my-script-{{ version }}.js"
bin = "my-script"
windows_script_interpreter = "node" # Windows gets my-script.cmd
```

## Sharing extracted files {#shared-extraction}

By default, each install holds its own copy of the extracted files, and
`mise uninstall` and `mise prune` remove them with the version. Set
`shared_extraction = true` to extract each distinct artifact once into
`$MISE_DATA_DIR/http-tarballs/` (by default `~/.local/share/mise/http-tarballs/`)
and link installs to it:

```toml
[tools."http:my-tool"]
version = "1.0.0"
url = "https://example.com/releases/my-tool-v1.0.0.tar.gz"
shared_extraction = true
```

Sharing saves disk space and extraction time when several tools or versions use
the same artifact; mise still downloads the artifact to identify its content.
Changes to shared files, including changes made by a `postinstall` hook, affect
every install that uses them. `mise install --system`, `mise install --shared`
and `mise install-into` destinations always get their own copy.

::: warning The shared store is trusted as is
mise does not re-check files already in `http-tarballs` against the lockfile
checksum when it links an install to them. Protect the store like the binaries
themselves: do not let untrusted jobs, users or refs write to one on a shared
volume or in a restored CI cache. See
[Restored caches and installed tools](/dev-tools/mise-lock.html#restored-caches)
for why mise cannot validate this for you.
:::

`mise prune`, `mise cache prune` and `mise cache clear` leave `http-tarballs`
alone, because installs may still link to it. Installs made by older mise
versions may also link into it. To give an install its own copy, remove
`shared_extraction` and run `mise install --force <tool>@<version>`. Changing the
option alone does not change an installed version.

To reclaim the space, give every such install its own copy or uninstall it,
check that nothing links into the directory, then delete it:

```sh
find ~/.local/share/mise/installs -lname '*http-tarballs*' # should print nothing
rm -rf ~/.local/share/mise/http-tarballs
```

Use your [data directory](/directories.html) if it is not the default.

Implementation: [`src/backend/http.rs`](https://github.com/jdx/mise/blob/main/src/backend/http.rs).
