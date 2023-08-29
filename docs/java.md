# Java in rtx

The following are instructions for using the java rtx core plugin. This is used when there isn't a 
git plugin installed named "java".

If you want to use [asdf-java](https://github.com/halcyon/asdf-java)
or [rtx-java](https://github.com/rtx-plugins/rtx-java)
then use `rtx plugins install java GIT_URL`.

The code for this is inside the rtx repository at
[`./src/plugins/core/java.rs`](https://github.com/jdx/rtx/blob/main/src/plugins/core/java.rs).

## Usage

The following installs the latest version of java-openjdk-17.x (if some version of openjdk-17.x is 
not already installed) and makes it the global default:

```sh-session
$ rtx use -g java@openjdk-17
$ rtx use -g java@17         # alternate shorthands for openjdk-only
```

See available versions with `rtx ls-remote java`.

## macOS JAVA_HOME Integration

Some applications in macOS rely on `/usr/libexec/java_home` to find installed Java runtimes.

To integrate an installed Java runtime with macOS run the following commands for the proper version (e.g. openjdk-20).

```sh-session
$ sudo mkdir /Library/Java/JavaVirtualMachines/openjdk-20.jdk
$ sudo ln -s ~/.local/share/rtx/installs/java/openjdk-20/Contents /Library/Java/JavaVirtualMachines/openjdk-20.jdk/Contents
```

The distribution from  Azul Systems does support the integration but the symlink target location will differ from the example above (e.g `~/.local/share/rtx/installs/java/zulu-11.64.190/zulu-11.jdk/Contents`).

> Note: Not all distributions of the Java SDK support this integration (e.g liberica).
