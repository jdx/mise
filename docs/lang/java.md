# Java

Like `sdkman`, `mise` can manage multiple versions of Java on the same system.

## Usage

Select a JDK vendor and release for the current project:

```sh
mise use java@temurin-21
mise exec -- java -version
mise exec -- javac -version
```

Use `mise use -g java@temurin-21` for a personal default. The vendor prefix makes
the project's distribution choice explicit.

You can also install a JDK from a different vendor. To get the latest version from a vendor, use the
vendor prefix.

```sh
mise use java@temurin        # latest version from Temurin
mise use java@temurin-21
mise use java@zulu-21
mise use java@corretto-21
```

See available versions with `mise ls-remote java`.

::: info Vendor selection
Unqualified versions such as `java@21` use
[`java.shorthand_vendor`](/configuration/settings.html#java.shorthand_vendor),
which defaults to `openjdk`. Vendor distributions have different update and
support policies. Use a vendor-qualified request when the project depends on a
particular distribution.
:::

These instructions use mise's built-in java support. An installed external
plugin with the same name can change the behavior; use `mise plugins ls` to
check for overrides. See the [core implementation](https://github.com/jdx/mise/blob/main/src/plugins/core/java.rs)
for backend details.

## JAVA_HOME

mise sets `JAVA_HOME` for commands run with `mise exec`, tasks, and activated
shells. [Shell activation](/cli/activate.html) updates the parent shell itself;
running a shim does not export `JAVA_HOME` back into that parent shell.

If `JAVA_HOME` appears stuck on an old version after changing your `mise.toml`, try:

```sh
cd . # triggers mise hook-env to re-evaluate
echo $JAVA_HOME
```

If you use an IDE that reads `JAVA_HOME` at startup, you may need to restart it after switching Java versions. For non-interactive environments (CI, scripts), use `mise exec` or `mise run`, which always set up the full environment.

## macOS JAVA_HOME Integration

Some applications on macOS rely on `/usr/libexec/java_home` to find installed Java runtimes.

If the selected distribution includes a macOS `Contents` bundle, register it with
macOS. First inspect the installation selected for this directory:

```sh
mise where java
```

Then, in a POSIX shell:

```sh
mise_java_home="$(mise where java)"
if test -d "$mise_java_home/Contents"; then
  sudo mkdir -p /Library/Java/JavaVirtualMachines/mise-java.jdk
  sudo ln -s "$mise_java_home/Contents" /Library/Java/JavaVirtualMachines/mise-java.jdk/Contents
fi
/usr/libexec/java_home -V
```

Run the link command only when `Contents` exists and the destination is not
already registered. Not all distributions include this bundle. The link points
to the selected installation; it does not automatically follow future upgrades.

## `.java-version` and `.sdkmanrc` files support

Enable discovery of `.java-version` and `.sdkmanrc` explicitly:

```sh
mise settings add idiomatic_version_file_enable_tools java
```

A conflicting Java declaration in `mise.toml` takes precedence. See
[idiomatic version files](/configuration.html#idiomatic-version-files).

For `.sdkmanrc` files, mise tries to map the vendor and version to the appropriate version
string. For example, the version `20.0.2-tem` is mapped to `temurin-20.0.2`. Due to Azul's Zulu
versioning, the version `11.0.12-zulu` is mapped to the major version `zulu-11`.

Not all vendors available in [sdkman](https://sdkman.io/jdks) are supported by mise.
The following vendors are NOT supported: `bsg` (Bisheng), `graal` (GraalVM), `nik` (Liberica NIK).

### Using unsupported versions

For a JDK already installed by SDKMAN or another source, point mise at its home
directory instead of creating internal cache entries or modifying the JDK:

```toml [mise.toml]
[tools]
java = { path = "/path/to/jdk-home" }
```

The directory must contain `bin/java` and, for a full JDK, `bin/javac`. For a
macOS `.jdk` bundle, this is usually its `Contents/Home` directory. Check with
`mise exec -- java -version`.

Alternatively, register a local installation under a name with
[`mise link`](/cli/link.html), then select it with `mise use`:

```sh
mise link java@local /path/to/jdk-home
mise use java@local
```

mise uses this installation in place; updates remain the responsibility of the
source that installed it.

## Tool Options

The following [tool-options](/dev-tools/#tool-options) are available for the `java` backend.
These options go in the `[tools]` section in `mise.toml`.

### `install_env`

Set environment variables for install-time commands run by the core `java` backend:

```toml
[tools]
java = { version = "latest", install_env = { JAVA_TOOL_OPTIONS = "-Djava.net.useSystemProxies=true" } }
```

### `release_type`

The `release_type` option specifies the type of release to install. The following values
are supported:

- `ga` (default): General Availability release
- `ea`: Early Access release

```toml
[tools]
"java" = { version = "openjdk-21", release_type = "ea" }
```

## Gradle toolchains detection

Run Gradle through mise so it inherits the selected `JAVA_HOME`:

```sh
mise exec -- ./gradlew -q javaToolchains
```

This assumes the project has a Gradle wrapper and JVM build configuration. The
report shows which JDKs Gradle detects and how it found them.

To expose the selected JDK as an explicit toolchain candidate, add:

```properties [gradle.properties]
org.gradle.java.installations.fromEnv=JAVA_HOME
```

For multiple JDKs, Gradle also accepts a comma-separated list of installation
homes in `org.gradle.java.installations.paths`. It does not recursively search
those directories. Use actual JDK homes, not mise's entire `installs/java`
directory; see [Gradle's custom toolchain locations](https://docs.gradle.org/current/userguide/toolchains.html#sec:custom_loc).

The build's toolchain requirements still determine which candidate Gradle uses.
After changing toolchain configuration, stop the existing daemon with
`mise exec -- ./gradlew --stop` before checking again.

## Settings

<script setup>
import Settings from '/components/settings.vue';
</script>
<Settings child="java" :level="3" />
