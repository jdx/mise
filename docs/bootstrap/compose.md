# Docker Compose projects

`[bootstrap.compose]` declaratively manages long-running Docker Compose
projects after packages, privileged files, directories, and system services
have converged. This makes it suitable for self-hosted services whose Compose
file, environment file, Docker installation, and daemon lifecycle are all
managed by the same bootstrap configuration.

```toml
[bootstrap.packages]
"apt:docker.io" = "latest"
"apt:docker-compose-v2" = "latest"

[bootstrap.services.docker]
state = "running"
enabled = true

[bootstrap.directories."/opt/mise-cache"]
mode = "0755"

[bootstrap.files."/opt/mise-cache/compose.yaml"]
source = "./infra/mise-cache/compose.yaml"
mode = "0644"

[bootstrap.files."/opt/mise-cache/.env"]
content = """
S3_ACCESS_KEY={{ secret(name="s3_access_key") }}
S3_SECRET_KEY={{ secret(name="s3_secret_key") }}
"""
mode = "0600"

[bootstrap.compose.mise-cache]
project_dir = "/opt/mise-cache"
files = ["compose.yaml"]
env_files = [".env"]
project_name = "mise-cache"
state = "running"
sudo = true
depends_on = ["package:apt:docker.io", "service:docker"]
```

`project_dir` is required and must be absolute. Relative entries in `files`
and `env_files` resolve from it. With no `files`, Compose performs its normal
project-directory discovery. Mise passes multiple files and environment files
in declaration order, so later entries retain Compose's override semantics.

## Lifecycle and convergence

`state` controls the project lifecycle:

- `"running"` (default) runs `compose up --detach`, checks every selected
  service's runtime and health, and compares Compose's canonical config hash
  with each container's `com.docker.compose.config-hash` label.
- `"stopped"` runs `compose stop`, preserving configured containers and project
  data. With `remove_orphans = true`, containers removed from the Compose model
  are removed through the configured container engine.
- `"absent"` runs `compose down`, with optional volume and image removal.

Status remains converged only while both container state and the rendered
Compose model match. Changes to Compose files, interpolation inputs, profiles,
or service configuration are surfaced as an update rather than being hidden
behind a merely-running container.

`oneshot` lists selected services that are converged after exiting successfully
with code 0. Other selected services must be running and, when a health check
exists, healthy.

## Project selection

- `project_name`: explicit Compose project name. It must start with a lowercase
  letter or digit and contain only lowercase letters, digits, dashes, and
  underscores.
- `files`: ordered Compose files passed with `--file`.
- `env_files`: ordered interpolation environment files passed with
  `--env-file`. Put secret values in mode-`0600` managed files rather than in
  the bootstrap config.
- `profiles`: profiles enabled with `--profile`.
- `services`: optional service subset. Empty means every service enabled by the
  selected files and profiles.
- `oneshot`: successfully completed services allowed to remain exited. When
  `services` is set, each one-shot service must also be selected there.
- `depends_on`: additional bootstrap resource IDs that must converge first,
  such as `"package:apt:docker.io"` or `"service:docker"`. Managed entries
  matching `project_dir`, `files`, and `env_files` are linked automatically.
  Explicit dependencies must exist, so misspellings fail during planning.

## Apply policy

- `pull`: `"missing"` (default), `"always"`, or `"never"`.
- `build`: `"auto"` (default), `"always"` (`--build`), or `"never"`
  (`--no-build`).
- `recreate`: `"auto"` (default), `"always"` (`--force-recreate`), or
  `"never"` (`--no-recreate`).
- `wait`: wait for running/healthy services after `up` (default `true`).
- `wait_timeout`: maximum wait in seconds.
- `timeout`: stop/shutdown timeout in seconds.
- `remove_orphans`: remove project containers no longer present in the model
  (default `true`).
- `renew_anonymous_volumes`: renew anonymous volumes during `up` (default
  `false`).
- `down_volumes`: remove named and anonymous volumes during `down` (default
  `false`). This is destructive and should be enabled deliberately.
- `down_images`: optionally remove `"local"` or `"all"` project images during
  `down`. This is also destructive.

## Engines and privileges

Mise uses `docker compose` when available and falls back to a standalone Docker
Compose v2 `docker-compose` command. Legacy Compose v1 is not supported because
it lacks the structured inspection and lifecycle flags required for safe
convergence. `command` and `engine_command` accept argv arrays for Podman,
remote Docker contexts, or other compatible frontends without invoking a shell:

```toml
[bootstrap.compose.edge]
project_dir = "/srv/edge"
command = ["podman", "compose"]
engine_command = ["podman"]
```

The engine command is used only to inspect container config-hash labels. Set
`sudo = true` when the project belongs to the system Docker daemon and the
bootstrap user does not have socket access. Mise authenticates before capturing
status output, never hides an interactive sudo prompt, and honors the existing
`system_packages.sudo` policy.

```sh
mise bootstrap compose status
mise bootstrap compose status --json
mise bootstrap compose apply --dry-run
mise bootstrap compose apply --yes
```

Aggregate `mise bootstrap plan` orders Compose projects after package, file,
directory, and system-service resources. Aggregate apply re-inspects projects
after those dependencies finish, so a Compose file or Docker installation
created during the same run is handled without guessing during execution.

See Docker's references for [`docker compose`](https://docs.docker.com/reference/cli/docker/compose/),
[`up`](https://docs.docker.com/reference/cli/docker/compose/up/), and
[`down`](https://docs.docker.com/reference/cli/docker/compose/down/) for the
underlying lifecycle semantics.
