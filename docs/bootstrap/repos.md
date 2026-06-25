# Repos <Badge type="warning" text="experimental" />

mise can declare git repositories in `[bootstrap.repos]` and apply them with
`mise bootstrap repos apply`:

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
  the configured `origin` URL matches.
- **No implicit writes** — repos are applied only by
  `mise bootstrap repos apply` or `mise bootstrap`.
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
```

## States

| State      | Meaning                                      |
| ---------- | -------------------------------------------- |
| `current`  | repo exists, origin matches, and ref matches |
| `missing`  | target path does not exist or is empty       |
| `differs`  | repo is clean but not at the configured ref  |
| `dirty`    | repo has local changes or untracked files    |
| `conflict` | target path is not the expected git repo     |
