---
description: "Keep Docker Compose projects running, stopped, or removed on a host, after mise bootstrap installs Docker and writes their files."
socialDescription: "Keep Docker Compose projects running, stopped, or removed on a host."
---

# Docker Compose projects

Use `[bootstrap.compose]` to keep a Docker Compose project running, stopped, or
removed on a host. The same config can install Docker and write the project's
files, which suits self-hosted services.

## Example

```toml
[bootstrap.compose.app]
project_dir = "/opt/app"
```

```sh
mise bootstrap compose apply --dry-run
mise bootstrap compose apply
```

[`mise bootstrap compose apply`](/cli/bootstrap/compose/apply.html) runs
`docker compose up --detach --wait` for the Compose file in `/opt/app` and
waits until its services are running and, where they define a health check,
healthy. Later runs change nothing until the project drifts from its Compose
model.

## Install Docker and run a project

[`mise bootstrap`](/bootstrap.html) applies Compose projects after packages,
files, system services, and the firewall, so one config can install Docker,
write the Compose file and its secrets, and start the project. This example
targets an Ubuntu host, whose repositories provide the `docker.io` and
`docker-compose-v2` packages:

```toml
[bootstrap.secrets]
s3_access_key = "S3_ACCESS_KEY" # read from $S3_ACCESS_KEY
s3_secret_key = "S3_SECRET_KEY"

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
template = true
owner = "root"
group = "root"
mode = "0600"

[bootstrap.compose.mise-cache]
project_dir = "/opt/mise-cache"
files = ["compose.yaml"]
env_files = [".env"]
project_name = "mise-cache"
sudo = true # the bootstrap user cannot reach the Docker socket
depends_on = ["package:apt:docker.io", "service:docker"]
```

Create `infra/mise-cache/compose.yaml` next to this `mise.toml`, set both
[secret inputs](/bootstrap/secrets.html), and run `mise bootstrap`. The
environment file template assumes single-line values that are valid in
Compose's `.env` format.

The project depends on the files in `/opt/mise-cache` without saying so,
because mise links a project to managed entries for its `project_dir`,
`files`, and `env_files`. `depends_on` adds the Docker package and service.

## Choose the project's state {#lifecycle-and-convergence}

| `state`               | Command               | Converged when                                                                                  |
| --------------------- | --------------------- | ----------------------------------------------------------------------------------------------- |
| `"running"` (default) | `compose up --detach` | Every selected service runs, is healthy if it has a health check, and matches the Compose model |
| `"stopped"`           | `compose stop`        | No selected container is running; containers and data are kept                                  |
| `"absent"`            | `compose down`        | No container of the project remains                                                             |

For a running project, mise compares Compose's config hash for each service
with the `com.docker.compose.config-hash` label on its containers. Editing a
Compose file, an interpolated variable, a profile, or a service definition
therefore shows up as an update even while the old containers keep running.

List services that run once and exit in `oneshot`. They count as converged
after they exit with code 0, so a migration or setup job does not keep the
project out of date.

With `remove_orphans = true`, the default, mise also removes containers for
services that are no longer in the Compose model: `up` and `down` pass
`--remove-orphans`, and a stopped project removes them with the container
engine.

## Select files, profiles, and services {#project-selection}

`project_dir` is required and must be an absolute path. mise passes it to
Compose as `--project-directory`, and relative entries in `files` and
`env_files` are resolved from it. Without `files`, Compose finds the project's
Compose file the usual way. mise passes several files and environment files in
the order you list them, so later files override earlier ones as Compose
defines.

```toml
[bootstrap.compose.edge]
project_dir = "/srv/edge"
files = ["compose.yaml", "compose.prod.yaml"]
env_files = [".env"]
profiles = ["monitoring"]
services = ["proxy", "grafana"]
```

`services` limits `up` and `stop` to those services; without it, mise manages
every service that the files and profiles enable. Put secret values in an
environment file that a [`[bootstrap.files]`](/bootstrap/files.html) template
writes with mode `0600`, not in `mise.toml`.

## Use Podman or another engine {#engines-and-privileges}

mise uses `docker compose` when it is available, and otherwise a standalone
Docker Compose v2 `docker-compose`. Compose v1 is not supported, because it
lacks the inspection and lifecycle flags that mise relies on. Set `command` to
use another Compose frontend, such as Podman or a remote Docker context, and
`engine_command` for the container engine that mise uses to read container
labels and remove orphaned containers. Both are argument lists, run without a
shell:

```toml
[bootstrap.compose.edge]
project_dir = "/srv/edge"
command = ["podman", "compose"]
engine_command = ["podman"]
```

When `command` contains a `compose` argument, the engine defaults to the
arguments before it, so `engine_command` above only makes the default explicit.
Otherwise the engine is `docker`.

Set `sudo = true` when the project belongs to the system Docker daemon and the
bootstrap user cannot reach its socket. mise then runs every Compose and engine
command, including the ones `status` uses, through `sudo`, and follows
[`system_packages.sudo`](/configuration/settings.html#system_packages.sudo). It
authenticates before it captures command output, so a `sudo` password prompt
is never hidden.

## Use templates {#templates}

Project values are [Tera templates](/templates.html), rendered before mise
checks `project_dir` or resolves `files` and `env_files`. That lets a portable
config use values from `[vars]`, the environment, or its own location:

```toml
[vars]
site = "prod"

[bootstrap.compose.app]
project_dir = "{{ config_root }}/compose"
files = ["compose.yaml", "compose.{{ vars.site }}.yaml"]
env_files = ["{{ config_root }}/compose/.env"]
```

Here <code v-pre>{{ config_root }}</code> is the declaring config's root: the
project directory for a project config, or `MISE_GLOBAL_CONFIG_ROOT` (default
`$HOME`) for the global config. It does not change with the directory you run
mise from.

Text values and lists of text are rendered, including entries in `files`,
`env_files`, `command`, `services`, `profiles`, and `depends_on`. Fields with a
fixed set of values, such as `state`, `pull`, `build`, `recreate`, and
`down_images`, must be written literally. A value without template syntax is
used unchanged. `exec()` is not available, because `status`, `plan`,
`apply --dry-run`, and `apply` must render the same declaration; see
[templates in bootstrap](/bootstrap.html#templates). A template error stops the
command and names the declaring config file.

## How configs combine

Projects merge by name across the
[config hierarchy](/configuration.html#configuration-hierarchy). A more local
config replaces the whole declaration for that name; mise does not merge keys
across files.

## Remove a project

Deleting a declaration leaves the project as it is. To stop and remove it, set
`state = "absent"` and apply, then delete the entry:

```toml
[bootstrap.compose.mise-cache]
project_dir = "/opt/mise-cache"
state = "absent"
down_volumes = true
```

`down_volumes = true` also deletes the project's named and anonymous volumes,
and `down_images` removes its `"local"` or `"all"` images. Both destroy data
that `compose up` cannot recreate, so set them only when you mean to.
[`mise bootstrap unapply`](/bootstrap/modules.html#remove-a-module-s-resources)
does not remove Compose projects; declare them absent instead.

## Preview and apply

Check each project's state with
[`mise bootstrap compose status`](/cli/bootstrap/compose/status.html), and
preview the commands before you apply them:

```sh
mise bootstrap compose status            # state of each project
mise bootstrap compose status --json     # the same, as JSON
mise bootstrap compose status --missing  # exit 1 if any project would change
mise bootstrap compose apply --dry-run   # print the Compose commands
mise bootstrap compose apply             # apply after a confirmation prompt
mise bootstrap compose apply --yes       # apply without prompting
```

`mise bootstrap compose apply` changes only Compose projects. It does not
install Docker or write the project's files, and a project whose Compose file
does not exist yet is reported as `unknown`. When the same config provides
those, preview and apply them together:

```sh
mise bootstrap plan
mise bootstrap --only packages,files,services,compose --dry-run
```

`mise bootstrap` inspects projects again after the earlier steps finish, so a
Compose file or Docker installation created in the same run is picked up. A dry
run cannot show whether an image starts or passes its health check.

## Reference

| Key                       | Values                                                                                                                         | Default                                  |
| ------------------------- | ------------------------------------------------------------------------------------------------------------------------------ | ---------------------------------------- |
| `project_dir`             | Absolute path                                                                                                                  | Required                                 |
| `project_name`            | Lowercase letters, digits, `-`, and `_`, starting with a letter or digit                                                       | Chosen by Compose                        |
| `files`                   | Compose files, passed with `--file`                                                                                            | Compose's own discovery                  |
| `env_files`               | Interpolation files, passed with `--env-file`                                                                                  | `[]`                                     |
| `profiles`                | Profiles, passed with `--profile`                                                                                              | `[]`                                     |
| `services`                | Services to manage                                                                                                             | Every enabled service                    |
| `oneshot`                 | Services that may stay exited after code 0; must also be in `services` when it is set                                          | `[]`                                     |
| `depends_on`              | Bootstrap resources to apply first, as `<kind>:<name>` with kind `package`, `file`, `directory`, `service`, `user`, or `group` | `[]`                                     |
| `state`                   | `"running"`, `"stopped"`, `"absent"`                                                                                           | `"running"`                              |
| `pull`                    | `"missing"`, `"always"`, `"never"`                                                                                             | `"missing"`                              |
| `build`                   | `"auto"`, `"always"` (`--build`), `"never"` (`--no-build`)                                                                     | `"auto"`                                 |
| `recreate`                | `"auto"`, `"always"` (`--force-recreate`), `"never"` (`--no-recreate`)                                                         | `"auto"`                                 |
| `wait`                    | `true` passes `--wait` to `up`                                                                                                 | `true`                                   |
| `wait_timeout`            | Seconds for `--wait-timeout`                                                                                                   | None                                     |
| `timeout`                 | Seconds for `--timeout` on `up`, `stop`, and `down`                                                                            | None                                     |
| `remove_orphans`          | `true` removes containers no longer in the model                                                                               | `true`                                   |
| `renew_anonymous_volumes` | `true` passes `--renew-anon-volumes` to `up`                                                                                   | `false`                                  |
| `down_volumes`            | `true` passes `--volumes` to `down`                                                                                            | `false`                                  |
| `down_images`             | `"local"` or `"all"`, passed as `--rmi`                                                                                        | None                                     |
| `sudo`                    | `true` runs Compose and engine commands through `sudo`                                                                         | `false`                                  |
| `command`                 | Compose command as an argument list                                                                                            | `docker compose`, or `docker-compose` v2 |
| `engine_command`          | Engine command as an argument list                                                                                             | Derived from `command`, or `docker`      |

Some combinations are rejected while mise plans the run:

- `services` cannot be combined with `state = "absent"`, because `compose down`
  removes the whole project.
- `down_volumes` and `down_images` require `state = "absent"`.
- `renew_anonymous_volumes` requires `state = "running"`.
- `wait_timeout` requires `wait = true` and a value above zero.
- Each `depends_on` entry must name a declared resource, so a misspelling fails
  during planning.

See Docker's references for
[`docker compose`](https://docs.docker.com/reference/cli/docker/compose/),
[`up`](https://docs.docker.com/reference/cli/docker/compose/up/), and
[`down`](https://docs.docker.com/reference/cli/docker/compose/down/).

## See also

- [Bootstrap](/bootstrap.html#how-it-runs) for where Compose projects fall in
  the run order.
- [Linux firewall](/bootstrap/firewall.html), which does not filter ports that
  Docker publishes.
- [Daemons](/daemons.html) for development services that run per project rather
  than per host.
