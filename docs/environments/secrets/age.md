---
description: "Encrypt individual environment variables in mise.toml with age, and decrypt them when mise loads the environment."
---

# age values <Badge type="warning" text="experimental" />

Store individual secrets in `mise.toml` as [age](https://age-encryption.org)
encrypted values. mise encrypts and decrypts them itself, so you need only an
age identity or an SSH key. Commit the encrypted values and share identities
outside the repository.

::: warning Experimental
age values are experimental. Enable them with `experimental = true` under
`[settings]`, or run `mise settings experimental=true`.
:::

Decrypted values behave like other `[env]` values: your activated shell, tasks
and `mise env` see the plaintext. mise redacts them in task output by default.

## Quick start {#quick-start}

### 1. Get an identity

An existing SSH key works: `~/.ssh/id_ed25519` or `~/.ssh/id_rsa`, with its
`.pub` file beside it. To use a dedicated age identity instead, create one. Skip
`age-keygen` if `age.txt` already holds an identity you want to keep:

```sh
mise use -g age
mkdir -p ~/.config/mise
mise exec -- age-keygen -o ~/.config/mise/age.txt
# Public key: age1...
```

The public key is a recipient: share it with people who encrypt values for you.
`age.txt` holds the private identity that decrypts them; keep it outside the
repository.

### 2. Encrypt a value

```sh
mise set --age-encrypt --prompt DB_PASSWORD
# Enter value for DB_PASSWORD: [hidden input]
```

`--prompt` reads the value without echoing it, so the plaintext stays out of
your shell history. mise writes the ciphertext to `mise.toml`:

```toml [mise.toml]
[env]
DB_PASSWORD = { age = "<base64>" }
```

### 3. Use it

mise decrypts the value before it starts a command or task:

```sh
mise exec -- sh -c 'test -n "$DB_PASSWORD" && echo "DB_PASSWORD is available"'
```

`mise env` and `mise set DB_PASSWORD` print the decrypted value; `mise set`
with no arguments shows `[redacted]`.

## Who can decrypt {#defaults-for-recipients-encryption}

Without recipient flags, `mise set --age-encrypt` encrypts to:

- the public keys of the identities in `~/.config/mise/age.txt`, or in the file
  named by
  [`age.key_file`](/configuration/settings.html#age.key_file) when that setting
  is set
- `~/.ssh/id_ed25519` and `~/.ssh/id_rsa`, when their `.pub` files exist

If none of these exist, the command fails and asks for recipients.

### Share with a team {#share-with-a-team}

Pass `--age-recipient` once for each teammate's age public key, or
`--age-ssh-recipient` with an SSH public key or the path to one.
`--age-key-file` adds the public keys of the identities in another key file as
recipients. When you pass any of these flags, mise encrypts only to the
recipients you name:

```sh
mise set --age-encrypt --prompt \
  --age-recipient age1... \
  --age-ssh-recipient ~/.ssh/teammate.pub \
  DB_PASSWORD
```

To change a value or add a recipient, run `mise set --age-encrypt` again with
the full recipient list. See [`mise set`](/cli/set.html) for every flag.

## Decryption identities {#decryption-identities}

mise tries identities from these sources, in order:

1. The `MISE_AGE_KEY` environment variable, which can hold one or more raw
   `AGE-SECRET-KEY-...` lines or the contents of an age identity file
2. The files in [`age.identity_files`](/configuration/settings.html#age.identity_files)
3. The file in [`age.key_file`](/configuration/settings.html#age.key_file)
4. `~/.config/mise/age.txt`, if it exists
5. The SSH keys in
   [`age.ssh_identity_files`](/configuration/settings.html#age.ssh_identity_files),
   then `~/.ssh/id_ed25519` and `~/.ssh/id_rsa`

Paths in `age.key_file`, `age.identity_files` and `age.ssh_identity_files`
resolve against the config root of the file that sets them. They support
templates, including <span v-pre>`{{ config_root }}`</span> and values from
`env`. Absolute paths and paths that start with `~` keep their meaning.

Decryption is strict by default: when no identity is found, no identity can
decrypt a value, or the payload is invalid, mise fails instead of continuing
with a partial environment. To skip values it cannot decrypt and resolve the
rest, set [`age.strict`](/configuration/settings.html#age.strict) to `false`:

```sh
mise settings age.strict=false
```

Decrypted values are redacted by default. Set `redact = false` on the variable
to opt out:

```toml [mise.toml]
[env]
DB_PASSWORD = { age = "<base64>", redact = false }
```

## Storage format {#storage-format}

The stored payload is base64-encoded age ciphertext, not encoded plaintext.
mise writes small values as `KEY = { age = "<base64>" }`. When the ciphertext is
larger than 1 KiB, mise compresses it with zstd and writes
`KEY = { age = { value = "<base64>", format = "zstd" } }`. When reading, mise
also accepts the table form without `format` or with `format = "raw"`, which
both mean uncompressed.

## Settings

<script setup>
import Settings from '/components/settings.vue';
</script>
<Settings child="age" :level="3" />
