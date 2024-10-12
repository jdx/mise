# Vfox Backend <Badge type="warning" text="experimental" />

[Vfox](https://github.com/version-fox/vfox) plugins may be used in mise as an alternative for asdf
plugins. On Windows, only vfox plugins are supported since asdf plugins require POSIX compatibility.

The code for this is inside of the mise repository at [`./src/backend/vfox.rs`](https://github.com/jdx/mise/blob/main/src/backend/vfox.rs).

## Dependencies

No dependencies are required for vfox. Vfox lua code is read via a lua interpreter built into mise.

## Usage

The following installs the latest version of cmake and sets it as the active version on PATH:

```sh
$ mise use -g vfox:cmake
$ cmake --version
cmake version 3.21.3
```

Alternatively, you can specify the GitHub repo:

```sh
$ mise use -g vfox:version-fox/vfox-cmake
$ cmake --version
cmake version 3.21.3
```

The version will be set in `~/.config/mise/config.toml` with the following format:

```toml
[tools]
"vfox:cmake" = "latest"
```

## Default plugin backend

If you'd like to use vfox plugins by default like on Windows, set the following settings:

```sh
mise settings set asdf false
mise settings set vfox true
```

Now you can list available plugins with `mise registry`:

```sh
$ mise registry | grep vfox:
clang          vfox:version-fox/vfox-clang
cmake          vfox:version-fox/vfox-cmake
crystal        vfox:yanecc/vfox-crystal
dart           vfox:version-fox/vfox-dart
dotnet         vfox:version-fox/vfox-dotnet
elixir         vfox:version-fox/vfox-elixir
etcd           vfox:version-fox/vfox-etcd
flutter        vfox:version-fox/vfox-flutter
golang         vfox:version-fox/vfox-golang
gradle         vfox:version-fox/vfox-gradle
groovy         vfox:version-fox/vfox-groovy
julia          vfox:ahai-code/vfox-julia
kotlin         vfox:version-fox/vfox-kotlin
kubectl        vfox:ahai-code/vfox-kubectl
maven          vfox:version-fox/vfox-maven
mongo          vfox:yeshan333/vfox-mongo
php            vfox:version-fox/vfox-php
protobuf       vfox:ahai-code/vfox-protobuf
scala          vfox:version-fox/vfox-scala
terraform      vfox:enochchau/vfox-terraform
vlang          vfox:ahai-code/vfox-vlang
```

And they will be installed when running commands such as `mise use -g cmake` without needing to
specify `vfox:cmake`.
