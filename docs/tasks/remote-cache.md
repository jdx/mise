---
description: "Share task cache results between CI and developer machines through a self-hosted cache server."
---

# Remote task cache <Badge type="warning" text="experimental" />

A remote cache lets CI jobs and developer machines reuse each other's task
results. mise looks up entries on the server when the local
[artifact cache](/tasks/caching.html#enable-artifact-caching) misses, and
uploads new results from trusted CI jobs. Get the local cache working for your
tasks before you add a server.

::: warning Experimental
The remote cache is part of the artifact cache and requires `experimental = true`.
:::

## Choose a server

mise speaks version 1 of the [remote cache protocol](/tasks/remote-cache-protocol.html)
over HTTPS. Any server that implements it works.

The reference server is `mbx-cache`, the cache server for Mr Boxington. Its
source is [`jdx/mr-boxington-cache`](https://github.com/jdx/mr-boxington-cache).
It stores blobs on the filesystem or in S3-compatible storage, keeps metadata in
memory or in PostgreSQL, grants per-namespace read and write access to static
tokens and OIDC identities, and ships a Docker Compose file and a Helm chart.
It has no release yet, so install it from the repository:

```sh
cargo install --git https://github.com/jdx/mr-boxington-cache
```

The `mise-cache` 0.1 releases from the same repository, published when it was
named `jdx/mise-cache`, expect `mise-cache-*` request headers instead of the
`mbx-cache-*` headers that mise sends, so they reject mise's requests.

## Configure mise

Point mise at the server and choose a namespace:

```toml
[settings]
experimental = true
task.cache.remote_url = "https://cache.example.com/"
task.cache.remote_namespace = "acme/widgets"
```

- [`task.cache.remote_url`](/configuration/settings.html#task.cache.remote_url)
  is the server's base URL. mise appends `v1/...` to it.
- [`task.cache.remote_namespace`](/configuration/settings.html#task.cache.remote_namespace)
  is required. The server uses it to keep entries apart and to decide who may
  read and write them. It is an identifier, such as a repository name, not a
  secret or a credential.
- [`task.cache.remote_mode`](/configuration/settings.html#task.cache.remote_mode)
  limits what mise does with the server: `read-write` (the default),
  `read-only`, or `write-only`.

These settings can live in the project's `mise.toml`. Credentials cannot; see
[Credentials](#credentials).

Use a separate namespace wherever one group of writers should not be able to
affect another group's entries, and when a change to how tasks behave could
collide with entries written under a different policy.

With a remote configured, a lookup checks the local cache first and then the
server. mise verifies a remote hit, copies it into the local cache, and restores
it from there. After a successful run, mise stores the result locally and, when
the run may write, uploads it. A server that cannot be reached produces a
warning, and mise continues with the local cache only. To skip the server for
one run, use [`mise run --task-cache local-only`](/cli/run.html).

## Who can write

mise uploads results only from CI jobs that build a protected branch. Every
other run can read from the server but does not write to it:

| Where mise runs                                                                                                                                    | Reads | Writes |
| -------------------------------------------------------------------------------------------------------------------------------------------------- | ----- | ------ |
| GitHub Actions `push` to a protected branch (`GITHUB_EVENT_NAME=push`, `GITHUB_REF_TYPE=branch`, `GITHUB_REF_PROTECTED=true`)                      | Yes   | Yes    |
| GitLab CI push pipeline for a protected branch (`CI_PIPELINE_SOURCE=push`, `CI_COMMIT_REF_PROTECTED=true`, no `CI_COMMIT_TAG` or merge request ID) | Yes   | Yes    |
| Pull requests, merge requests, tags, unprotected branches, other CI systems, and local runs                                                        | Yes   | No     |

With `remote_mode = "write-only"`, mise turns the remote off entirely in the
read-only contexts instead of reading from it.

This check is defense in depth. The server must enforce write access itself,
because anyone who can write to a namespace can publish entries that its
readers restore. The [protocol](/tasks/remote-cache-protocol.html#authentication-and-namespace-policy)
describes what a server must check. Checksums detect corruption, and HTTPS
authenticates the server, but a checksum is not a signature from the job that
produced the entry.

To keep untrusted jobs from publishing:

- Give pull-request jobs read-only credentials, or none.
- Run them with `mise run --task-cache read-only`.
- Put less-trusted writers in their own namespace.

## Credentials

mise sends a bearer token when you provide one. Set it in the job's environment
or in your global config; project config cannot set these settings.

| Variable                               | Setting                                                                                           | What it does                                                            |
| -------------------------------------- | ------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------- |
| `MISE_TASK_CACHE_REMOTE_TOKEN`         | [`task.cache.remote_token`](/configuration/settings.html#task.cache.remote_token)                 | Sends this token                                                        |
| `MISE_TASK_CACHE_REMOTE_TOKEN_FILE`    | [`task.cache.remote_token_file`](/configuration/settings.html#task.cache.remote_token_file)       | Rereads the token from this file before every request                   |
| `MISE_TASK_CACHE_REMOTE_OIDC_AUDIENCE` | [`task.cache.remote_oidc_audience`](/configuration/settings.html#task.cache.remote_oidc_audience) | Requests a GitHub Actions OIDC token for this audience and refreshes it |

When more than one is set, mise uses the token, then the token file, then OIDC.
A static token can therefore override workload identity in an emergency without
a change to project configuration. The token file suits rotating credentials
such as Kubernetes projected service account tokens. CI systems other than
GitHub Actions can pass the OIDC token they issue through
`MISE_TASK_CACHE_REMOTE_TOKEN`.

A request that carries a credential must use HTTPS, except to `localhost`,
`127.0.0.0/8`, or `::1`. A cache-enabled task with a credential and a plain
`http://` URL to any other host fails with
`remote cache URL must use HTTPS except for loopback development servers`.
Without a credential, plain HTTP works, but mise warns that cache traffic can be
read or modified in transit.

### GitHub Actions OIDC

In GitHub Actions, mise can request and refresh a short-lived OIDC token itself.
Let the workflow request an identity token and set the audience:

```yaml
permissions:
  contents: read
  id-token: write

jobs:
  test:
    runs-on: ubuntu-latest
    env:
      MISE_TASK_CACHE_REMOTE_OIDC_AUDIENCE: https://cache.example.com
    steps:
      - uses: actions/checkout@v7
      - uses: jdx/mise-action@v5
      - run: mise run test
```

mise keeps the token in memory and refreshes it before it expires. Configure the
server to trust GitHub's issuer (`https://token.actions.githubusercontent.com`),
accept the same audience, and grant the workflow's identity claims, such as its
repository, access to the namespace.

Without `id-token: write`, cache-enabled tasks fail with
``remote cache OIDC audience requires GitHub Actions OIDC; grant `id-token: write` or set MISE_TASK_CACHE_REMOTE_TOKEN``.

## What an entry contains

Every reader of a namespace can read everything in its entries. An uploaded
entry contains:

- The action description that mise hashes into the cache key. It holds the
  task's name, `run` entries, arguments, shell, output patterns, directory,
  source hash, resolved tool versions, operating system, and architecture. It
  lists the cache keys of cached dependencies and the text of each
  `cache.command_inputs` command, with hashes of the command's output. It also
  holds the values of the task's own `env` entries, of every variable named in
  `cache.env` or `task_config.global_env`, and of every mise variable the task
  can see, from `[vars]` and the task's `vars`.
- The task's captured stdout and stderr, after mise applies
  [redactions](/environments/secrets/#redaction).
- Every declared output file.

Source files are not uploaded; only a hash of them is.

::: danger Secrets in the action description
mise writes the variable values and command input text in the action
description verbatim. It does not apply `redact = true` or `redactions` to them.
Do not put secrets in a cached task's `env`, in the `env` of a `{ task = ... }`
run entry, in a variable named in `cache.env` or `task_config.global_env`, in
`vars`, or in a command input. Provide credentials through the job's
environment instead, and list them in
[`pass_through_env`](/tasks/caching.html#environment-variables-and-cache-keys)
when the task runs in an environment sandbox.
:::

Redactions on the log catch only values mise knows about. A task can still
print an unknown credential or write one into an output file, so cache such a
task only when its log and outputs are safe to share with everyone who can read
the namespace.

[`mise cache clear`](/cli/cache/clear.html) removes local entries only. Use the
server's retention and deletion controls to remove uploaded copies.

## Share entries between machines

An entry matches only when every [cache-key input](/tasks/caching.html#what-goes-into-the-cache-key)
matches, so:

- Machines must run the same operating system and architecture.
- Resolved tool versions must be the same. A [lockfile](/dev-tools/mise-lock.html)
  keeps them aligned.
- Source paths are relative to the outermost config root, so different checkout
  locations share entries. A source outside that root is keyed by its absolute
  path and is shared only by checkouts at the same location.
- A variable named in `cache.env` that has a different value in CI, such as
  `CI=true`, produces a different key.
