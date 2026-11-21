# Alpine packaging

This directory builds the `ghcr.io/jdx/mise:alpine` image. The `release-alpine`
workflow runs `scripts/release-alpine.sh` in that image to bump
`community/mise/APKBUILD` in Alpine's aports repository and open a merge
request. It runs for releases whose version ends in `0`, or when started by
hand, and reads its signing keys and GitLab credentials from GitHub secrets.

## Rotate the release keys

1. Start the Alpine container from the repository root:

   ```bash
   docker run -it --rm -v $(pwd):/work/mise ghcr.io/jdx/mise:alpine
   ```

2. Inside the container, generate a key pair as the `packager` user:

   ```bash
   sudo su - packager
   abuild-keygen -a -n
   ```

3. Store the private key in the `ALPINE_PRIV_KEY` secret and the public key in
   `ALPINE_PUB_KEY`.
4. Store the name of the private key file, which looks like `-5f2b2c4e.rsa`, in
   the `ALPINE_KEY_ID` secret.

## Rotate the GitLab credentials

Generate a new token in the
[Alpine GitLab portal](https://gitlab.alpinelinux.org/-/user_settings/personal_access_tokens)
and store it in the `ALPINE_GITLAB_TOKEN` secret. The workflow uses it to open
the merge request.

When the `ALPINE_GITLAB_SSH_KEY` secret is set, the workflow pushes the branch
to `jdxcode/aports` over SSH with that key instead of over HTTPS with the
token.
