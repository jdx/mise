# Java

The following are instructions for using the java mise core plugin. This is used when there isn't a
git plugin installed named "java".

If you want to use [asdf-java](https://github.com/halcyon/asdf-java)
then use `mise plugins install java GIT_URL`.

The code for this is inside the mise repository at
[`./src/plugins/core/java.rs`](https://github.com/jdx/mise/blob/main/src/plugins/core/java.rs).

## Usage

The following installs the latest version of openjdk-21.x (if some version of openjdk-21.x is
not already installed) and makes it the global default:

```sh
mise use -g java@openjdk-21
mise use -g java@21         # alternate shorthands for openjdk-only
```

See available versions with `mise ls-remote java`.

::: warning
Note that shorthand versions (like `21` in the example) use `OpenJDK` as the vendor.
The OpenJDK versions will only be updated for a 6-month period. Updates and security patches will not be available after this short period. This also applies for LTS versions. Also see <https://whichjdk.com> for more information.
:::

## Tool Options

The following [tool-options](/dev-tools/#tool-options) are available for the `java` backend—these
go in `[tools]` in `mise.toml`.

### `release_type`

The `release_type` option allows you to specify the type of release to install. The following values
are supported:

- `ga` (default): General Availability release
- `ea`: Early Access release

```toml
[tools]
"java" = { version = "openjdk-21", release_type = "ea" }
```

## macOS JAVA_HOME Integration

Some applications in macOS rely on `/usr/libexec/java_home` to find installed Java runtimes.

To integrate an installed Java runtime with macOS run the following commands for the proper
version (e.g. openjdk-21).

```sh
sudo mkdir /Library/Java/JavaVirtualMachines/openjdk-21.jdk
sudo ln -s ~/.local/share/mise/installs/java/openjdk-21/Contents /Library/Java/JavaVirtualMachines/openjdk-21.jdk/Contents
```

> Note: Not all distributions of the Java SDK support this integration (e.g liberica).

## Idiomatic version files

The Java core plugin supports the idiomatic version files `.java-version` and `.sdkmanrc`.

For `.sdkmanrc` files, mise will try to map the vendor and version to the appropriate version
string. For example, the version `20.0.2-tem` will be mapped to `temurin-20.0.2`. Due to Azul's Zulu
versioning, the version `11.0.12-zulu` will be mapped to the major version `zulu-11`. Not all
vendors available in SDKMAN are supported by mise. The following vendors are NOT supported: `bsg` (
Bisheng), `graal` (GraalVM), `nik` (Liberica NIK).

In case an unsupported version of java is needed, some manual work is required:

1. Download the unsupported version to a directory (e.g `~/.sdkman/candidates/java/21.0.1-open`)
2. symlink the new version:

```sh
ln -s ~/.sdkman/candidates/java/21.0.1-open ~/.local/share/mise/installs/java/21.0.1-open
```

3. If on Mac:

```sh
mkdir ~/.local/share/mise/installs/java/21.0.1-open/Contents
mkdir ~/.local/share/mise/installs/java/21.0.1-open/Contents/MacOS

ln -s ~/.sdkman/candidates/java/21.0.1-open ~/.local/share/mise/installs/java/21.0.1-open/Contents/Home
cp ~/.local/share/mise/installs/java/21.0.1-open/lib/libjli.dylib ~/.local/share/mise/installs/java/21.0.1-open/Contents/MacOS/libjli.dylib
```

4. Don't forget to make sure the cache is blocked and valid, by making sure an **empty** directory **exists** for your version in the [mise cache](https://mise.jdx.dev/directories.html#cache-mise):
   e.g.

```sh
$ ls -R $MISE_CACHE_DIR/java
21.0.1-open

mise/java/21.0.1-open:
```
