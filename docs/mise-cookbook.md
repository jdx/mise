# Tasks Cookbook

Here we are sharing a few configurations for tasks that other people have found useful.

## Managing `terraform`/`opentofu` Projects

It is often necessary to have your terraform configuration in a `terraform/` subdirectory.
This necessitates the use of syntax like `terraform -chdir=terraform plan` to use appropriate
terraform command. The following config allows you to invoke all of them from `mise`, leveraging
`mise` tasks.

```toml
[tools]
terraform = "1"

[tasks."terraform:init"]
description = "Initializes a Terraform working directory"
run = "terraform -chdir=terraform init"

[tasks."terraform:plan"]
description = "Generates an execution plan for Terraform"
run = "terraform -chdir=terraform plan"

[tasks."terraform:apply"]
description = "Applies the changes required to reach the desired state of the configuration"
run = "terraform -chdir=terraform apply"

[tasks."terraform:destroy"]
description = "Destroy Terraform-managed infrastructure"
run = "terraform -chdir=terraform destroy"

[tasks."terraform:validate"]
description = "Validates the Terraform files"
run = "terraform -chdir=terraform validate"

[tasks."terraform:format"]
description = "Formats the Terraform files"
run = "terraform -chdir=terraform fmt"

[tasks."terraform:check"]
description = "Checks the Terraform files"
depends = ["terraform:format", "terraform:validate"]

[env]
'_'.file = ".env"

```
