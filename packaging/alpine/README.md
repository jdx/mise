To generate new release keys, first start the alpine docker container:

```bash
docker run -it --rm -v $(pwd):/work/mise ghcr.io/jdx/mise:alpine
```

And inside the container:

```bash
sudo su - packager
abuild-keygen -a -n
```

Then store them in GitHub secrets as `ALPINE_PRIV_KEY` and `ALPINE_PUB_KEY`.
Note the name of the private key file, it will be something like `-5f2b2c4e.rsa`.
Save that string as `ALPINE_KEY_ID` as another secret.

Also, the `ALPINE_GITLAB_TOKEN` needs to be rolled as well, use the [alpine gitlab portal](https://gitlab.alpinelinux.org/-/user_settings/personal_access_tokens)
to generate a new token and store it in Github secrets as `ALPINE_GITLAB_TOKEN`.
