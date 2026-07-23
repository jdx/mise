#!/usr/bin/env python3
"""Generate deterministic local Homebrew handoff fixtures."""

import hashlib
import json
import os
import stat
import sys
import zipfile
from pathlib import Path

FIXED_TIME = (2020, 1, 1, 0, 0, 0)


def write_zip(path: Path, files: dict[str, tuple[bytes, int]]) -> str:
    path.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(path, "w", compression=zipfile.ZIP_DEFLATED) as archive:
        for name in sorted(files):
            contents, mode = files[name]
            info = zipfile.ZipInfo(name, FIXED_TIME)
            info.create_system = 3
            info.external_attr = (mode & 0xFFFF) << 16
            archive.writestr(info, contents)
    return hashlib.sha256(path.read_bytes()).hexdigest()


def app_files(name: str, payload: str) -> dict[str, tuple[bytes, int]]:
    executable = name.removesuffix(".app").replace(" ", "-").lower()
    plist = (
        '<?xml version="1.0" encoding="UTF-8"?>\n'
        '<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" '
        '"http://www.apple.com/DTDs/PropertyList-1.0.dtd">\n'
        '<plist version="1.0"><dict>'
        f"<key>CFBundleExecutable</key><string>{executable}</string>"
        f"<key>CFBundleIdentifier</key><string>dev.mise.handoff.{executable}</string>"
        "<key>CFBundlePackageType</key><string>APPL</string>"
        "<key>CFBundleShortVersionString</key><string>1.0.0</string>"
        "</dict></plist>\n"
    ).encode()
    return {
        f"{name}/Contents/Info.plist": (plist, stat.S_IFREG | 0o644),
        f"{name}/Contents/MacOS/{executable}": (
            f"#!/bin/sh\nprintf '%s\\n' '{payload}'\n".encode(),
            stat.S_IFREG | 0o755,
        ),
    }


def cask_api(token: str, url: str, sha256: str, artifacts: list[dict]) -> dict:
    return {
        "token": token,
        "version": "1.0.0",
        "url": url,
        "sha256": sha256,
        "artifacts": artifacts,
        "tap": "mise-test/handoff",
    }


def main() -> None:
    if len(sys.argv) != 3:
        raise SystemExit("usage: generate.py ROOT PORT")
    root = Path(sys.argv[1]).resolve()
    port = int(sys.argv[2])
    if root == Path("/") or not str(root).startswith(("/tmp/", "/private/")):
        raise SystemExit(f"fixture root must be temporary: {root}")
    root.mkdir(parents=True, exist_ok=True)
    api_dir = root / "api/cask"
    casks_dir = root / "tap/Casks"
    api_dir.mkdir(parents=True, exist_ok=True)
    casks_dir.mkdir(parents=True, exist_ok=True)

    fixtures = [
        ("mise-handoff-identical", "Mise Handoff Identical.app", False, "identical"),
        ("mise-handoff-different", "Mise Handoff Different.app", False, "archive"),
        ("mise-handoff-auto-updates", "Mise Handoff Auto Updates.app", True, "archive"),
    ]
    matrix = []
    for token, app_name, auto_updates, payload in fixtures:
        archive_name = f"{token}-1.0.0.zip"
        archive_path = root / "archives" / archive_name
        sha256 = write_zip(archive_path, app_files(app_name, payload))
        url = f"http://127.0.0.1:{port}/archives/{archive_name}"
        api = cask_api(token, url, sha256, [{"app": [app_name]}])
        (api_dir / f"{token}.json").write_text(json.dumps(api, indent=2) + "\n")
        lines = [
            f'cask "{token}" do',
            '  version "1.0.0"',
            f'  sha256 "{sha256}"',
            f'  url "{url}"',
            f'  name "{app_name.removesuffix(".app")}"',
            f'  desc "Deterministic mise handoff fixture {token}"',
            '  homepage "https://mise.jdx.dev/"',
        ]
        if auto_updates:
            lines.append("  auto_updates true")
        lines.extend([f'  app "{app_name}"', "end", ""])
        (casks_dir / f"{token}.rb").write_text("\n".join(lines))
        matrix.append(
            {
                "token": token,
                "class": "app",
                "auto_updates": auto_updates,
                "archive_sha256": sha256,
                "required_rows": [
                    "no_caskroom",
                    "same_version_mise_caskroom",
                    "different_target",
                    "retry_after_failure",
                    "homebrew_lifecycle",
                ],
            }
        )

    binary_token = "mise-handoff-binary"
    binary_name = "mise-handoff-tool"
    binary_archive = root / "archives" / f"{binary_token}-1.0.0.zip"
    binary_sha = write_zip(
        binary_archive,
        {binary_name: (b"#!/bin/sh\nprintf 'handoff-binary\\n'\n", stat.S_IFREG | 0o755)},
    )
    binary_url = f"http://127.0.0.1:{port}/archives/{binary_archive.name}"
    (api_dir / f"{binary_token}.json").write_text(
        json.dumps(
            cask_api(
                binary_token,
                binary_url,
                binary_sha,
                [{"binary": [binary_name], "target": f"$HOMEBREW_PREFIX/bin/{binary_name}"}],
            ),
            indent=2,
        )
        + "\n"
    )
    (casks_dir / f"{binary_token}.rb").write_text(
        "\n".join(
            [
                f'cask "{binary_token}" do',
                '  version "1.0.0"',
                f'  sha256 "{binary_sha}"',
                f'  url "{binary_url}"',
                '  name "mise handoff binary"',
                '  desc "Deterministic mise handoff binary fixture"',
                '  homepage "https://mise.jdx.dev/"',
                f'  binary "{binary_name}"',
                "end",
                "",
            ]
        )
    )
    matrix.append(
        {
            "token": binary_token,
            "class": "binary",
            "auto_updates": False,
            "archive_sha256": binary_sha,
            "required_rows": [
                "no_caskroom",
                "same_version_mise_caskroom",
                "different_target",
                "retry_after_failure",
                "homebrew_lifecycle",
            ],
        }
    )
    (root / "matrix.json").write_text(json.dumps(matrix, indent=2) + "\n")


if __name__ == "__main__":
    main()
