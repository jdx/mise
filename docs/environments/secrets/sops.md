---
description: "Commit SOPS-encrypted .env.json, .env.yaml or .env.toml files and load their values with env._.file."
---

# SOPS files

Commit a [SOPS](https://getsops.io)-encrypted `.env.json`, `.env.yaml` or
`.env.toml` file and load it with [`env._.file`](/environments/#env-file). mise
decrypts it each time it loads the environment.

<span id="example"></span>

## Encrypt a file and load it {#encrypt-with-sops}

This walkthrough uses an age key, which mise can decrypt without the `sops` CLI.

### 1. Install the tools

```sh
mise use -g sops age
```

### 2. Create an age key

Reuse an existing age identity, or create one:

```sh
mkdir -p ~/.config/mise
mise exec -- age-keygen -o ~/.config/mise/age.txt
# Public key: age1...
```

Keep `age.txt` outside the repository.

### 3. Encrypt a file

Create `.env.json` with your values. This example uses a placeholder:

```json [.env.json]
{
  "API_TOKEN": "replace-with-your-token"
}
```

Encrypt it with the public key that `age-keygen` printed:

```sh
mise exec -- sops encrypt -i --age "age1..." .env.json
```

`-i` replaces the plaintext file with ciphertext. Commit the encrypted file. To
edit it later, point the `sops` CLI at the key; it reads `SOPS_AGE_KEY_FILE`,
not the `MISE_` variables:

```sh
SOPS_AGE_KEY_FILE="$HOME/.config/mise/age.txt" mise exec -- sops .env.json
```

### 4. Load it

```toml [mise.toml]
[env]
_.file = { path = ".env.json", redact = true }
```

mise now decrypts the file for `mise exec`, tasks and activated shells. Check
that the value arrives without printing it:

```sh
mise exec -- sh -c 'test -n "$API_TOKEN" && echo "API_TOKEN is set"'
```

`redact = true` keeps the values out of task output, but `mise env` still prints
them. See [Redaction and CI masking](/environments/secrets/#redaction).

## Choose a decryption method {#choose-a-decryption-method}

mise decrypts age-encrypted SOPS files itself, so the `sops` CLI is not needed
at runtime. For AWS KMS, GCP KMS, Azure Key Vault, HashiCorp Vault or PGP,
install the `sops` CLI, sign in to the provider, and set
[`sops.rops = false`](/configuration/settings.html#sops.rops) so mise runs
`sops decrypt` instead. mise looks for `sops` in the project's tools first, then
on `PATH`.

The `sops` CLI cannot read TOML. With `sops.rops = false`, use `.env.json` or
`.env.yaml`; an encrypted `.env.toml` fails with an error.

By default, mise stops with an error when it cannot decrypt a file, for example
when no key is found, the key is wrong, or the `sops` CLI is missing. Set
[`sops.strict = false`](/configuration/settings.html#sops.strict) to skip the
file and continue instead.

## Where mise finds the age key {#environment-variables}

mise uses the first of these that is set:

1. `MISE_SOPS_AGE_KEY`, or the
   [`sops.age_key`](/configuration/settings.html#sops.age_key) setting: the key
   itself
2. `MISE_SOPS_AGE_KEY_FILE`, or the
   [`sops.age_key_file`](/configuration/settings.html#sops.age_key_file)
   setting: a key file
3. `SOPS_AGE_KEY_FILE`: a key file
4. `SOPS_AGE_KEY`: the key itself
5. `~/.config/mise/age.txt`

The `MISE_` variables let you give mise a different key without changing the
SOPS configuration that other tools use.

These variables can also come from `[env]`, as long as they appear before the
encrypted file. The settings, including their `MISE_` variables set in your
shell, still take precedence:

```toml [mise.toml]
[env]
MISE_SOPS_AGE_KEY_FILE = "~/age.txt"
_.file = ".env.yaml"
```

A key file can hold several identities, one per line. mise ignores blank lines
and lines that start with `#`, and tries each identity.

## Share with a team {#share-with-a-team}

Encrypt to each team member's public key, for example
`sops encrypt -i --age "age1...,age1..." .env.json`, or add a `.sops.yaml` file
with creation rules so `sops` picks the recipients itself. See the
[SOPS documentation](https://getsops.io/docs/).

## Settings

<script setup>
import Settings from '/components/settings.vue';
</script>
<Settings child="sops" :level="3" />
