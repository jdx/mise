# Repos

mise can declare git repositories in `[bootstrap.repos]` and apply them with
`mise bootstrap repos apply` or as part of [`mise bootstrap`](/bootstrap.html):

```toml
[bootstrap.repos]
"~/src/dotfiles" = { url = "git@github.com:jdx/dotfiles.git", ref = "main" }
"~/src/mise" = { url = "https://github.com/jdx/mise.git" }
```

Each key is the target path. The `url` is required. The optional `ref` can be a
branch, tag, or full commit SHA.

Repos run after `[bootstrap.packages]` and before `[dotfiles]`, so a bootstrap
config can install `git`, clone a dotfiles repository, and then apply dotfiles
from that checkout.

## Semantics

- **Declarative and path-keyed** — entries merge across the config hierarchy
  by expanded target path. A more local config replaces the full repo entry
  for that path.
- **Safe updates only** — mise clones missing repos or empty target
  directories and updates existing repos only when the worktree is clean and
  the configured `origin` URL matches. Common network URL forms are compared
  transport-agnostically: `git@host:path`, `ssh://git@host/path`, and
  `https://host/path` identify the same repo. Different hosts, ssh aliases,
  explicit ports, paths, or non-`git` ssh users still conflict, and local
  paths and `file://` URLs require an exact match.
- **No implicit writes** — repos are changed only by explicit `apply`, `update`,
  `exec`, or top-level `mise bootstrap` commands. Applying never pulls an
  existing repo without a configured `ref`; use `mise bootstrap repos update`
  when you want that imperative behavior.
- **No forced resets** — dirty repos, non-empty non-git target paths, and
  mismatched origins fail instead of overwriting local work.
- **Omitted `ref`** — an existing repo with the expected origin is considered
  current; mise does not fetch or update it.

## Commands

```sh
mise bootstrap repos status            # shows repo checkout state
mise bootstrap repos status --json     # machine-readable
mise bootstrap repos status --missing  # exit 1 if any repo is not current

mise bootstrap repos apply           # clone or update missing/changed repos
mise bootstrap repos apply --dry-run # print the commands without running them
mise bootstrap repos apply --yes     # skip the confirmation prompt

mise bootstrap repos update             # clone missing and pull existing repos
mise bootstrap repos update ~/src/mise  # update only a matching path
mise bootstrap repos update --dry-run   # print the commands without running them
mise bootstrap repos update --yes       # skip the confirmation prompt

mise bootstrap repos exec -- git status        # run argv in every usable repo
mise bootstrap repos exec ~/src/mise -- git pull
mise bootstrap repos exec --continue-on-error -- command
mise bootstrap repos exec --dry-run -- command
```

`update` fetches and fast-forward pulls the current branch of repos without a
configured `ref`. It warns and skips an unpinned repo with a detached HEAD.
Dirty repos, conflicting origins, and non-git targets fail before any repo is
changed. Passing one or more paths limits the update to exact configured paths
or their expanded forms.

`exec` runs the command directly, without shell interpolation, with each repo
as its working directory. Missing and conflicting repos are skipped with a
warning. It stops on the first command failure unless `--continue-on-error` is
set; in that mode it visits every usable repo and reports all failures at the
end.

## States

| State      | Meaning                                      |
| ---------- | -------------------------------------------- |
| `current`  | repo exists, origin matches, and ref matches |
| `missing`  | target path does not exist or is empty       |
| `differs`  | repo is clean but not at the configured ref  |
| `dirty`    | repo has local changes or untracked files    |
| `conflict` | target path is not the expected git repo     |
