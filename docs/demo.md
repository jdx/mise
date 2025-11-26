# Demo

The following demo shows:

- how to use `mise exec` to run a command with a specific version of a tool
- how you can use `mise` to install and many other tools such as `jq`, `terraform`, or `go`.
- how to use `mise` to manage multiple versions of `node` on the same system.

<video style="max-width: 100%; height: auto;" controls="controls" src="./tapes/demo.mp4" />

## Transcript

`mise exec <tool> -- <command>` allows you to run any tools with mise

```shell
mise exec node@24 -- node -v
# mise node@24.x.x ✓ installed
# v24.x.x
```

node is only available in the mise environment, not globally

```shell
node -v
# bash: node: command not found
```

---

Here is another example where we run terraform with `mise exec`

```shell
mise exec terraform -- terraform -v
# mise terraform@1.11.3 ✓ installed
# Terraform v1.11.3
```

---

`mise exec` is great for running one-off commands, however it can be convenient to activate mise. When activated, mise will automatically update your `PATH` to include the tools you have installed, making them available directly.

We will start by installing node@lts and make it the global default

```shell
mise use --global node@lts
# v22.14.0
```

```shell
node -v
# v22.14.0
```

```shell
which node
# /root/.local/share/mise/installs/node/22.14.0/bin/node
```

Note that we get back the path to the real node here, not a shim.

---

We can also install other tools with mise. For example, we will install terraform, jq, and go

```shell
mise use -g terraform jq go
# mise jq@1.7.1 ✓ installed
# mise terraform@1.11.3 ✓ installed
# mise go@1.24.1 ✓ installed
# mise ~/.config/mise/config.toml tools: go@1.24.1, jq@1.7.1, terraform@1.11.3
```

```shell
terraform -v
# Terraform v1.11.3
```

```shell
jq --version
# jq-1.7
```

```shell
go version
# go version go1.24.1 linux/amd64
```

```shell
mise ls
# Tool       Version  Source                      Requested
# go         1.24.1   ~/.config/mise/config.toml  latest
# jq         1.7.1    ~/.config/mise/config.toml  latest
# node       22.14.0  ~/.config/mise/config.toml  lts
# terraform  1.11.3   ~/.config/mise/config.toml  latest
```

---

Let's enter a project directory where we will set up node@23

```shell
cd myproj
mise use node@23 pnpm@10
# mise node@23.10.0 ✓ installed
# mise pnpm@10.7.0 ✓ installed
```

```shell
node -v
# v23.10.0
pnpm -v
# 10.7.0
```

As expected, `node -v` is now v23.x

```shell
cat mise.toml
# [tools]
# node = "23"
# pnpm = "10"
```

We will leave this directory. The node version will revert to the global LTS version

```shell
cd ..
node -v
# v22.14.0
```
