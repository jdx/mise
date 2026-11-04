---
description: "Run Terraform or OpenTofu commands for a subdirectory as mise tasks: init, check, plan and apply."
---

# Terraform and OpenTofu

Pin Terraform or OpenTofu for a project and run init, validate, plan and apply
as tasks. The recipe assumes a `terraform/` directory that holds your
configuration, with provider credentials already set in the environment.

## Run Terraform from a subdirectory {#managing-terraform-opentofu-projects}

Each task passes `-chdir=terraform`, and mise runs tasks from the config root,
so the tasks work from any directory in the project:

```toml [mise.toml]
[tools]
terraform = "1"

[tasks."terraform:init"]
description = "Initialize the working directory"
run = "terraform -chdir=terraform init"

[tasks."terraform:plan"]
description = "Show the execution plan"
depends = ["terraform:init"]
run = "terraform -chdir=terraform plan"

[tasks."terraform:apply"]
description = "Apply the planned changes"
depends = ["terraform:init"]
interactive = true
run = "terraform -chdir=terraform apply"

[tasks."terraform:destroy"]
description = "Destroy the managed infrastructure"
depends = ["terraform:init"]
interactive = true
run = "terraform -chdir=terraform destroy"

[tasks."terraform:validate"]
description = "Validate the configuration"
depends = ["terraform:init"]
run = "terraform -chdir=terraform validate"

[tasks."terraform:format"]
description = "Format the configuration files"
run = "terraform -chdir=terraform fmt"

[tasks."terraform:format-check"]
description = "Check formatting without changing files"
run = "terraform -chdir=terraform fmt -check"

[tasks."terraform:check"]
description = "Check formatting and validate the configuration"
depends = ["terraform:format-check", "terraform:validate"]
```

Run `mise run terraform:check` to validate, then `mise run terraform:plan` to
see the proposed changes. `terraform:format` is the task that rewrites
formatting. Every task that depends on `terraform:init` can download providers
and update `.terraform.lock.hcl`, even when you only run a check.

`terraform:apply` and `terraform:destroy` keep Terraform's confirmation prompt;
[`interactive = true`](/tasks/task-configuration.html#interactive) gives those
commands the terminal. The recipe saves no plan file, so `apply` computes its
own plan before it asks for approval.

## Use OpenTofu

Replace the tool with `opentofu = "1"` and the command `terraform` with `tofu`.
The `terraform/` directory and the task names can stay, or you can rename them
to match your project.

## Read the version from `.terraform-version` {#read-the-version-from-terraform-version}

If the project already has a `.terraform-version` (tfenv) or
`.opentofu-version` (tofuenv) file, enable it as an
[idiomatic version file](/dev-tools/versions.html#idiomatic-version-files)
instead of setting the version in `[tools]`:

```toml [mise.toml]
[settings]
idiomatic_version_file_enable_tools = ["terraform"]
```

Use `"opentofu"` for `.opentofu-version`.

## Load credentials from a dotenv file

If you keep credentials in a local dotenv file, load it with
[`_.file`](/environments/#env-file). Keep plaintext credential files out of
version control; see [secrets](/environments/secrets/) for encrypted storage.
