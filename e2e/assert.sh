assert() {
  local actual
  actual="$($1)"
  if [[ "$actual" != "$2" ]]; then
    echo "Expected '$2' but got '$actual'"
    exit 1
  fi
}

assert_contains() {
  local actual
  actual="$($1)"
  if [[ "$actual" != *"$2"* ]]; then
    echo "Expected '$2' to be in '$actual'"
    exit 1
  fi
}

assert_fail() {
  if $1 2>&1; then
    echo "Expected failure but succeeded"
    exit 1
  fi
}
