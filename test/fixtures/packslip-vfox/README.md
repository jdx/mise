# Signed vfox plugin fixture

These artifacts came from the successful publisher pilot:
https://github.com/mise-plugins/vfox-bfs/actions/runs/34177222330

- Commit: `17261ce5c5588900763224ba9d612a8aad843149`
- Manifest version: `0.1.0-dev.2`
- Signer: `https://github.com/mise-plugins/vfox-bfs/.github/workflows/release.yml@refs/heads/codex/packslip-releases`
- Archive SHA256: `23ddff8ccce302ef4b20ddcd280cf04704b1a75e6f456a7946d410885e261d09`

The archive is unmodified; the bundle has only outer JSON whitespace normalized.
Its signed payload and signature are unchanged. The archive includes the plugin's license. It contains no Git directory. The
workflow verified the signature and digest and installed bfs from this archive
on Linux and macOS. No GitHub release was published for this development version.

The local fixture server exposes release metadata for this signed artifact. It
does not rewrite the manifest, regenerate its signature, or disable verification.
