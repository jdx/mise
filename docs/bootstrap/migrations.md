# Bootstrap Migrations

Bootstrap migrations are ordered executable files that mise runs once per
machine. Put them in `mise-migrations/` next to the project's `mise.toml`:

```text
mise.toml
mise-migrations/
├── 20260830-move-config
├── 20260912-rebuild-cache
└── 20261001-update-service
```

They are useful for transitions that cannot be expressed as convergent
bootstrap resources, such as moving state from an old location or rewriting a
file whose old shape is no longer configured. Prefer declarative
[`[bootstrap]`](/bootstrap.html) resources whenever mise can describe the desired
end state directly.

## Execution

Migrations run in filename order after tools and bootstrap package plugins are
installed, and before the recurring `bootstrap` task. Each file runs from the
project root through `mise exec`, so configured tools and environment variables
are available. Migration files must be executable and should start with an
appropriate shebang:

```sh
#!/usr/bin/env bash
set -euo pipefail

mv "$HOME/.old-example" "$HOME/.config/example"
```

`MISE_BOOTSTRAP_MIGRATION` contains the current migration filename while the
file runs.

After a migration exits successfully, mise stores its BLAKE3 digest under
`$MISE_STATE_DIR/bootstrap/migrations/`. The filename is its machine-global ID,
which lets the same completion record work when
[`mise bootstrap remote`](/bootstrap/remote.html) uses a temporary checkout.
Use a date or another globally unique prefix to avoid collisions with migrations
from other bootstrap projects.

Applied migrations are immutable. If an applied file's content changes, mise
reports an error and asks you to restore it and add a new migration. A failed
migration is not recorded and is retried on the next apply. Removing an applied
migration does not undo it or remove its completion record.

## Commands

```sh
mise bootstrap migrations status
mise bootstrap migrations status --missing
mise bootstrap migrations status --json
mise bootstrap migrations apply
mise bootstrap migrations apply --dry-run
```

The full bootstrap command also supports `--only migrations` and
`--skip migrations`. A full `mise bootstrap --dry-run` reports pending
migrations without running them or writing completion state.
