#!/usr/bin/env bash

# Differential Homebrew state oracle. Normalize only producer identity,
# wall-clock values, producer-local source paths, and the tap-global head for
# homebrew/core or homebrew/cask. The fixture recorder separately pins the full
# per-definition payload (including ruby_source_checksum); a global tap head can
# advance for an unrelated package during one oracle run. Third-party tap heads
# and immutable package facts (including built_on and source_modified_time)
# remain exact.

brew_oracle_mode() {
  if [[ ${BREW_ORACLE_PLATFORM:-$(uname)} == Darwin ]]; then
    stat -f '%Lp' "$1"
  else
    stat -c '%a' "$1"
  fi
}

brew_oracle_normalize_json() {
  local input=$1 output=$2
  local filename=${input##*/}
  if [[ $filename == INSTALL_RECEIPT.json ||
    $filename == *-INSTALL_RECEIPT.json ]]; then
    if ! jq -e '
      has("homebrew_version") and has("time") and
      has("loaded_from_api") and (.loaded_from_api | type == "boolean") and
      (.source | type == "object" and has("path") and has("tap_git_head")) and
      ((has("used_options") | not) or has("source_modified_time")) and
      ((has("built_on") | not) or (.built_on == null) or
        (.built_on | type == "object")) and
      ((.source.tap_git_head == null) or
        (.source.tap_git_head | type == "string" and test("^[0-9a-f]{40}$")))
    ' "$input" >/dev/null; then
      printf 'invalid Homebrew receipt in oracle snapshot: %s\n' "$input" >&2
      jq -c '{
        keys: keys,
        loaded_from_api_type: (.loaded_from_api | type),
        source_type: (.source | type),
        source_keys: (if (.source | type) == "object" then (.source | keys) else [] end),
        built_on_type: (.built_on | type)
      }' "$input" >&2 || true
      return 1
    fi
    jq -S '
      .homebrew_version = "<NORMALIZED>" |
      .time = "<NORMALIZED>" |
      .loaded_from_api = "<NORMALIZED>" |
      .source.path = "<NORMALIZED>" |
      if (.source.tap == "homebrew/core" or .source.tap == "homebrew/cask") and
        (.source.tap_git_head | type == "string")
      then .source.tap_git_head = "<NORMALIZED TAP-GLOBAL HEAD>"
      else . end
    ' "$input" >"$output"
  elif [[ $input == */sbom.spdx.json ]]; then
    jq -e '
      .creationInfo |
      type == "object" and has("created") and
      (.creators | type == "array" and length == 1 and
        (.[0] | type == "string" and
          startswith("Tool: https://github.com/Homebrew/brew@")))
    ' "$input" >/dev/null
    jq -S '
      .creationInfo.created = "<NORMALIZED>" |
      .creationInfo.creators[0] = "<NORMALIZED>"
    ' "$input" >"$output"
  else
    jq -S . "$input" >"$output"
  fi
}

brew_oracle_normalize_path() {
  # Homebrew assigns cask metadata a wall-clock installation directory.
  sed -E 's#(/\.metadata/[^/]+)/[0-9]{14}\.[0-9]{3}(/|$)#\1/<TIMESTAMP>\2#'
}

brew_oracle_macho_fingerprint() {
  local path=$1 description=${2:-}
  [[ -n $description ]] || description=$(file -b "$path")
  # Ad-hoc signatures and their embedded byte layout are producer-local.
  # Compare the executable contract: architecture, dylib identity,
  # dependencies, rpaths, and a valid strict signature. The outer snapshot
  # still compares entry kind, mode, path, and every symlink target exactly.
  codesign --verify --strict "$path" >/dev/null 2>&1
  {
    printf '%s\n' "$description"
    otool -arch all -D "$path" 2>/dev/null || true
    otool -arch all -L "$path"
    otool -arch all -l "$path" | awk '
      $1 == "cmd" { command = $2 }
      command == "LC_RPATH" && $1 == "path" { print $0; command = "" }
    '
  } | shasum -a 256 | awk '{print $1}'
}

brew_oracle_snapshot() {
  local output=$1
  shift
  local scratch entries regular
  scratch=$(mktemp -d)
  entries="$scratch/entries"
  regular="$scratch/regular"
  : >"$entries"
  : >"$regular"
  local BREW_ORACLE_PLATFORM
  BREW_ORACLE_PLATFORM=$(uname)
  local spec label root
  for spec in "$@"; do
    label=${spec%%=*}
    root=${spec#*=}
    [[ $label != "$spec" && -e $root ]] || return 1
    while IFS= read -r path; do
      local relative normalized mode digest target
      if [[ $path == "$root" ]]; then
        relative=.
      else
        relative=${path#"$root"/}
      fi
      relative="$label/$relative"
      if [[ $relative == */.metadata/* ]]; then
        relative=$(printf '%s' "$relative" | brew_oracle_normalize_path)
      fi
      if [[ -L $path ]]; then
        mode=$(brew_oracle_mode "$path")
        target=$(readlink "$path")
        printf 'l %s %s -> %s\n' "$mode" "$relative" "$target" >>"$entries"
      elif [[ -d $path ]]; then
        mode=$(brew_oracle_mode "$path")
        printf 'd %s %s\n' "$mode" "$relative" >>"$entries"
      elif [[ -f $path ]]; then
        if [[ $path == */INSTALL_RECEIPT.json ||
          $path == */sbom.spdx.json ||
          $path == */Caskroom/*/.metadata/config.json ||
          $path == */Caskroom/*/.metadata/*/Casks/*.json ]]; then
          mode=$(brew_oracle_mode "$path")
          normalized="$scratch/normalized.json"
          brew_oracle_normalize_json "$path" "$normalized"
          digest=$(shasum -a 256 "$normalized" | awk '{print $1}')
          printf 'f %s %s %s\n' "$mode" "$relative" "$digest" >>"$entries"
        else
          printf '%s\0%s\0' "$path" "$relative" >>"$regular"
        fi
      fi
    done < <(find "$root" -print | LC_ALL=C sort)
  done

  # `uname`, `stat`, `file`, and `shasum` once ran once per payload file. Large
  # formulae such as ansible contain tens of thousands of ordinary files,
  # turning three truthful topology snapshots into more than 100,000
  # subprocesses. Batch modes and hashes, and reserve Mach-O probes for signed
  # binary candidates, while retaining the same per-path snapshot contract.
  local -a paths relatives modes macho ordinary_paths ordinary_modes
  local -a ordinary_relatives digests
  local path relative mode line digest description macho_index
  local batch_size=256 index exhausted=0
  local mode_output="$scratch/modes" hash_output="$scratch/hashes"
  local macho_output="$scratch/macho-indexes"
  while ((exhausted == 0)); do
    paths=()
    relatives=()
    for ((index = 0; index < batch_size; index++)); do
      if ! IFS= read -r -d '' path <&3; then
        exhausted=1
        break
      fi
      IFS= read -r -d '' relative <&3 || return 1
      paths+=("$path")
      relatives+=("$relative")
    done
    ((${#paths[@]} > 0)) || break

    if [[ $BREW_ORACLE_PLATFORM == Darwin ]]; then
      stat -f '%Lp' "${paths[@]}" >"$mode_output" || return 1
    else
      stat -c '%a' "${paths[@]}" >"$mode_output" || return 1
    fi
    modes=()
    while IFS= read -r mode; do
      modes+=("$mode")
    done <"$mode_output"
    ((${#modes[@]} == ${#paths[@]})) || return 1

    macho=()
    if [[ $BREW_ORACLE_PLATFORM == Darwin ]]; then
      # Bottle loadable modules are commonly extensionless and mode 0444, so
      # executable bits and suffixes cannot identify every Mach-O payload.
      # Inspect the four-byte magic for the entire batch in one process; only
      # actual Mach-O files pay the more expensive `file`/`otool` probes.
      perl -e '
        use strict;
        use warnings;
        my %macho = map { $_ => 1 } qw(
          cafebabe bebafeca cafebabf bfbafeca
          feedface cefaedfe feedfacf cffaedfe
        );
        for my $index (0 .. $#ARGV) {
          open my $file, "<", $ARGV[$index]
            or die "cannot inspect $ARGV[$index]: $!\n";
          binmode $file;
          my $read = read $file, my $magic, 4;
          die "cannot read $ARGV[$index]: $!\n" unless defined $read;
          print "$index\n" if $read == 4 && $macho{unpack "H8", $magic};
        }
      ' "${paths[@]}" >"$macho_output" || return 1
      while IFS= read -r macho_index; do
        [[ $macho_index =~ ^[0-9]+$ ]] || return 1
        ((macho_index < ${#paths[@]})) || return 1
        macho[macho_index]=1
      done <"$macho_output"
    fi

    ordinary_paths=()
    ordinary_modes=()
    ordinary_relatives=()
    for index in "${!paths[@]}"; do
      description=
      path=${paths[$index]}
      if [[ ${macho[$index]:-} == 1 ]]; then
        description=$(file -b "$path")
      fi
      if [[ $description == *Mach-O* ]]; then
        digest=$(brew_oracle_macho_fingerprint \
          "$path" "$description")
        printf 'f %s %s %s\n' \
          "${modes[$index]}" "${relatives[$index]}" "$digest" >>"$entries"
      else
        ordinary_paths+=("${paths[$index]}")
        ordinary_modes+=("${modes[$index]}")
        ordinary_relatives+=("${relatives[$index]}")
      fi
    done

    if ((${#ordinary_paths[@]} > 0)); then
      shasum -a 256 "${ordinary_paths[@]}" >"$hash_output" || return 1
      digests=()
      while IFS= read -r line; do
        digest=${line%% *}
        [[ $digest =~ ^[0-9a-f]{64}$ ]] || return 1
        digests+=("$digest")
      done <"$hash_output"
      ((${#digests[@]} == ${#ordinary_paths[@]})) || return 1
      for index in "${!ordinary_paths[@]}"; do
        printf 'f %s %s %s\n' \
          "${ordinary_modes[$index]}" "${ordinary_relatives[$index]}" \
          "${digests[$index]}" >>"$entries"
      done
    fi
  done 3<"$regular"
  LC_ALL=C sort "$entries" >"$output"
  rm -rf "$scratch"
}

brew_oracle_diff() {
  local brew_snapshot=$1 mise_snapshot=$2
  if ! diff -u "$brew_snapshot" "$mise_snapshot"; then
    printf 'first divergent snapshot line:\n' >&2
    diff -u "$brew_snapshot" "$mise_snapshot" | sed -n '/^[+-][^+-]/p' | head -1 >&2
    return 1
  fi
}

brew_oracle_canonicalize_api_fixture() {
  local kind=$1 input=$2 output=$3
  [[ $kind == formula || $kind == cask ]] || return 1
  # Analytics, generation time, the tap-global head, and vulnerability advisory
  # counts can change without changing the package definition or install state.
  # Keep per-definition source checksums and every operational field exact.
  jq -S 'del(.analytics, .generated_date, .tap_git_head, .vulnerabilities)' \
    "$input" >"$output"
}

brew_oracle_record_api_fixture() {
  local kind=$1 token=$2 expected_version=$3 expected_sha=$4
  local result_dir=${MISE_BREW_ORACLE_RESULT_DIR:-}
  local version_filter url safe_token raw canonical actual_version actual_sha

  [[ $kind == formula || $kind == cask ]] || return 1
  [[ $token =~ ^[a-z0-9@+._-]+$ ]] || return 1
  [[ $expected_sha =~ ^[0-9a-f]{64}$ ]] || return 1
  [[ -d $result_dir && ! -L $result_dir ]] || return 1
  safe_token=${token//@/_at_}
  raw="$result_dir/api-$kind-$safe_token.raw.json"
  canonical="$result_dir/api-$kind-$safe_token.json"
  [[ ! -e $raw && ! -L $raw && ! -e $canonical && ! -L $canonical ]] || return 1
  url="https://formulae.brew.sh/api/$kind/$token.json"
  curl --retry 5 --retry-all-errors --retry-delay 2 --retry-max-time 90 \
    --connect-timeout 10 --max-time 30 -fsSL "$url" -o "$raw"
  if [[ $kind == formula ]]; then
    version_filter=.versions.stable
  else
    version_filter=.version
  fi
  actual_version=$(jq -er "$version_filter" "$raw")
  [[ $actual_version == "$expected_version" ]] || {
    echo "brew oracle fixture drift: $kind:$token version $actual_version != $expected_version" >&2
    return 1
  }
  brew_oracle_canonicalize_api_fixture "$kind" "$raw" "$canonical"
  actual_sha=$(shasum -a 256 "$canonical" | awk '{print $1}')
  [[ $actual_sha == "$expected_sha" ]] || {
    echo "brew oracle fixture drift: $kind:$token digest $actual_sha != $expected_sha" >&2
    return 1
  }
  printf '%s:%s version=%s sha256=%s\n' \
    "$kind" "$token" "$actual_version" "$actual_sha" \
    >>"$result_dir/api-fixtures.txt"
}
