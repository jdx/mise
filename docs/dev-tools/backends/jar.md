# Jar Backend

You may install runnable JAR files as tools using the `jar` backend. mise downloads the jar
(from a Maven repository or a direct URL), installs it under a stable versionless path, and
generates a `java -jar` launcher script so the tool behaves like any other mise-managed
binary — no hand-written wrapper scripts required.

The code for this is inside of the mise repository at [`./src/backend/jar.rs`](https://github.com/jdx/mise/blob/main/src/backend/jar.rs).

::: warning
The `jar` backend is experimental and requires `MISE_EXPERIMENTAL=1` or
`experimental = true` in your config.
:::

## Dependencies

The launcher runs the jar with `java` — from `$JAVA_HOME` if set, otherwise from `PATH`.
The backend declares a dependency on mise's `java` tool, so a `java` entry in your config
is installed first and is available to the launcher.

## Usage

### Maven coordinates

Tools published to a Maven repository are addressed as `jar:<groupId>/<artifactId>`:

```sh
mise use -g jar:com.facebook/ktfmt[classifier=with-dependencies]@0.64
```

```toml
[tools]
"jar:com.facebook/ktfmt" = { version = "0.64", classifier = "with-dependencies" }
```

The jar is downloaded from Maven Central by default
(`https://repo1.maven.org/maven2/<group path>/<artifact>/<version>/<artifact>-<version>[-<classifier>].jar`),
and `mise ls-remote`/`mise upgrade` list versions from the repository's `maven-metadata.xml`.

### Direct URL

Jars only published as e.g. GitHub release assets can use any tool name plus a templated
`url` option:

```toml
[tools."jar:elasticmq"]
version = "1.7.1"
url = "https://github.com/softwaremill/elasticmq/releases/download/v{{version}}/elasticmq-server-all-{{version}}.jar"
```

In URL mode there is no version listing unless you also provide `version_list_url`
(see below).

## Install Layout

Each install contains the jar at a stable, versionless path plus generated launchers:

```text
<install path>/
├── lib/<bin>.jar     # the downloaded jar, renamed to a stable name
└── bin/
    ├── <bin>         # POSIX sh launcher (exec java ... -jar ../lib/<bin>.jar "$@")
    └── <bin>.cmd     # Windows cmd launcher
```

`<bin>` defaults to the Maven artifactId (or the tool name in URL mode) and can be
overridden with the `bin` option. Scripts that need the jar itself (e.g. to pass their own
JVM flags) can reference `$(mise where jar:<tool>)/lib/<bin>.jar` without knowing the
version.

## JVM Options

Two ways to pass flags to the JVM (not the program):

- `java_args` tool option — baked into the launcher, e.g. `java_args = "-Xmx512m"`. The
  value is inserted verbatim before `-jar`.
- `JAVA_OPTS` environment variable — read by the launcher at run time:

  ```sh
  JAVA_OPTS="-Dnode-address.port=9325" mise x -- elasticmq
  ```

Program arguments are passed through normally after the jar.

## Tool Options

The following [tool-options](/dev-tools/#tool-options) are available for the `jar`
backend—these go in `[tools]` in `mise.toml`.

### `repository`

Maven repository base URL for coordinate-style tools. Defaults to
`https://repo1.maven.org/maven2`:

```toml
[tools]
"jar:com.example/internal-tool" = { version = "1.2.3", repository = "https://nexus.example.com/repository/releases" }
```

### `classifier`

Maven artifact classifier, commonly used for fat/uber jars
(`with-dependencies`, `jar-with-dependencies`, `all`, ...):

```toml
[tools]
"jar:com.squareup.wire/wire-compiler" = { version = "5.5.0", classifier = "jar-with-dependencies" }
```

### `url`

Direct download URL, overriding Maven coordinate resolution. Supports the same templating
as the [http backend](/dev-tools/backends/http) (<code v-pre>{{version}}</code>, etc.).

### `bin`

Name of the generated launcher (and the installed `lib/<bin>.jar`):

```toml
[tools]
"jar:com.squareup.wire/wire-compiler" = { version = "5.5.0", classifier = "jar-with-dependencies", bin = "wire" }
```

### `java_args`

JVM arguments baked into the launcher, inserted verbatim before `-jar`:

```toml
[tools]
"jar:elasticmq" = { version = "1.7.1", url = "...", java_args = "-Dconfig.file=custom.conf" }
```

### `checksum` and `size`

Verify the downloaded jar:

```toml
[tools]
"jar:com.facebook/ktfmt" = { version = "0.64", classifier = "with-dependencies", checksum = "sha256:5e7eb28a0b2006d1cefbc9213bfc70a3191825e69ff8ea23dd5e5e2e1c39cb6a" }
```

With [lockfiles](/dev-tools/mise-lock) enabled, checksums and sizes are recorded and
verified automatically. Jar artifacts are platform-independent, so `mise lock` resolves
the same URL and checksum for every target platform.

### `version_list_url`, `version_regex`, `version_json_path`, `version_expr`

Version listing for URL-mode tools, identical to the
[http backend options](/dev-tools/backends/http#version-listing-options):

```toml
[tools."jar:elasticmq"]
version = "1.7.1"
url = "https://github.com/softwaremill/elasticmq/releases/download/v{{version}}/elasticmq-server-all-{{version}}.jar"
version_list_url = "https://api.github.com/repos/softwaremill/elasticmq/releases"
version_regex = '"tag_name": ?"v([^"]+)"'
```
