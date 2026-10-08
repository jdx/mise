---
description: "Clone Git repositories to declared paths and keep them on a branch, tag, or commit with mise bootstrap."
socialDescription: "Clone Git repositories to declared paths with mise bootstrap."
---

# Git repositories

Declare Git checkouts in `[bootstrap.repos]`, keyed by where each one should
live. [`mise bootstrap`](/bootstrap.html) clones missing ones before it applies
[dotfiles](/dotfiles.html), so a new machine can clone your dotfiles repository
and link files from it in one run.

## Example

```toml
[bootstrap.packages]
"apt:git" = "latest"

[bootstrap.repos]
"~/src/dotfiles" = { url = "git@github.com:example/dotfiles.git", ref = "main" }
"~/src/project" = { url = "https://github.com/example/project.git" }

[dotfiles]
"~/.zshrc" = "~/src/dotfiles/zshrc"
```

```sh
mise bootstrap repos apply --dry-run
mise bootstrap
```

`mise bootstrap` installs Git, clones both repositories, and then links
`~/.zshrc` to the file in the new checkout. Git must be able to authenticate to
each `url`, for example with an SSH key for the `git@github.com:` form.

Until the repository is cloned, a dry run of the full `mise bootstrap` stops at
dotfiles whose source is inside it, so preview the repositories on their own the
first time.

To work with repositories alone, use
[`mise bootstrap repos apply`](/cli/bootstrap/repos/apply.html).

## Declare a repository

Each key is the path of the checkout, and `url` is required. A path can be
absolute, start with `~/`, or be relative. A relative path is resolved from the
project root of the config that declares it and must name a directory inside
that root: it cannot be empty or `.`, and it cannot leave the root with `..` or
an absolute segment. Relative paths therefore work only in a project config,
not in the global config `~/.config/mise/config.toml`.

The optional `ref` is a branch, a tag, or a full commit SHA. An invalid entry
is reported with a warning and skipped.

## Choose a ref {#choose-apply-or-update}

What `apply` and
[`mise bootstrap repos update`](/cli/bootstrap/repos/update.html) do with an
existing checkout depends on `ref`:

| Declaration  | `apply` on an existing checkout                                                    | `update`                                     |
| ------------ | ---------------------------------------------------------------------------------- | -------------------------------------------- |
| No `ref`     | Leaves it at its current commit                                                    | Fetches and fast-forwards the current branch |
| Branch `ref` | Unless it already matches origin's branch, checks out the branch and fast-forwards | Same as `apply`                              |
| Tag `ref`    | Unless it is already at the tag, fetches and checks out the tag                    | Same as `apply`                              |
| Commit `ref` | Unless it is already at the commit, fetches and checks out the commit              | Same as `apply`                              |

Both commands clone missing repositories, at `ref` when one is set. With a
branch or tag `ref`, status asks the remote for its current commit with
`git ls-remote`, so it needs network access; when the remote cannot be
reached, the repository shows as `differs`. A commit `ref` is checked locally.
`update` skips an unpinned repository whose `HEAD` is detached, with a warning.

The full `mise bootstrap` applies repositories; `mise bootstrap --update` runs
`update` for them instead.

## Protect local work

mise never overwrites local work. Before it changes any repository, apply and
update fail if a checkout has uncommitted or untracked changes, if a target is
a non-empty directory that is not a Git checkout, or if a checkout's `origin`
does not match `url`. An empty directory is treated as missing and cloned into.

Pass `--skip-dirty` to skip repositories with local changes, with a warning,
and update the rest. Conflicts still stop the run before anything changes.

### How origins are matched

These forms count as the same repository, with host names compared
case-insensitively:

- `git@host:owner/repo`
- `ssh://git@host/owner/repo`
- `https://host/owner/repo`

A different host, SSH alias, port, path, or SSH user is a conflict. Any other
URL must match exactly, except that a trailing `/` is ignored and, on network
URLs, so is a trailing `.git`. Exact matching applies to:

- `http://` and `git://` URLs, so an insecure transport never stands in for the
  `https://` form
- SSH URLs without a user, which Git resolves to your login name rather than
  `git`
- URLs with a query string or fragment, and `https://` URLs with a user name or
  password
- local paths and `file://` URLs

## Run a command in every repository

[`mise bootstrap repos exec`](/cli/bootstrap/repos/exec.html) runs a command
in each checkout that exists and whose origin matches, including checkouts with
local changes. It runs the command directly, without a shell, with the checkout
as the working directory, and skips missing and conflicting repositories with a
warning:

```sh
mise bootstrap repos exec -- git status --short
mise bootstrap repos exec ~/src/project -- git log -1
mise bootstrap repos exec --continue-on-error -- git fetch
mise bootstrap repos exec --dry-run -- git pull
```

Paths before `--` limit the command to those checkouts. By default it stops at
the first failure; with `--continue-on-error`, it visits every checkout and
reports all failures at the end. With `--dry-run`, it prints the command for
each checkout without running it.

## How configs combine

Entries merge by expanded path across the
[config hierarchy](/configuration.html#configuration-hierarchy), so
`"~/src/project"` and `"/home/you/src/project"` are the same entry. A more
local config replaces the whole entry for that path.

## Remove a repository

mise never deletes a checkout. Deleting a declaration only stops mise from
managing it, and
[`mise bootstrap unapply`](/bootstrap/modules.html#remove-a-module-s-resources)
reports how many repository entries a module declares but leaves the checkouts
for you to delete. Delete the directory yourself when you no longer need it.

## Preview and apply

Check each repository with
[`mise bootstrap repos status`](/cli/bootstrap/repos/status.html), and preview
the Git commands before you apply them:

```sh
mise bootstrap repos status                  # state of each repository
mise bootstrap repos status --json           # the same, as JSON
mise bootstrap repos status --missing        # exit 1 if any repository is not current
mise bootstrap repos apply --dry-run         # print the Git commands
mise bootstrap repos apply                   # clone or move checkouts to their ref
mise bootstrap repos apply --yes             # apply without prompting
mise bootstrap repos apply --skip-dirty      # skip checkouts with local changes
mise bootstrap repos update                  # also fast-forward unpinned checkouts
mise bootstrap repos update ~/src/project    # update one configured path
mise bootstrap repos update --skip-dirty     # skip checkouts with local changes
```

`update` takes the same `--dry-run` and `--yes` flags. Paths passed to it must
match a configured path, as written or expanded.

| State      | Meaning                                                                |
| ---------- | ---------------------------------------------------------------------- |
| `current`  | The checkout exists, its origin matches, and it is at `ref`            |
| `missing`  | The path does not exist or is an empty directory                       |
| `differs`  | The checkout is clean but not at `ref`                                 |
| `dirty`    | The checkout has uncommitted or untracked changes                      |
| `conflict` | The path is not a directory, not a Git checkout, or has another origin |

## Reference

| Key   | Values                          | Default                         |
| ----- | ------------------------------- | ------------------------------- |
| `url` | Repository URL or path          | Required                        |
| `ref` | Branch, tag, or full commit SHA | None; the checkout is not moved |

## See also

- [Bootstrap](/bootstrap.html#how-it-runs) for where repositories fall in the
  run order, before dotfiles.
- [Dotfiles](/dotfiles.html) for linking files from a cloned repository.
- [Bootstrap from a repository](/bootstrap/from-repository.html) to start a
  machine from a configuration repository.
