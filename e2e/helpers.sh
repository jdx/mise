#!/usr/bin/env bash

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" &>/dev/null && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# Prints the mise binary the tests run: MISE_E2E_BIN when set, otherwise this
# checkout's debug build. There is no fallback to a mise found on PATH, because
# that one does not contain the change being tested and the run would still
# pass. A `target` symlink whose destination is gone counts as no build.
resolve_mise_bin() {
  if [[ -n ${MISE_E2E_BIN:-} ]]; then
    if [[ ! -f $MISE_E2E_BIN || ! -x $MISE_E2E_BIN ]]; then
      err "MISE_E2E_BIN is set to '$MISE_E2E_BIN', which is not an executable file."
      return 1
    fi
    # Outside Docker the tests find the binary by name on PATH. A Docker run
    # mounts the file as `mise`, so any name works there.
    if [[ ${MISE_E2E_DOCKER:-0} != 1 && $(basename "$MISE_E2E_BIN") != mise ]]; then
      err "MISE_E2E_BIN is set to '$MISE_E2E_BIN', but the tests call the binary by name: the file must be named 'mise'."
      return 1
    fi
    printf '%s\n' "$MISE_E2E_BIN"
    return
  fi
  local candidate="${CARGO_TARGET_DIR:-$ROOT/target}/debug/mise"
  if [[ -f $candidate && -x $candidate ]]; then
    printf '%s\n' "$candidate"
    return
  fi
  err "No mise build at '$candidate'. Run 'mise run build' first, or set MISE_E2E_BIN=/path/to/mise to test another mise binary on purpose."
  return 1
}

# Docker containers that outlive a test.
#
# With MISE_E2E_DOCKER=1, run_all_tests gives each running test a slot number
# (MISE_E2E_DOCKER_SLOT) that no other running test holds, and run_test runs the
# test with `docker exec` in the slot's container, started by the first test to
# use it. The slot's host directories, which back /tmp, /root and /home, are in
# $MISE_E2E_DOCKER_SLOT_ROOT/<slot>; $MISE_E2E_DOCKER_RUN_ID tells the
# containers of one run_all_tests process from those of another.

docker_slot_container_name() {
  printf 'mise-e2e-%s-%s\n' "$MISE_E2E_DOCKER_RUN_ID" "$1"
}

# The container's first process is a reaper (docker --init), which has the
# keeper as its only child: `sleep infinity` under the name mise-e2e-keeper.
# Killing the keeper would stop the container, so it is the one process the kill
# and the reset leave alone besides the reaper.
docker_slot_keeper_command() {
  printf '%s\n' 'exec -a mise-e2e-keeper sleep infinity'
}

# Kills every process in the container but the reaper, the keeper and its own
# shell. Processes are stopped before they are killed so that they cannot fork
# in between; a new one that slips through is caught by the next round.
DOCKER_SLOT_KILL_SCRIPT=$(
  cat <<'SCRIPT'
for _ in 1 2 3 4 5; do
  pids=()
  for dir in /proc/[0-9]*; do
    pid=${dir#/proc/}
    [[ $pid == 1 || $pid == $$ ]] && continue
    arg0=
    IFS= read -r -d '' arg0 <"$dir/cmdline" 2>/dev/null || true
    [[ $arg0 == mise-e2e-keeper ]] && continue
    pids+=("$pid")
  done
  [[ ${#pids[@]} == 0 ]] && break
  kill -STOP "${pids[@]}" 2>/dev/null || true
  kill -9 "${pids[@]}" 2>/dev/null || true
done
exit 0
SCRIPT
)

# Puts the container back as a new one is: waits for the killed processes to be
# gone, empties the places a test can write to, and gives the mount points back
# their owners and mode. Arguments: the owners of /tmp, /home and /root as
# uid:gid, then the names /dev had at the start. Exits 3 when a process is still
# alive after five seconds.
DOCKER_SLOT_RESET_SCRIPT=$(
  cat <<'SCRIPT'
tmp_owner=$1 home_owner=$2 root_owner=$3
shift 3
baseline=" $* "

# A zombie holds nothing, and the reaper collects it.
alive() {
  local dir pid line arg0
  for dir in /proc/[0-9]*; do
    pid=${dir#/proc/}
    [[ $pid == 1 || $pid == $$ ]] && continue
    IFS= read -r line <"$dir/stat" 2>/dev/null || continue
    [[ ${line##*) } == Z* ]] && continue
    arg0=
    IFS= read -r -d '' arg0 <"$dir/cmdline" 2>/dev/null || true
    [[ $arg0 == mise-e2e-keeper ]] || return 0
  done
  return 1
}
for _ in $(seq 1 100); do
  alive || break
  sleep 0.05
done
if alive; then exit 3; fi

# The directories that are writable with a read-only root filesystem.
dirs=()
for dir in /tmp /root /home /dev/shm /dev/mqueue; do
  [[ -d $dir ]] && dirs+=("$dir")
done
find "${dirs[@]}" -mindepth 1 -delete || exit 4
for entry in /dev/* /dev/.[!.]* /dev/..?*; do
  [[ -e $entry || -L $entry ]] || continue
  case "$baseline" in *" ${entry#/dev/} "*) continue ;; esac
  find "$entry" -delete || exit 5
done
ipcrm --all 2>/dev/null || true

chown "$tmp_owner" /tmp
chown "$home_owner" /home
chown "$root_owner" /root
chown 0:0 /dev/shm
chmod 1777 /tmp /home /root /dev/shm
SCRIPT
)

# Returns 0 when the container is clean for the next test.
docker_slot_reset() {
  local container="$1" slot_dir="$2" status
  local owners=() baseline=() line
  while IFS= read -r line; do
    owners+=("$line")
  done <"$slot_dir/owners"
  read -r -a baseline <"$slot_dir/dev" || true
  for _ in 1 2 3; do
    docker exec --user root "$container" bash -c "$DOCKER_SLOT_KILL_SCRIPT" >/dev/null 2>&1 || true
    status=0
    docker exec --user root "$container" bash -c "$DOCKER_SLOT_RESET_SCRIPT" reset \
      "${owners[@]}" ${baseline[@]+"${baseline[@]}"} >/dev/null 2>&1 || status=$?
    [[ $status == 0 ]] && return 0
    # Only a process that survived the kill is worth another round.
    [[ $status == 3 ]] || return 1
  done
  return 1
}

# Removes the container and its host directories, whatever state they are in.
# What the test created in the directories is emptied from inside first: it can
# belong to root, whom the host user cannot delete from.
docker_slot_discard() {
  local container="$1" slot_dir="$2"
  docker exec --user root "$container" bash -c "$DOCKER_SLOT_KILL_SCRIPT" >/dev/null 2>&1 || true
  docker exec --user root "$container" find /tmp /root /home -mindepth 1 -delete >/dev/null 2>&1 || true
  docker rm -f "$container" >/dev/null 2>&1 || true
  rm -rf "$slot_dir" 2>/dev/null || true
}

# Removes every container this run_all_tests process started, and the host
# directories below $1. The containers are found by label, so one that was being
# started when the run was interrupted is found too.
docker_slots_teardown() {
  local slot_root="$1" ids id pids=()
  if [[ -n $(ls -A "$slot_root" 2>/dev/null) ]]; then
    ids="$(docker ps -aq --filter "label=mise-e2e-run=$MISE_E2E_DOCKER_RUN_ID" 2>/dev/null || true)"
    for id in $ids; do
      (
        docker exec --user root "$id" bash -c "$DOCKER_SLOT_KILL_SCRIPT" || true
        docker exec --user root "$id" find /tmp /root /home -mindepth 1 -delete || true
        docker rm -f "$id"
      ) >/dev/null 2>&1 &
      pids+=($!)
    done
    for id in ${pids[@]+"${pids[@]}"}; do
      wait "$id" || true
    done
  fi
  rm -rf "$slot_root" 2>/dev/null || true
}
