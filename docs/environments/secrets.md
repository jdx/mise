# Secrets <Badge type="warning" text="experimental" />

Because env vars in mise.toml can store sensitive information, mise has built-in support for reading encrypted secrets
from files. Currently, this is done with a [sops](https://getsops.io) implementation however other secret backends could
be added in the future.

Secrets are `.env.(json|yaml|toml)` files with a simple structure, for example:

```json
{
  "AWS_ACCESS_KEY_ID": "AKIAIOSFODNN7EXAMPLE",
  "AWS_SECRET_ACCESS_KEY": "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY"
}
```

Env vars from this can be imported into a mise config with the following:

```toml
[env]
_.file = ".env.json"
```

mise will automatically use a secret backend like sops if the file is encrypted.

## sops

mise uses the rust [rops](https://github.com/gibbz00/rops) library to interact with [sops](https://getsops.io) files. If
you encrypt a sops file, mise will automatically decrypt it when reading the file. sops files can be in json, yaml, or
toml format—however if you want to use toml you'll need to use the rops cli instead of sops. Otherwise, either sops or
rops will work fine.

::: info Currently age is the only sops encryption method supported. :::

In order to encrypt a file with sops, you'll first need to install it (`mise use -g sops`). You'll also need to install
[age](https://github.com/FiloSottile/age) (`mise use -g age`) to generate a keypair for sops to use if you have not
already done so.

To generate a keypair with age run the following and note the public key that is output to use in the next command to
`sops`:

```sh
$ age-keygen -o ~/.config/mise/age.txt
Public key: <public key>
```

Assuming we have a `.env.json` file like at the top of this doc, we can now encrypt it with sops:

```sh
sops encrypt -i --age "<public key>" .env.json
```

::: tip The `-i` here overwrites the file with an encrypted version. This encrypted version is safe to commit into your
repo as without the private key (`~/.config/mise/age.txt` in this case) the file is useless.

You can later decrypt the file with `sops decrypt -i .env.json` or edit it in EDITOR with `sops edit .env.json`.
However, you'll first need to set SOPS_AGE_KEY_FILE to `~/.config/mise/age.txt` to decrypt the file. :::

Lastly, we need to add the file to our mise config which can be done with `mise set _.file=.env.json`.

Now when you run `mise env` you should see the env vars from the file:

```sh
$ mise env
export AWS_ACCESS_KEY_ID=AKIAIOSFODNN7EXAMPLE
export AWS_SECRET_ACCESS_KEY=wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY
```

### `sops` Settings

<script setup>
import Settings from '/components/settings.vue';
</script>
<Settings child="sops" :level="4" />
