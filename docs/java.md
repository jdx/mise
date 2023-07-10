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

```sh-session
$ rtx use -g java@openjdk-17
$ rtx use -g java@17         # alternate shorthands for openjdk-only
```

See available versions with `rtx ls-remote java`.
