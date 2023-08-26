# Java in rtx

The following are instructions for using the java rtx core plugin. This is used when there isn't a 
git plugin installed named "java".

If you want to use [asdf-java](https://github.com/halcyon/asdf-java)
or [rtx-java](https://github.com/rtx-plugins/rtx-java)
then use `rtx plugins install java GIT_URL`.

The code for this is inside the rtx repository at
[`./src/plugins/core/java.rs`](https://github.com/jdxcode/rtx/blob/main/src/plugins/core/java.rs).

## Usage

The following installs the latest version of java-openjdk-17.x (if some version of openjdk-17.x is 
not already installed) and makes it the global default:

```sh
rtx use -g java@openjdk-17
rtx use -g java@17         # alternate shorthands for openjdk-only
```

See available versions with `rtx ls-remote java`.

## Configuration

- `RTX_JAVA_MACOS_INTEGRATION` [bool]: enables macOS JAVA_HOME integration, defaults to false

## macOS JAVA_HOME Integration

Some applications in macOS rely on `/usr/libexec/java_home` to find installed Java runtimes.

The environment variable `RTX_JAVA_MACOS_INTEGRATION` enables the integration during the installation. On removal
the integration will be cleaned up regardless of the value of the environment variable.

Since the integration relies on symlinks within `/Library/Java/JavaVirtualMachines`, administrative privileges are
required when using this feature.

```sh
sudo RTX_JAVA_MACOS_INTEGRATION=true rtx install java@openjdk-20
```

> Note: Not all distributions of Java SDK support this integration (e.g liberica). The integration will be simply
> ignored in such cases even if the environment variable is set to true.
