---
description: "Manage a Linux host firewall with nftables, firewalld, or UFW from mise.toml, alongside rules that other tools add."
socialDescription: "Manage a Linux host firewall with nftables, firewalld, or UFW from mise.toml."
---

# Linux firewall

Declare a Linux host firewall in `[bootstrap.linux.firewall]` and mise applies
it with nftables, firewalld, or UFW. mise keeps its rules apart from rules that
other tools or people added, so you can adopt it on a host that already has a
firewall.

## Example

This example denies incoming traffic except HTTPS from anywhere and SSH from
one administration address. Replace `203.0.113.10/32` with your network and
`22` with your SSH port before you apply it. `backend = "auto"` uses a firewall
that is already installed; it does not install one.

```toml
[bootstrap.linux.firewall]
backend = "auto"
default_incoming = "deny"
default_outgoing = "allow"

[[bootstrap.linux.firewall.rules]]
name = "https"
port = 443
protocol = "tcp"

[[bootstrap.linux.firewall.rules]]
name = "ssh-admin"
port = 22
protocol = "tcp"
source = "203.0.113.10/32"
action = "limit"
```

Check the policy with
[`mise bootstrap firewall status`](/cli/bootstrap/firewall/status.html),
preview the commands with `--dry-run`, then apply them with
[`mise bootstrap firewall apply`](/cli/bootstrap/firewall/apply.html):

```sh
mise bootstrap firewall status
mise bootstrap firewall apply --dry-run
mise bootstrap firewall apply
```

The full [`mise bootstrap`](/bootstrap.html) applies the firewall after
packages, files, and system services, and before Compose projects.

## Avoid locking yourself out {#ssh-lockout-protection}

When you run mise over SSH with an incoming policy of `deny` or `reject`, mise
reads `SSH_CONNECTION` and refuses the configuration unless a present incoming
TCP `allow` or `limit` rule covers your client address, the server address, and
the SSH port, without an `interface`. The rule in the example above does that
for a client at `203.0.113.10` connecting to port 22.

Until a covering rule exists, `mise bootstrap firewall apply` and
`mise bootstrap` refuse before they change anything. The read-only commands,
`mise bootstrap firewall status`, `apply --dry-run`, `mise bootstrap status`,
`mise bootstrap plan`, and `mise bootstrap --dry-run`, report the firewall as
`unknown` with the reason instead. A dry run then warns that the change needs
manual action and still exits 0, so a CI check on `mise bootstrap --dry-run`
does not catch the lockout. mise also refuses when:

- `SSH_CONNECTION` is missing, for example because `sudo` removed it, and mise
  finds an `sshd` parent process or cannot inspect its process ancestry.
- A `deny` or `reject` rule that covers the connection comes before the allow
  rule (nftables and UFW), or covers it at all (firewalld, which cannot
  guarantee that the allow wins).

Set `allow_lockout = true` only when you have another way into the host, such
as a provider console.

Each backend switches to the new ruleset without a gap: nftables replaces its
table in one transaction, firewalld changes its permanent policies and then
reloads once, and UFW adds the new rules before it removes the old ones and
applies the default policy last. A half-applied ruleset does not drop your
session.

## Choose a backend {#backends}

| `backend`          | What mise manages                                                                                                                                                       |
| ------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `"auto"` (default) | The backend an earlier mise run used, if still installed; else an active firewalld or UFW; else nftables, firewalld, or UFW, whichever is installed first in that order |
| `"nftables"`       | Its own `inet mise_bootstrap` table, saved to `/etc/mise/bootstrap/firewall.nft` and loaded at boot by `mise-bootstrap-firewall.service`                                |
| `"firewalld"`      | Permanent policies `mise-bootstrap-in` and `mise-bootstrap-out`; mise starts firewalld if it is not running                                                             |
| `"ufw"`            | Rules whose comment is `mise:<name>`, in declared order; mise enables UFW after the rules are in place                                                                  |

The nftables input chain always accepts established and related connections,
loopback traffic, and ICMP and ICMPv6 before the declared rules, whatever
`default_incoming` says. mise checks the generated ruleset before it loads it,
and firewalld validates its permanent configuration before mise reloads it.

If you name a backend and its command is not installed, apply fails instead of
falling back to another backend. `status` and `plan` show which backend is in
use. When the backend changes, for example from UFW to nftables, mise removes
its rules from the old backend as part of the apply.

### Install the backend in the same run

Because the firewall comes after packages and system services in
`mise bootstrap`, one config can install the backend and then apply the policy:

```toml
[bootstrap.packages]
"apt:nftables" = "latest"

[bootstrap.linux.firewall]
backend = "nftables"
default_incoming = "deny"
# plus rules, such as the SSH rule from the example above
```

`mise bootstrap firewall apply` on its own does not install packages, so run
the full `mise bootstrap` the first time.

## Write rules

Each `[[bootstrap.linux.firewall.rules]]` entry is one rule. mise identifies a
rule by its `name`, so keep names stable.

| Field         | Values                                     | Default      | Notes                                                          |
| ------------- | ------------------------------------------ | ------------ | -------------------------------------------------------------- |
| `name`        | Up to 64 ASCII letters, digits, `-`, `_`   | Required     | Unique within the firewall                                     |
| `state`       | `"present"`, `"absent"`                    | `"present"`  | `"absent"` removes a rule mise added earlier                   |
| `direction`   | `"incoming"`, `"outgoing"`                 | `"incoming"` |                                                                |
| `action`      | `"allow"`, `"limit"`, `"deny"`, `"reject"` | `"allow"`    | `"limit"` needs incoming TCP and is not available on firewalld |
| `protocol`    | `"tcp"`, `"udp"`, `"sctp"`, `"dccp"`       | Any          | UFW supports only `"tcp"` and `"udp"`                          |
| `port`        | Number, or a range such as `"8000-8010"`   | Any          | Needs `protocol`                                               |
| `source`      | IPv4 or IPv6 CIDR, such as `"10.0.0.0/8"`  | Any          |                                                                |
| `destination` | IPv4 or IPv6 CIDR                          | Any          | Same address family as `source`                                |
| `interface`   | Interface name, such as `"eth0"`           | Any          | nftables and UFW only                                          |

With nftables and UFW, rules apply in the order you declare them; firewalld
orders the rules in a policy itself. When a rule needs a feature the chosen
backend lacks, mise asks you to pick another backend instead of applying a
weaker rule.

`action = "limit"` rate-limits new incoming TCP connections from each source
address. UFW uses its own limit, which rejects an address after six connection
attempts within 30 seconds. The nftables backend drops new connections from an
address that exceeds 12 a minute, after a burst of five. The two backends'
limits are not identical. firewalld can only limit a rule as a whole, which
would let one address use up everyone's budget, so mise refuses `limit` rules
there.

## How configs combine

When several config files declare `[bootstrap.linux.firewall]`, the most local
value of each top-level key wins, and rules merge by `name`: a more local rule
replaces an inherited rule with the same name. Declaring the same name twice in
one file is an error.

## Remove rules or the firewall {#ownership-and-deletion}

Deleting a rule, or the whole section, from your configuration changes nothing
on the host. mise keeps the rules it applied until you remove them explicitly,
so several config files can each add rules without removing each other's.

- To remove one rule, keep its `name` and set `state = "absent"`.
- `state = "disabled"` removes mise's policy from the running firewall but
  keeps its rules on record. With UFW it disables UFW for the whole host.
- `state = "absent"` deletes mise's rules and its record of them, and leaves
  other rules alone.
- `exclusive = true` drops any mise rule that the current configuration does
  not declare. With UFW it runs `ufw reset`, which also deletes rules that mise
  did not create.

Status shows `coexisting` or `exclusive` for the last choice. Disabling or
deleting the firewall, removing a rule, and exclusive mode are flagged as
destructive changes in the confirmation prompt. `--yes` and non-interactive
runs skip that prompt, so check `exclusive` before you apply. With UFW, the
`apply --dry-run` preview lists the `ufw --force reset` that exclusive mode
runs and the `ufw default` policy commands.

## Preview and apply

```sh
mise bootstrap firewall status            # backend, policy, and each rule
mise bootstrap firewall status --json     # the same, as JSON
mise bootstrap firewall status --missing  # exit 1 if anything would change
mise bootstrap firewall apply --dry-run   # print the backend commands
mise bootstrap firewall apply             # apply after a confirmation prompt
mise bootstrap firewall apply --yes       # apply without prompting
```

A plan describes the changes mise makes, not whether a service is reachable.
mise's rules filter traffic to and from the host itself. Forwarded
traffic, such as connections to ports that Docker publishes for containers,
does not pass through them, and other firewall tools on the host can still
block or allow traffic. Test reachability from the networks that use each
service.

## Reference

| Key                | Values                                                        | Default     |
| ------------------ | ------------------------------------------------------------- | ----------- |
| `backend`          | `"auto"`, `"nftables"`, `"firewalld"`, `"ufw"`                | `"auto"`    |
| `state`            | `"enabled"`, `"disabled"`, `"absent"`                         | `"enabled"` |
| `default_incoming` | `"allow"`, `"deny"`, `"reject"`                               | `"deny"`    |
| `default_outgoing` | `"allow"`, `"deny"`, `"reject"`                               | `"allow"`   |
| `exclusive`        | `true` drops mise rules this configuration does not declare   | `false`     |
| `allow_lockout`    | `true` skips the [SSH lockout check](#ssh-lockout-protection) | `false`     |
| `rules`            | Array of [rules](#write-rules)                                | `[]`        |

### Privileges

Firewall management needs root, even to read the current state. When you are
not root, mise runs its firewall helper through `sudo` for `status`, `plan`,
and `apply`, and passes the plan to it on standard input rather than through a
shell. Set
[`system_packages.sudo = false`](/configuration/settings.html#system_packages.sudo)
to forbid elevation. mise records what it applied in
`/var/lib/mise/bootstrap/firewall.json`.

## On macOS and Windows

Firewall management is Linux-only. On macOS and Windows, `mise bootstrap` and
`mise bootstrap plan` skip the section with the warning
`ignoring [bootstrap.linux.firewall] on non-Linux host`.
`mise bootstrap firewall apply` fails there, and
`mise bootstrap firewall status` reports an unsupported platform.

To avoid the warning, keep the section in a config that only Linux machines
load, such as a [machine module](/bootstrap/modules.html) that only Linux
machines select, or a `mise.linux.toml` file with
[platform environments](/configuration/environments.html#platform-environments)
turned on.

## Troubleshooting

| Problem                                                     | What to do                                                                                                                                   |
| ----------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------- |
| "refusing firewall default incoming deny over SSH"          | Add an incoming TCP rule that covers your address and SSH port, or see [lockout](#ssh-lockout-protection)                                    |
| "firewall backend 'ufw' requires command 'ufw'"             | Install that backend, or use `backend = "auto"`                                                                                              |
| A rule fails with "select backend"                          | The rule uses `limit`, `interface`, or a protocol the backend lacks; change the rule or backend                                              |
| A container port is reachable although the policy denies it | Docker forwards published ports past these rules; publish the port on `127.0.0.1` in the Compose file, or restrict it in Docker              |
| "only supported on Linux" on a Mac                          | `mise bootstrap firewall apply` runs only on Linux; `mise bootstrap` skips the section there. See [macOS and Windows](#on-macos-and-windows) |

## See also

- [Bootstrap](/bootstrap.html#how-it-runs) for where the firewall falls in the
  run order.
- [Docker Compose projects](/bootstrap/compose.html) for services the firewall
  exposes.
