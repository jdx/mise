# Linux host firewall

`[bootstrap.linux.firewall]` declaratively manages a Linux host firewall. It
supports native nftables, firewalld policies, and UFW while keeping mise-owned
rules separate from unrelated host rules.

```toml
[bootstrap.linux.firewall]
backend = "auto"
state = "enabled"
default_incoming = "deny"
default_outgoing = "allow"

[[bootstrap.linux.firewall.rules]]
name = "https"
port = 443
protocol = "tcp"
action = "allow"

[[bootstrap.linux.firewall.rules]]
name = "ssh-admin"
port = 22
protocol = "tcp"
source = "203.0.113.10/32"
action = "allow"
```

Firewall convergence runs after packages, privileged files, and system
services, but before Compose projects. This lets the configuration install and
start its selected firewall backend before its policy is applied.

## Backends

`backend` accepts:

- `"auto"` (default): reuse the backend recorded by an earlier mise run, then
  prefer an already-active firewalld or UFW installation, then use nftables,
  firewalld, or UFW in that order when available.
- `"nftables"`: maintain an isolated `inet mise_bootstrap` table and a
  persistent `mise-bootstrap-firewall.service`. Runtime replacement is an
  atomic, syntax-checked nft transaction.
- `"firewalld"`: maintain the permanent `mise-bootstrap-in` and
  `mise-bootstrap-out` policies and reload firewalld only after its
  permanent configuration validates.
- `"ufw"`: maintain rules bearing `mise:<name>` comments in declared order,
  apply them before the default policy, and enable UFW after all rules are
  installed.

Explicitly selected backends fail closed when their command is unavailable.
The selected backend and effective state are visible in `status` and `plan`.

## Policy and rules

`state` accepts `"enabled"` (default), `"disabled"`, or `"absent"`.
Removing the firewall section from config does nothing: deletion must be
requested explicitly with `state = "absent"`. `disabled` retains mise's saved
rule model but removes the nftables/firewalld policy from the runtime; for UFW,
it disables UFW globally. `absent` removes only mise-managed rules and
metadata. Disabling and deleting are presented as destructive changes.

`default_incoming` and `default_outgoing` accept `"allow"`, `"deny"`, or
`"reject"`. Their defaults are incoming deny and outgoing allow.

Each `[[bootstrap.linux.firewall.rules]]` supports:

- `name` (required): stable ASCII identifier used to reconcile the rule
- `state`: `"present"` (default) or `"absent"`
- `direction`: `"incoming"` (default) or `"outgoing"`
- `action`: `"allow"` (default), `"deny"`, or `"reject"`
- `protocol`: `"tcp"`, `"udp"`, `"sctp"`, or `"dccp"`
- `port`: a number or inclusive string range such as `"8000-8010"`
- `source` and `destination`: IPv4 or IPv6 CIDR networks
- `interface`: an interface name (nftables and UFW only)

A port requires a protocol. One rule cannot mix IPv4 and IPv6 source and
destination networks. UFW supports only TCP and UDP; firewalld policy rules do
not safely support per-rule interface matching, so mise asks you to select
nftables or UFW for those combinations rather than silently weakening a rule.

## Ownership and deletion

By default, a later config preserves previously managed rules that it does not
mention. Use a same-name rule with `state = "absent"` to remove one explicitly.
This makes removing configuration non-destructive and permits separately
layered configs to coexist.

Set `exclusive = true` only when the configuration owns the complete firewall.
It drops undeclared mise rules. With UFW, exclusive mode performs `ufw reset`,
which also removes unrelated UFW rules, and is therefore always confirmed as a
destructive operation.

## SSH lockout protection

When bootstrap runs over SSH and incoming policy is deny or reject, mise checks
`SSH_CONNECTION` before making changes. At least one present incoming TCP allow
rule must cover both the connected peer address and the server port. Otherwise
apply fails before elevation. A deliberately out-of-band deployment can set
`allow_lockout = true` as an explicit escape hatch.

Rules are installed in declared order before deny policies for UFW. nftables
installs the complete ruleset atomically, and firewalld changes permanent
policies before a single validated reload, so intermediate state cannot drop
the active SSH connection.

```sh
mise bootstrap firewall status
mise bootstrap firewall status --json
mise bootstrap firewall status --missing
mise bootstrap firewall apply --dry-run
mise bootstrap firewall apply --yes
```

Firewall management is Linux-only and uses the same constrained privileged
helper protocol as bootstrap accounts, files, and services. Typed plans travel
on stdin; config values are never interpolated into a root shell command.
