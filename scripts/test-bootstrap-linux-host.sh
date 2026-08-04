#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
mise_bin=${1:-$repo_root/target/debug/mise}

if [[ ! -x $mise_bin ]]; then
	echo "mise binary is not executable: $mise_bin" >&2
	exit 1
fi
mise_bin=$(cd "$(dirname "$mise_bin")" && pwd)/$(basename "$mise_bin")
for command in docker ssh ssh-keygen; do
	if ! command -v "$command" >/dev/null; then
		echo "required command is unavailable: $command" >&2
		exit 1
	fi
done

test_tmp_root=${RUNNER_TEMP:-/tmp}
case_dir=$(mktemp -d "$test_tmp_root/mise-bootstrap-host.XXXXXXXX")
container="mise-bootstrap-host-$RANDOM-$$"
image="mise-bootstrap-host:test-$RANDOM-$$"

cleanup() {
	status=$?
	if ((status != 0)); then
		docker logs "$container" >&2 2>/dev/null || true
		docker exec "$container" systemctl --failed --no-pager >&2 2>/dev/null || true
	fi
	docker rm --force "$container" >/dev/null 2>&1 || true
	docker image rm --force "$image" >/dev/null 2>&1 || true
	case "$case_dir" in
	"$test_tmp_root"/mise-bootstrap-host.*) rm -rf -- "$case_dir" ;;
	*) echo "refusing to remove unexpected test directory: $case_dir" >&2 ;;
	esac
	return "$status"
}
trap cleanup EXIT

ssh-keygen -q -t ed25519 -N '' -f "$case_dir/id_ed25519"

cat >"$case_dir/mise.toml" <<'TOML'
[bootstrap.groups.mise-case]
system = true

[bootstrap.users.mise-case]
system = true
group = "mise-case"
home = "/var/lib/mise-case"
shell = "/usr/sbin/nologin"
comment = "mise bootstrap host acceptance test"
create_home = true

[bootstrap.directories."/opt/mise-case"]
owner = "mise-case"
group = "mise-case"
mode = "0750"

[bootstrap.files."/etc/mise-case.conf"]
content = "generation=1\n"
owner = "root"
group = "root"
mode = "0640"
notify = ["mise-case"]

[bootstrap.files."/etc/systemd/system/mise-case.service"]
content = '''
[Unit]
Description=mise bootstrap host acceptance service

[Service]
ExecStart=/usr/bin/tail -f /dev/null

[Install]
WantedBy=multi-user.target
'''
owner = "root"
group = "root"
mode = "0644"
notify = ["mise-case"]

[bootstrap.files."/opt/mise-case/compose.yaml"]
source = "./compose.yaml"
owner = "mise-case"
group = "mise-case"
mode = "0640"

[bootstrap.services.docker]
state = "running"
enabled = true

[bootstrap.services.mise-case]
state = "running"
enabled = true
on_change = "restart"

[bootstrap.linux.firewall]
backend = "nftables"
state = "enabled"
default_incoming = "deny"
default_outgoing = "allow"

[[bootstrap.linux.firewall.rules]]
name = "ssh"
direction = "incoming"
action = "allow"
port = 22
protocol = "tcp"

[bootstrap.compose.smoke]
project_dir = "/opt/mise-case"
files = ["compose.yaml"]
project_name = "mise-bootstrap-smoke"
state = "running"
pull = "missing"
wait = true
wait_timeout = 60
depends_on = ["service:docker"]
TOML

cat >"$case_dir/compose.yaml" <<'YAML'
services:
  smoke:
    image: busybox:1.36.1
    command: ["sh", "-c", "while true; do sleep 3600; done"]
YAML

docker build --tag "$image" "$repo_root/test/fixtures/bootstrap-linux-host"
docker run --detach \
	--name "$container" \
	--privileged \
	--cgroupns=host \
	--tmpfs /run \
	--tmpfs /run/lock \
	--volume /sys/fs/cgroup:/sys/fs/cgroup:rw \
	"$image" >/dev/null
docker cp "$mise_bin" "$container:/opt/mise"
docker exec "$container" chmod 0755 /opt/mise

for _ in {1..60}; do
	system_state=$(docker exec "$container" systemctl is-system-running 2>/dev/null || true)
	if [[ $system_state == running || $system_state == degraded ]]; then
		break
	fi
	sleep 1
done
system_state=$(docker exec "$container" systemctl is-system-running 2>/dev/null || true)
if [[ $system_state != running && $system_state != degraded ]]; then
	echo "container systemd failed to start: $system_state" >&2
	exit 1
fi

docker exec --interactive "$container" bash -c \
	'install -d -m 0700 /root/.ssh && install -m 0600 /dev/stdin /root/.ssh/authorized_keys' \
	<"$case_dir/id_ed25519.pub"

host=$(docker inspect "$container" --format '{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}')
host_key=$(docker exec "$container" cat /etc/ssh/ssh_host_ed25519_key.pub)
printf '%s %s\n' "$host" "$host_key" >"$case_dir/known_hosts"
ssh_args=(
	-i "$case_dir/id_ed25519"
	-o BatchMode=yes
	-o StrictHostKeyChecking=yes
	-o "UserKnownHostsFile=$case_dir/known_hosts"
	-o SendEnv=-\*
	-o LogLevel=ERROR
	"root@$host"
)

for _ in {1..30}; do
	if ssh "${ssh_args[@]}" true 2>/dev/null; then
		break
	fi
	sleep 1
done
ssh "${ssh_args[@]}" true

remote_bootstrap() {
	"$mise_bin" bootstrap remote \
		--host "root@$host" \
		--identity-file "$case_dir/id_ed25519" \
		--source "$case_dir" \
		--remote-mise /opt/mise \
		--ssh-option StrictHostKeyChecking=yes \
		--ssh-option "UserKnownHostsFile=$case_dir/known_hosts" \
		--ssh-option 'SendEnv=-*' \
		--ssh-option LogLevel=ERROR \
		--only accounts,files,services,firewall,compose \
		--yes
}

remote_bootstrap
remote_bootstrap

ssh "${ssh_args[@]}" \
	'getent passwd mise-case >/dev/null
   test "$(stat -c %U:%G:%a /opt/mise-case)" = mise-case:mise-case:750
   test "$(stat -c %U:%G:%a /etc/mise-case.conf)" = root:root:640
   systemctl is-active --quiet mise-case.service
   systemctl is-enabled --quiet mise-case.service
   systemctl is-active --quiet docker.service
   nft list table inet mise_bootstrap | grep -q mise-bootstrap
   docker compose --project-directory /opt/mise-case --file /opt/mise-case/compose.yaml --project-name mise-bootstrap-smoke ps --status running --quiet | grep -q .'

ssh "${ssh_args[@]}" \
	'printf "drift\n" >/etc/mise-case.conf
   systemctl stop mise-case.service
   nft add rule inet mise_bootstrap input tcp dport 9 accept comment '"'"'manual-drift'"'"''

remote_bootstrap

ssh "${ssh_args[@]}" \
	'test "$(cat /etc/mise-case.conf)" = generation=1
   systemctl is-active --quiet mise-case.service
   ! nft list table inet mise_bootstrap | grep -q manual-drift
   docker compose --project-directory /opt/mise-case --file /opt/mise-case/compose.yaml --project-name mise-bootstrap-smoke ps --status running --quiet | grep -q .'
