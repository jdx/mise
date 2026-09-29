"""Offline regression tests for the bootstrap installer's release-age policy."""

import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent.parent
SCRIPT = (ROOT / "packaging/standalone/install.envsubst").read_text().split(
    "\ninstall_mise\n", 1
)[0]


class ReleaseAgeTests(unittest.TestCase):
    def run_shell(self, command, **variables):
        env = {k: v for k, v in os.environ.items() if not k.startswith("MISE_")}
        env.update(variables)
        return subprocess.run(
            ["sh", "-c", SCRIPT + "\n" + command],
            env=env,
            text=True,
            capture_output=True,
            check=False,
        )

    def resolve(self, **variables):
        return self.run_shell(
            """
            date() { echo 1000000; }
            curl() { printf 'v2026.10.0 999999\nv2026.9.30 913600\nv2026.9.29 300000\n'; }
            resolve_release
            """,
            **variables,
        )

    def test_default_and_boundary(self):
        result = self.resolve()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.strip(), "2026.9.30")

    def test_global_setting(self):
        self.assertEqual(
            self.resolve(MISE_MINIMUM_RELEASE_AGE="7d").stdout.strip(), "2026.9.29"
        )

    def test_override_and_zero(self):
        result = self.resolve(
            MISE_MINIMUM_RELEASE_AGE="7d", MISE_SELF_UPDATE_MINIMUM_RELEASE_AGE="0s"
        )
        self.assertEqual(result.stdout.strip(), "2026.10.0")

    def test_pin_bypasses_policy_and_network(self):
        result = self.run_shell(
            "curl() { exit 77; }; resolve_release",
            MISE_VERSION="v2026.10.0",
            MISE_SELF_UPDATE_MINIMUM_RELEASE_AGE="invalid",
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.strip(), "2026.10.0")

    def test_invalid_age(self):
        for age in ["", "-1d", "garbage", "1mo", "1.5d"]:
            with self.subTest(age=age):
                self.assertNotEqual(
                    self.resolve(MISE_SELF_UPDATE_MINIMUM_RELEASE_AGE=age).returncode, 0
                )

    def test_units(self):
        for age in ["86400s", "1440m", "24h", "1d"]:
            with self.subTest(age=age):
                self.assertEqual(
                    self.resolve(MISE_SELF_UPDATE_MINIMUM_RELEASE_AGE=age).stdout.strip(),
                    "2026.9.30",
                )
        self.assertEqual(
            self.resolve(MISE_SELF_UPDATE_MINIMUM_RELEASE_AGE="1w").stdout.strip(),
            "2026.9.29",
        )

    def test_no_eligible_release(self):
        self.assertNotEqual(self.resolve(MISE_MINIMUM_RELEASE_AGE="90d").returncode, 0)

    def test_failed_fetch(self):
        result = self.run_shell("curl() { return 1; }; resolve_release")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("could not fetch", result.stderr)

    def test_invalid_index(self):
        for index in ["", "v2026.1.0", "latest 1", "v2026.1.0 1 extra", "v2026.1.0 1\nv2026.2.0 unknown"]:
            with self.subTest(index=index):
                result = self.run_shell('printf "%s\\n" "$INDEX" | select_release 1000', INDEX=index)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(result.stdout, "")

    def test_mise_order_not_publication_order(self):
        result = self.run_shell(
            "printf 'v2026.9.30 500\nv2026.10.0 200\nv2026.9.29 800\n' | select_release 1000"
        )
        self.assertEqual(result.stdout.strip(), "2026.10.0")

    def publish_index(self, releases):
        script = (ROOT / "scripts/publish-version.sh").read_text()
        jq_filter = script.split("jq -sr '", 1)[1].split("' >", 1)[0]
        return subprocess.run(
            ["jq", "-sr", jq_filter], input=releases, text=True,
            capture_output=True, check=False,
        )

    def test_publication_index_uses_published_time_and_all_pages(self):
        def release(tag, **kwargs):
            return dict(tag_name=tag, draft=False, prerelease=False,
                        created_at="2000-01-01T00:00:00Z",
                        published_at="2026-01-01T00:00:00Z", **kwargs)
        newest = release("v2026.10.0")
        draft = dict(release("v9999.1.0"), draft=True)
        prerelease = dict(release("v9999.2.0"), prerelease=True)
        result = self.publish_index(
            json.dumps([release("v2026.9.30"), draft, prerelease]) + "\n" +
            json.dumps([newest])
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, "v2026.10.0\t1767225600\nv2026.9.30\t1767225600\n")

    def test_publication_index_requires_publication_time(self):
        result = self.publish_index(json.dumps([dict(
            tag_name="v2026.1.0", draft=False, prerelease=False,
            created_at="2000-01-01T00:00:00Z", published_at=None,
        )]))
        self.assertNotEqual(result.returncode, 0)

    def test_existing_versions(self):
        for version, expected_status in [
            ("2026.10.0", 0),
            ("2026.10.0-DEBUG", 0),
            ("2026.9.29-DEBUG", 77),
            ("", 1),
        ]:
            with self.subTest(version=version), tempfile.TemporaryDirectory() as tmp:
                binary = Path(tmp) / "mise"
                binary.write_text(f"#!/bin/sh\necho {version}\n")
                binary.chmod(0o755)
                result = self.run_shell(
                    """
                    resolve_release() { echo 2026.9.30; }
                    download_file() { exit 77; }
                    install_mise
                    """,
                    MISE_INSTALL_PATH=str(binary),
                    MISE_INSTALL_OS="linux",
                    MISE_INSTALL_ARCH="x64",
                    MISE_INSTALL_EXT="tar.gz",
                    MISE_CURRENT_VERSION="v2026.10.0",
                )
                self.assertEqual(result.returncode, expected_status, result.stderr)
                if expected_status == 0:
                    self.assertIn("keeping installed version", result.stderr)

    def test_release_publication_promotes_installer_after_index(self):
        with tempfile.TemporaryDirectory() as tmp:
            log = Path(tmp) / "aws.log"
            env = os.environ.copy()
            env.update(CLOUDFLARE_ACCESS_KEY_ID="test", CLOUDFLARE_SECRET_ACCESS_KEY="test",
                       CLOUDFLARE_API_TOKEN="test", GITHUB_REPOSITORY="jdx/mise", TEST_AWS_LOG=str(log))
            result = subprocess.run(
                ["bash", "-c", r'''
                    aws() { echo "$*" >>"$TEST_AWS_LOG"; }
                    curl() { :; }
                    gh() { echo '[{"tag_name":"v2026.1.0","draft":false,"prerelease":false,"published_at":"2026-01-01T00:00:00Z"}]'; }
                    export -f aws curl gh
                    bash "$1" v2026.1.0
                ''', "test", str(ROOT / "scripts/publish-version.sh")],
                env=env, text=True, capture_output=True, check=False,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            calls = log.read_text().splitlines()
            index = next(i for i, call in enumerate(calls) if "s3://mise/releases.tsv" in call)
            installer = next(i for i, call in enumerate(calls) if "s3://mise/install.sh " in call)
            self.assertLess(index, installer)
            self.assertIn("s3://mise/v2026.1.0/install.sh", calls[installer])

    def test_paginated_retry_discards_partial_response_and_waits_for_reset(self):
        helper = ROOT / "scripts/gh-api-retry.sh"
        with tempfile.TemporaryDirectory() as tmp:
            result = subprocess.run(
                ["bash", "-c", r'''
                    set -euo pipefail
                    source "$1"
                    gh() {
                        if [[ $2 == rate_limit ]]; then echo "0 110"; return; fi
                        if [[ ! -f "$2" ]]; then
                            touch "$2"
                            echo "partial page that must be discarded"
                            return 1
                        fi
                        echo "complete pages"
                    }
                    date() { echo 100; }
                    sleep() { echo "sleep $1" >&2; }
                    gh_api "$2" --paginate
                ''', "test", str(helper), str(Path(tmp) / "attempt")],
                text=True, capture_output=True, check=False,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(result.stdout.strip(), "complete pages")
            self.assertIn("sleep 15", result.stderr)


if __name__ == "__main__":
    unittest.main()
