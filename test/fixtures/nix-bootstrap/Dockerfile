FROM nixos/nix:2.35.2@sha256:617d914dba5384bf75adf17081583b69371031ec7defce36c34c5fa14fc819b0

# The image includes a fixed nixpkgs tree. No channel update is performed.
RUN nix-build --out-link /test-tools --expr 'let pkgs = import <nixpkgs> {}; in pkgs.buildEnv { name = "mise-nix-test-tools"; paths = [ pkgs.gnused pkgs.patchelf pkgs.openssl.out pkgs.stdenv.cc.cc.lib pkgs.jq pkgs.glibc ]; }'
RUN mkdir /input
ENV PATH="/test-tools/bin:/nix/var/nix/profiles/default/bin:/bin"
ENV NIX_CONFIG="experimental-features = nix-command flakes"
COPY run.sh /test/run.sh
ENTRYPOINT ["bash", "/test/run.sh"]
