#!/usr/bin/env bash

export MISE_EXPERIMENTAL=1

# Test age keygen
assert_contains "mise age keygen --help" "Generate an age identity"

# Create a test key
KEYFILE="$HOME/.config/mise/test_age.txt"
mkdir -p "$HOME/.config/mise"
assert "mise age keygen --out $KEYFILE --force"

# Verify key file exists and contains age identity
assert "test -f $KEYFILE"
assert_contains "cat $KEYFILE" "AGE-SECRET-KEY-"

# Extract public key
PUBLIC_KEY=$(mise age keys show --key-file "$KEYFILE")
assert_contains "echo $PUBLIC_KEY" "age1"

# Test age keys ls
assert "mise age keys ls"

# Test age config
assert "mise age config"

# Test age recipients ls
assert "mise age recipients ls"

# Test age doctor
assert "mise age doctor"

# Create test file
echo "test secret" >"$HOME/test.txt"

# Test encryption
assert "mise age encrypt $HOME/test.txt --recipient $PUBLIC_KEY --out $HOME/test.age"
assert "test -f $HOME/test.age"

# Test decryption
MISE_AGE_KEY=$(cat "$KEYFILE" | grep AGE-SECRET-KEY)
export MISE_AGE_KEY
assert "mise age decrypt $HOME/test.age --out $HOME/test.decrypted"
assert_contains "cat $HOME/test.decrypted" "test secret"

# Test armor encryption
assert "mise age encrypt $HOME/test.txt --recipient $PUBLIC_KEY --armor --out $HOME/test.armor.age"
assert_contains "cat $HOME/test.armor.age" "BEGIN AGE ENCRYPTED FILE"

# Test age inspect
assert "mise age inspect $HOME/test.age"
assert "mise age inspect $HOME/test.armor.age --detailed"

# Test stdin/stdout encryption
echo "stdin test" | mise age encrypt --recipient "$PUBLIC_KEY" >"$HOME/stdin.age"
assert "test -f $HOME/stdin.age"

# Test stdin/stdout decryption
cat "$HOME/stdin.age" | assert_contains "mise age decrypt" "stdin test"

# Test rekey
KEYFILE2="$HOME/.config/mise/test_age2.txt"
assert "mise age keygen --out $KEYFILE2 --force"
PUBLIC_KEY2=$(mise age keys show --key-file "$KEYFILE2")

assert "mise age rekey $HOME/test.age --add-recipient $PUBLIC_KEY2 --identity-file $KEYFILE"
assert "test -f $HOME/test.age.rekey"

# Test import-ssh help (we don't test actual import as we may not have SSH keys)
assert_contains "mise age import-ssh --help" "Convert SSH public keys"

# Test recipients add help
assert_contains "mise age recipients add --help" "Add recipients"
