"""Replace disposable copies of the CI-built mise, then execute the result."""

import hashlib
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile


def digest(path):
    with path.open("rb") as file:
        return hashlib.file_digest(file, "sha256").hexdigest()


def run(binary, args, env, cwd):
    result = subprocess.run(
        [str(binary), *args], env=env, cwd=cwd, text=True,
        stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=240,
    )
    print(result.stdout, flush=True)
    assert result.returncode == 0, f"{args} failed ({result.returncode})"
    return result.stdout


def main():
    source = Path(sys.argv[1]).resolve()
    original = digest(source)
    # Each case starts with the newly built updater, never the release installed
    # by the previous case. Exercise both packslip and pre-packslip releases.
    for version in ("2026.9.16", "2026.9.3", "2026.9.2"):
        with tempfile.TemporaryDirectory(prefix="mise-su-", ignore_cleanup_errors=True) as tmp:
            root = Path(tmp)
            binary = root / source.name
            shutil.copy2(source, binary)
            env = {key: value for key, value in os.environ.items()
                   if not key.startswith(("MISE_", "__MISE_"))}
            for token in ("MISE_GITHUB_TOKEN",):
                if os.environ.get(token):
                    env[token] = os.environ[token]
            env.update({
                "HOME": str(root / "home"), "USERPROFILE": str(root / "home"),
                "MISE_CONFIG_DIR": str(root / "config"),
                "MISE_DATA_DIR": str(root / "data"),
                "MISE_CACHE_DIR": str(root / "cache"),
                "MISE_STATE_DIR": str(root / "state"),
                "MISE_SELF_UPDATE_AVAILABLE": "true", "MISE_AUTO_UPDATE": "false",
                "MISE_LOG_LEVEL": "info", "MISE_CI": "true",
                # Exact versions must bypass even an impossible age policy.
                "MISE_SELF_UPDATE_MINIMUM_RELEASE_AGE": "100y",
            })
            (root / "home").mkdir()
            print(f"Testing real self-update to {version}", flush=True)
            output = run(binary, ["self-update", version, "--force", "--yes", "--no-plugins"], env, root)
            assert f"Updated mise to {version}" in output
            assert "Failed to update mise-shim.exe" not in output
            assert digest(binary) != original, "self-update did not replace the binary"
            installed = run(binary, ["--version"], env, root).split()[0]
            assert installed == version, (installed, version)
            if os.name != "nt":
                assert os.access(binary, os.X_OK), "installed binary lost execute permission"
    assert digest(source) == original, "test modified the CI binary"


if __name__ == "__main__":
    main()
