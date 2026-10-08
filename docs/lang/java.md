---
description: "Install JDKs from Temurin, Zulu, Corretto and other vendors with mise, and set JAVA_HOME."
---

# Java

mise installs JDKs from many vendors and sets `JAVA_HOME` to the one your
project selects.

## Quick start

Select a JDK vendor and release for the current project:

```sh
mise use java@temurin-25
mise exec -- java -version
mise exec -- javac -version
```

Use `mise use -g java@temurin-25` for a personal default. Naming the vendor
makes the project's distribution choice explicit.

## Choosing a vendor and version {#choosing-a-version}

| Request                  | Selects                                                                                   |
| ------------------------ | ----------------------------------------------------------------------------------------- |
| `java@temurin-25`        | The newest Temurin 25 build                                                               |
| `java@temurin`           | The newest Temurin build of any major                                                     |
| `java@zulu-21`           | The newest Zulu 21 build; `corretto-21` and other vendors work the same way               |
| `java@temurin-jre-25`    | The newest Temurin 25 JRE, without the compiler                                           |
| `java@oracle-graalvm-25` | The newest Oracle GraalVM for JDK 25                                                      |
| `java@25`                | The newest 25 build from [`java.shorthand_vendor`](/lang/java.html#java.shorthand_vendor) |
| `java@lts`               | The current LTS major (25) from `java.shorthand_vendor`                                   |

A request without a vendor uses `java.shorthand_vendor`, which defaults to
`temurin`, so `java@21` resolves to the newest Temurin 21 build. Set it to
`openjdk` for the builds published on jdk.java.net, which stop at the last
update jdk.java.net published for a release (`java@21` is then 21.0.2). Name a
vendor in shared config so every machine resolves the same build.

An OpenJDK build still installed when the default changes keeps working, and
plain `mise lock` locks it again under the new default, so machines without it
install the same OpenJDK build from the lockfile. Without the installed build,
a lockfile entry recorded with `shorthand_vendor = "openjdk"` no longer
matches: `mise install --locked` fails, and plain `mise lock` or `mise install`
replaces the entry with the newest matching Temurin build. Set
`java.shorthand_vendor = "openjdk"` to keep the old pin. `mise lock --bump java`
always moves to the newest matching Temurin build. Temurin publishes no builds
for some releases, such as Java 12, so request those with a vendor, for example
`openjdk-12`.

Vendor names also carry variants, such as `temurin-jre`, `zulu-javafx`,
`zulu-crac` and `liberica-nik-openjdk`. On Alpine and other musl systems, mise
lists the vendors' musl builds under the usual names. The full list has
thousands of entries; filter it with a prefix:

```sh
mise ls-remote java temurin-25
```

## Version files

mise can read `.java-version` and `.sdkmanrc`. Enable them for Java:

```sh
mise settings add idiomatic_version_file_enable_tools java
```

This changes your global config. Add `--local` to enable it in the project's
`mise.toml` instead, so teammates get the same behavior. A Java version in
`mise.toml` takes precedence over these files. See
[idiomatic version files](/dev-tools/versions.html#idiomatic-version-files).

A version without a vendor in `.java-version`, such as `21`, uses
`java.shorthand_vendor` like any other request.

For `.sdkmanrc`, mise maps SDKMAN's vendor identifiers to mise names. For
example, `21.0.9-tem` becomes `temurin-21.0.9`, and `21.0.9-amzn` becomes
`corretto-21.0.9`. Because of Azul's version numbering, `11.0.12-zulu` maps to
the major version only, `zulu-11`.

mise cannot translate these SDKMAN identifiers: `bisheng` (Huawei BiSheng,
formerly `bsg`), `graal` (Oracle GraalVM), `jbr` (JetBrains Runtime) and `nik`
(Liberica NIK). Write the mise name in `mise.toml` instead, such as
`java = "oracle-graalvm-21"`, `java = "jetbrains-21"` or
`java = "liberica-nik-openjdk21"`. BiSheng is not available through mise.

## Use a JDK installed elsewhere {#using-unsupported-versions}

To use a JDK that SDKMAN, a package manager or an installer put on the machine,
point mise at its home directory:

```toml [mise.toml]
[tools]
java = { path = "/path/to/jdk-home" }
```

The directory must contain `bin/java` and, for a full JDK, `bin/javac`. For a
macOS `.jdk` bundle, this is usually its `Contents/Home` directory. Check it
with `mise exec -- java -version`.

Alternatively, register the installation under a name with
[`mise link`](/cli/link.html), then select it with `mise use`:

```sh
mise link java@local /path/to/jdk-home
mise use java@local
```

mise uses these installations in place. Updates remain the job of whatever
installed them.

## `JAVA_HOME`

mise sets `JAVA_HOME` to the selected JDK for commands run with
[`mise exec`](/cli/exec.html), for tasks, and in shells where mise is
activated. With [shims](/dev-tools/shims.html) alone, only the shimmed `java`
process sees it. Your shell, your IDE and build tools that read `JAVA_HOME`
directly do not.

If `JAVA_HOME` still points at an old JDK, run `mise env | grep JAVA_HOME`. If
mise reports the new path, something else is overriding it: usually a
`JAVA_HOME` export in your shell startup file, or a shims-only setup. An IDE
that reads `JAVA_HOME` at startup needs a restart after you switch versions.

## Registering a JDK with macOS {#macos-java-home-integration}

Some macOS apps find Java through `/usr/libexec/java_home`. If the selected JDK
has a `Contents` bundle, register it:

```sh
mise_java_home="$(mise where java)"
if test -d "$mise_java_home/Contents"; then
  sudo mkdir -p /Library/Java/JavaVirtualMachines/mise-java.jdk
  sudo ln -s "$mise_java_home/Contents" /Library/Java/JavaVirtualMachines/mise-java.jdk/Contents
fi
/usr/libexec/java_home -V
```

Not every distribution includes the bundle. Run the link command only when the
destination is not already registered. The link points to the selected
installation and does not follow later upgrades.

## Gradle toolchains {#gradle-toolchains-detection}

Run Gradle through mise so it inherits the selected `JAVA_HOME`, then list the
JDKs Gradle detects and how it found them:

```sh
mise exec -- ./gradlew -q javaToolchains
```

This assumes the project has a Gradle wrapper and a JVM build. To offer the
selected JDK as an explicit toolchain candidate, add:

```properties [gradle.properties]
org.gradle.java.installations.fromEnv=JAVA_HOME
```

For several JDKs, Gradle also accepts a comma-separated list of JDK homes in
`org.gradle.java.installations.paths`. It does not search those directories
recursively, so list actual JDK homes, not mise's whole `installs/java`
directory; see
[Gradle's custom toolchain locations](https://docs.gradle.org/current/userguide/toolchains.html#sec:custom_loc).

The build's toolchain requirements still decide which candidate Gradle uses.
After changing toolchain configuration, stop the running daemon with
`mise exec -- ./gradlew --stop` before checking again.

## How mise installs Java

mise reads a catalog of vendor builds from `mise-java.jdx.dev`, downloads the
selected vendor's archive, and checks it against the catalog's checksum when
there is one. It then runs `java -version`. On macOS, mise keeps the bundle's
`Contents` directory and prints the commands that
[register the JDK with macOS](#macos-java-home-integration).

An installed plugin named `java` takes precedence over the built-in
installer. If mise behaves differently from this page, check
[`mise plugins ls`](/cli/plugins/ls.html) and see
[selecting another implementation](/core-tools.html#selecting-another-implementation).

## Tool options

### `release_type`

Selects general-availability builds (`ga`, the default) or early-access builds
(`ea`):

```toml [mise.toml]
[tools]
java = { version = "openjdk-28", release_type = "ea" }
```

Generic options such as `install_env` and `postinstall` work as described in
[tool options](/dev-tools/#tool-options). `install_env` reaches the
`java -version` check and `postinstall` commands, not the download. To download
through a proxy, set `https_proxy` in the environment that runs mise (see the
[FAQ](/faq.html#how-do-i-use-mise-with-http-proxies)).

## Settings

<script setup>
import Settings from '/components/settings.vue';
</script>
<Settings child="java" :level="3" />
