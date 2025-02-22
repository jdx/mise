# Demo

The following demo shows how to install and use `mise` to manage multiple versions of `node` on the same system.
Note that calling `which node` gives us a real path to node, not a shim.

It also shows that you can use `mise` to install and many other tools such as `jq`, `terraform`, or `go`.

<video style="max-width: 100%; height: auto;" controls="controls" src="https://github.com/user-attachments/assets/e64896d9-183d-4a8b-a8b2-7de0d1ed593f" />

## Transcript

```sh
# We will start by installing node@lts and make it the global default
mise use --global node@lts
# mise node@22.13.1 ✓ installed

node -v
# v22.13.1

which node
# /root/.local/share/mise/installs/node/22.13.1/bin/node

# Note that we get back the path to the real node here, not a shim

# we can also install other tools with mise.
# For example, we will install terraform, jq, and go
mise use -g terraform jq go
# mise jq@1.7.1 ✓ installed
# mise terraform@1.10.5 ✓ installed
# mise go@1.23.6 ✓ installed
# mise ~/.config/mise/config.toml tools: go@1.23.6, jq@1.7.1, terraform@1.10.5

terraform -v
# Terraform v1.10.5
jq --version
# jq-1.7
go version
# go version go1.23.6 linux/amd64

mise ls
# Tool       Version  Source                      Requested
# go         1.23.6   ~/.config/mise/config.toml  latest
# jq         1.7.1    ~/.config/mise/config.toml  latest
# node       22.13.1  ~/.config/mise/config.toml  lts
# terraform  1.10.5   ~/.config/mise/config.toml  latest

# Lets enter a project directory where we will set up node@21
cd myproj
mise use node@21
# mise node@21.7.3 ✓ installed
node -v
# v21.7.3

# As expected, `node -v` is now v21.x
cat mise.toml
# [tools]
# node = "21"

# We will leave this directory. The node version will revert to the global LTS version
cd ..
node -v
# v22.13.1
```
