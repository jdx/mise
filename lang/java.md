# Java

The following are instructions for using the java mise core plugin. This is used when there isn't a
git plugin installed named "java".

If you want to use [asdf-java](https://github.com/halcyon/asdf-java)
or [rtx-java](https://github.com/mise-plugins/rtx-java)
then use `mise plugins install java GIT_URL`.

The code for this is inside the mise repository at
[`./src/plugins/core/java.rs`](https://github.com/jdx/mise/blob/main/src/plugins/core/java.rs).

## Usage

The following installs the latest version of java-openjdk-17.x (if some version of openjdk-17.x is
not already installed) and makes it the global default:

```sh
mise use -g java@openjdk-17
mise use -g java@17         # alternate shorthands for openjdk-only
```

See available versions with `mise ls-remote java`.

## macOS JAVA_HOME Integration

Some applications in macOS rely on `/usr/libexec/java_home` to find installed Java runtimes.

To integrate an installed Java runtime with macOS run the following commands for the proper version (e.g. openjdk-20).

```sh
sudo mkdir /Library/Java/JavaVirtualMachines/openjdk-20.jdk
sudo ln -s ~/.local/share/mise/installs/java/openjdk-20/Contents /Library/Java/JavaVirtualMachines/openjdk-20.jdk/Contents
```

> Note: Not all distributions of the Java SDK support this integration (e.g liberica).
