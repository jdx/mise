---
---

# Shims

While the PATH design of mise works great in most cases, there are some situations where shims are
preferable. One example is when calling mise binaries from an IDE.

To support this, mise does have a shim dir that can be used. It's located at `~/.local/share/mise/shims`.

```sh
$ mise use -g node@20
$ npm install -g prettier@3.1.0
$ mise reshim # may be required if new shims need to be created after installing packages
$ ~/.local/share/mise/shims/node -v
v20.0.0
$ ~/.local/share/mise/shims/prettier -v
3.1.0
```
