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
    formula_dir = root / "tap/Formula"
    api_dir.mkdir(parents=True, exist_ok=True)
    casks_dir.mkdir(parents=True, exist_ok=True)
    formula_dir.mkdir(parents=True, exist_ok=True)

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

    mixed_token = "mise-handoff-mixed"
    mixed_app = "Mise Handoff Mixed.app"
    mixed_binary = "mise-handoff-mixed-tool"
    mixed_archive = root / "archives" / f"{mixed_token}-1.0.0.zip"
    mixed_files = app_files(mixed_app, "mixed-app")
    mixed_files[mixed_binary] = (
        b"#!/bin/sh\nprintf 'handoff-mixed\\n'\n",
        stat.S_IFREG | 0o755,
    )
    mixed_sha = write_zip(mixed_archive, mixed_files)
    mixed_url = f"http://127.0.0.1:{port}/archives/{mixed_archive.name}"
    (api_dir / f"{mixed_token}.json").write_text(
        json.dumps(
            cask_api(
                mixed_token,
                mixed_url,
                mixed_sha,
                [{"app": [mixed_app]}, {"binary": [mixed_binary]}],
            ),
            indent=2,
        )
        + "\n"
    )
    (casks_dir / f"{mixed_token}.rb").write_text(
        "\n".join(
            [
                f'cask "{mixed_token}" do',
                '  version "1.0.0"',
                f'  sha256 "{mixed_sha}"',
                f'  url "{mixed_url}"',
                '  name "mise handoff mixed"',
                '  desc "Deterministic mixed app and binary fixture"',
                '  homepage "https://mise.jdx.dev/"',
                f'  app "{mixed_app}"',
                f'  binary "{mixed_binary}"',
                "end",
                "",
            ]
        )
    )
    matrix.append(
        {
            "token": mixed_token,
            "class": "mixed_app_binary",
            "auto_updates": False,
            "archive_sha256": mixed_sha,
            "required_rows": [
                "no_caskroom",
                "same_version_mise_caskroom",
                "different_target",
                "retry_after_failure",
                "homebrew_lifecycle",
            ],
        }
    )

    dependency_formula = "mise-handoff-dependency"
    dependency_archive = root / "archives" / f"{dependency_formula}-1.0.0.zip"
    dependency_sha = write_zip(
        dependency_archive,
        {
            dependency_formula: (
                b"#!/bin/sh\nprintf 'handoff-dependency\\n'\n",
                stat.S_IFREG | 0o755,
            )
        },
    )
    dependency_url = f"http://127.0.0.1:{port}/archives/{dependency_archive.name}"
    (formula_dir / f"{dependency_formula}.rb").write_text(
        "\n".join(
            [
                "class MiseHandoffDependency < Formula",
                '  desc "Deterministic handoff dependency fixture"',
                '  homepage "https://mise.jdx.dev/"',
                f'  url "{dependency_url}"',
                '  version "1.0.0"',
                f'  sha256 "{dependency_sha}"',
                "",
                "  def install",
                f'    bin.install "{dependency_formula}"',
                "  end",
                "end",
                "",
            ]
        )
    )
    dependency_token = "mise-handoff-binary-dependency"
    dependency_binary = "mise-handoff-dependent-tool"
    dependency_cask_archive = root / "archives" / f"{dependency_token}-1.0.0.zip"
    dependency_cask_sha = write_zip(
        dependency_cask_archive,
        {
            dependency_binary: (
                b"#!/bin/sh\nprintf 'handoff-dependent\\n'\n",
                stat.S_IFREG | 0o755,
            )
        },
    )
    dependency_cask_url = (
        f"http://127.0.0.1:{port}/archives/{dependency_cask_archive.name}"
    )
    dependency_api = cask_api(
        dependency_token,
        dependency_cask_url,
        dependency_cask_sha,
        [{"binary": [dependency_binary]}],
    )
    dependency_api["depends_on"] = {
        "formula": [f"mise-test/handoff/{dependency_formula}"]
    }
    (api_dir / f"{dependency_token}.json").write_text(
        json.dumps(dependency_api, indent=2) + "\n"
    )
    (casks_dir / f"{dependency_token}.rb").write_text(
        "\n".join(
            [
                f'cask "{dependency_token}" do',
                '  version "1.0.0"',
                f'  sha256 "{dependency_cask_sha}"',
                f'  url "{dependency_cask_url}"',
                '  name "mise handoff binary dependency"',
                '  desc "Deterministic formula dependency fixture"',
                '  homepage "https://mise.jdx.dev/"',
                f'  depends_on formula: "mise-test/handoff/{dependency_formula}"',
                f'  binary "{dependency_binary}"',
                "end",
                "",
            ]
        )
    )
    matrix.append(
        {
            "token": dependency_token,
            "class": "binary_formula_dependency",
            "auto_updates": False,
            "archive_sha256": dependency_cask_sha,
            "dependency_archive_sha256": dependency_sha,
            "required_rows": [
                "no_caskroom",
                "same_version_mise_caskroom",
                "different_target",
                "retry_after_failure",
                "homebrew_lifecycle",
            ],
        }
    )

    pkg_token = "mise-handoff-pkg-ineligible"
    pkg_name = "mise-handoff.pkg"
    pkg_archive = root / "archives" / f"{pkg_token}-1.0.0.zip"
    pkg_sha = write_zip(
        pkg_archive,
        {pkg_name: (b"not-a-real-package\n", stat.S_IFREG | 0o644)},
    )
    pkg_url = f"http://127.0.0.1:{port}/archives/{pkg_archive.name}"
    (casks_dir / f"{pkg_token}.rb").write_text(
        "\n".join(
            [
                f'cask "{pkg_token}" do',
                '  version "1.0.0"',
                f'  sha256 "{pkg_sha}"',
                f'  url "{pkg_url}"',
                '  name "mise handoff ineligible pkg"',
                '  desc "Fixture that must be rejected before package execution"',
                '  homepage "https://mise.jdx.dev/"',
                f'  pkg "{pkg_name}"',
                "end",
                "",
            ]
        )
    )
    matrix.append(
        {
            "token": pkg_token,
            "class": "pkg",
            "auto_updates": False,
            "archive_sha256": pkg_sha,
            "required_rows": ["pre_mutation_ineligible"],
        }
    )

    hook_token = "mise-handoff-hook-ineligible"
    hook_app = "Mise Handoff Hook.app"
    hook_archive = root / "archives" / f"{hook_token}-1.0.0.zip"
    hook_sha = write_zip(hook_archive, app_files(hook_app, "hook"))
    hook_url = f"http://127.0.0.1:{port}/archives/{hook_archive.name}"
    (casks_dir / f"{hook_token}.rb").write_text(
        "\n".join(
            [
                f'cask "{hook_token}" do',
                '  version "1.0.0"',
                f'  sha256 "{hook_sha}"',
                f'  url "{hook_url}"',
                '  name "mise handoff ineligible hook"',
                '  desc "Fixture that must be rejected before lifecycle hooks"',
                '  homepage "https://mise.jdx.dev/"',
                f'  app "{hook_app}"',
                "  preflight do",
                '    system "/usr/bin/touch", "/tmp/mise-handoff-hook-ran"',
                "  end",
                "end",
                "",
            ]
        )
    )
    matrix.append(
        {
            "token": hook_token,
            "class": "flight_hook",
            "auto_updates": False,
            "archive_sha256": hook_sha,
            "required_rows": ["pre_mutation_ineligible"],
        }
    )
    (root / "matrix.json").write_text(json.dumps(matrix, indent=2) + "\n")


if __name__ == "__main__":
    main()
