"""Offline regression tests for the bootstrap installer's release-age policy."""

import hashlib
import json
import os
from pathlib import Path
import re
import shutil
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
        for version, expected_status, downloads in [
            ("2026.10.0", 0, False),
            ("2026.10.0-DEBUG", 0, False),
            ("2026.9.29-DEBUG", 1, True),
            ("", 1, False),
        ]:
            with self.subTest(version=version), tempfile.TemporaryDirectory() as tmp:
                binary = Path(tmp) / "mise"
                binary.write_text(f"#!/bin/sh\necho {version}\n")
                binary.chmod(0o755)
                result = self.run_shell(
                    """
                    resolve_release() { echo 2026.9.30; }
                    download_file() { echo DOWNLOAD_ATTEMPTED >&2; return 1; }
                    install_mise
                    """,
                    MISE_INSTALL_PATH=str(binary),
                    MISE_INSTALL_OS="linux",
                    MISE_INSTALL_ARCH="x64",
                    MISE_INSTALL_EXT="tar.gz",
                    MISE_CURRENT_VERSION="v2026.10.0",
                )
                self.assertEqual(result.returncode, expected_status, result.stderr)
                self.assertEqual("DOWNLOAD_ATTEMPTED" in result.stderr, downloads, result.stderr)
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

    def publish_with_staged_keys(self, staged, missing=""):
        with tempfile.TemporaryDirectory() as tmp:
            log = Path(tmp) / "aws.log"
            env = os.environ.copy()
            env.update(CLOUDFLARE_ACCESS_KEY_ID="test", CLOUDFLARE_SECRET_ACCESS_KEY="test",
                       CLOUDFLARE_API_TOKEN="test", GITHUB_REPOSITORY="jdx/mise",
                       TEST_AWS_LOG=str(log), TEST_STAGED=staged, TEST_MISSING=missing)
            result = subprocess.run(
                ["bash", "-c", r'''
                    aws() { echo "$*" >>"$TEST_AWS_LOG"; case "$*" in *"head-object"*"--key $TEST_MISSING") [ -n "$TEST_MISSING" ] && return 254;; *list-objects-v2*) printf '%b\n' "$TEST_STAGED";; esac; }
                    curl() { :; }
                    gh() { echo '[{"tag_name":"v2026.1.0","draft":false,"prerelease":false,"published_at":"2026-01-01T00:00:00Z"}]'; }
                    export -f aws curl gh
                    bash "$1" v2026.1.0
                ''', "test", str(ROOT / "scripts/publish-version.sh")],
                env=env, text=True, capture_output=True, check=False,
            )
            return result, log.read_text()

    def test_release_without_powershell_installer_still_publishes(self):
        result, calls = self.publish_with_staged_keys("None")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertNotIn("install.ps1", calls.replace("--prefix v2026.1.0/install.ps1", ""))

    def test_release_with_powershell_installer_promotes_it(self):
        staged = "v2026.1.0/install.ps1\\tv2026.1.0/install.ps1.sig\\tv2026.1.0/install.ps1.minisig"
        result, calls = self.publish_with_staged_keys(staged.replace("\\t", "\n"))
        self.assertEqual(result.returncode, 0, result.stderr)
        for name in ("install.ps1", "install.ps1.sig", "install.ps1.minisig"):
            self.assertIn(f"head-object --bucket mise --key v2026.1.0/{name}", calls)
            self.assertIn(f"cp s3://mise/v2026.1.0/{name} s3://mise/{name} ", calls)

    def test_staged_powershell_installer_requires_both_signatures(self):
        staged = "v2026.1.0/install.ps1\nv2026.1.0/install.ps1.sig\nv2026.1.0/install.ps1.minisig"
        for name in ("install.ps1.sig", "install.ps1.minisig"):
            result, calls = self.publish_with_staged_keys(staged, f"v2026.1.0/{name}")
            self.assertNotEqual(result.returncode, 0)
            self.assertNotIn("s3 cp", calls)

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


MIRROR = "https://mise.jdx.dev"
GITHUB = "https://github.com/jdx/mise/releases/download"


class DownloadSourceTests(unittest.TestCase):
    """Runs install_mise offline against a fake curl that serves a real tarball.

    mise.jdx.dev URLs serve MIRROR_TARBALL and MIRROR_SUM, which differ from the
    genuine GitHub ones when a test simulates a compromised mirror.
    """

    CURL_STUB = r"""
        curl() {
            eval "u=\${$#}"
            echo "$u" >>"$CURL_LOG"
            # every archive and checksum download refuses redirects off https
            [ "$1 $2" = "--proto-redir =https" ] || return 99
            shift 2
            case " $* " in *" --netrc-optional "*) echo "$u" >>"$NETRC_LOG" ;; esac
            case " $FAIL_URLS " in *" $u "*) return 22 ;; esac
            case " $OTHER_ASSET_SUM_URLS " in *" $u "*)
                printf '%064d  ./mise-v0-macos-arm64.tar.gz\n' 0
                return ;;
            esac
            stub_tarball="$TARBALL" stub_sum="$SUM"
            case "$u" in https://mise.jdx.dev/*)
                stub_tarball="$MIRROR_TARBALL" stub_sum="$MIRROR_SUM" ;;
            esac
            case "$1" in
                -\#fLo) cp "$stub_tarball" "$2" ;;
                *) printf '%s  ./%s\n' "$stub_sum" "$(basename "$TARBALL")" ;;
            esac
        }
        install_mise
    """

    @staticmethod
    def make_tarball(root, asset, output):
        binary = root / "mise/bin/mise"
        binary.parent.mkdir(parents=True)
        binary.write_text(f"#!/bin/sh\necho {output}\n")
        binary.chmod(0o755)
        tarball = root / asset
        subprocess.run(["tar", "-czf", str(tarball), "-C", str(root), "mise"], check=True)
        return tarball, hashlib.sha256(tarball.read_bytes()).hexdigest()

    def install(self, version="2026.1.0", fail_urls=(), other_asset_sum_urls=(),
                tampered_mirror=False, **variables):
        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            asset = f"mise-v{version}-linux-x64.tar.gz"
            tarball, digest = self.make_tarball(tmp / "genuine", asset, version)
            mirror_tarball, mirror_digest = tarball, digest
            if tampered_mirror:
                # a self-consistent archive and checksum served by mise.jdx.dev
                mirror_tarball, mirror_digest = self.make_tarball(tmp / "tampered", asset, "evil")
            log = tmp / "curl.log"
            log.touch()
            netrc_log = tmp / "netrc.log"
            netrc_log.touch()
            env = {k: v for k, v in os.environ.items() if not k.startswith("MISE_")}
            env.update(
                CURL_LOG=str(log), NETRC_LOG=str(netrc_log), TARBALL=str(tarball), SUM=digest,
                MIRROR_TARBALL=str(mirror_tarball), MIRROR_SUM=mirror_digest,
                FAIL_URLS=" ".join(u.format(v=version, a=asset) for u in fail_urls),
                # these serve a SHASUMS256.txt that only lists an unrelated asset
                OTHER_ASSET_SUM_URLS=" ".join(
                    u.format(v=version, a=asset) for u in other_asset_sum_urls
                ),
                MISE_INSTALL_PATH=str(tmp / "bin/mise"), MISE_INSTALL_OS="linux",
                MISE_INSTALL_ARCH="x64", MISE_INSTALL_EXT="tar.gz",
                MISE_CURRENT_VERSION="v2026.10.0", MISE_VERSION=version, MISE_INSTALL_HELP="0",
            )
            # static checksums baked into the rendered script for the built-in version
            for name in re.findall(r"\$(MISE_CHECKSUM_\w+)", SCRIPT):
                env[name] = f"{digest}  ./{asset}"
            env.update(variables)
            result = subprocess.run(
                ["sh", "-c", SCRIPT + "\n" + self.CURL_STUB],
                env=env, text=True, capture_output=True, check=False,
            )
            installed = (tmp / "bin/mise").exists()
            # the URLs curl fetched with --netrc-optional
            self.netrc_urls = netrc_log.read_text().splitlines()
            return result, log.read_text().splitlines(), installed

    def assert_installed(self, result, installed):
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(installed, result.stderr)

    def test_pinned_version_uses_mirror_verified_against_github(self):
        result, urls, installed = self.install()
        self.assert_installed(result, installed)
        self.assertEqual(urls, [
            f"{MIRROR}/v2026.1.0/mise-v2026.1.0-linux-x64.tar.gz",
            f"{GITHUB}/v2026.1.0/SHASUMS256.txt",
        ])

    def test_pinned_version_falls_back_to_github(self):
        result, urls, installed = self.install(fail_urls=[MIRROR + "/v{v}/{a}"])
        self.assert_installed(result, installed)
        self.assertIn("trying GitHub Releases", result.stderr)
        self.assertEqual(urls, [
            f"{MIRROR}/v2026.1.0/mise-v2026.1.0-linux-x64.tar.gz",
            f"{GITHUB}/v2026.1.0/mise-v2026.1.0-linux-x64.tar.gz",
            f"{GITHUB}/v2026.1.0/SHASUMS256.txt",
        ])

    def assert_refused(self, result, installed, message):
        self.assertNotEqual(result.returncode, 0, result.stderr)
        self.assertFalse(installed, result.stderr)
        self.assertIn(message, result.stderr)

    def test_tampered_mirror_archive_fails(self):
        # mise.jdx.dev's own SHASUMS256.txt would match its archive, but only
        # GitHub's is consulted
        result, urls, installed = self.install(tampered_mirror=True)
        self.assert_refused(result, installed, "checksum mismatch")
        self.assertNotIn(f"{MIRROR}/v2026.1.0/SHASUMS256.txt", urls)

        # the built-in version is checked against the checksums embedded in the script
        result, urls, installed = self.install(version="2026.10.0", tampered_mirror=True)
        self.assert_refused(result, installed, "checksum mismatch")
        self.assertEqual(urls, [f"{MIRROR}/v2026.10.0/mise-v2026.10.0-linux-x64.tar.gz"])

    def test_no_github_checksum_fails_without_mirror_fallback(self):
        github_sums = GITHUB + "/v{v}/SHASUMS256.txt"
        for case, kwargs in [
            ("download fails", dict(fail_urls=[github_sums])),
            ("asset not listed", dict(other_asset_sum_urls=[github_sums])),
        ]:
            with self.subTest(case=case):
                result, urls, installed = self.install(**kwargs)
                self.assert_refused(result, installed, "no checksum found")
                self.assertEqual(urls, [
                    f"{MIRROR}/v2026.1.0/mise-v2026.1.0-linux-x64.tar.gz",
                    f"{GITHUB}/v2026.1.0/SHASUMS256.txt",
                ])

    def test_builtin_version_without_static_checksum_fails(self):
        empty = {name: "" for name in re.findall(r"\$(MISE_CHECKSUM_\w+)", SCRIPT)}
        result, urls, installed = self.install(version="2026.10.0", **empty)
        self.assert_refused(result, installed, "no checksum found")

    def test_checksum_mismatch_fails(self):
        result, urls, installed = self.install(SUM="0" * 64)
        self.assert_refused(result, installed, "checksum mismatch")

    def test_both_hosts_fail(self):
        result, urls, installed = self.install(
            fail_urls=[MIRROR + "/v{v}/{a}", GITHUB + "/v{v}/{a}"]
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(installed)
        self.assertIn(f"failed to download {GITHUB}/v2026.1.0/", result.stderr)

    def test_install_from_github(self):
        for value in ["1", "true"]:
            with self.subTest(value=value):
                result, urls, installed = self.install(MISE_INSTALL_FROM_GITHUB=value)
                self.assert_installed(result, installed)
                self.assertEqual(urls, [
                    f"{GITHUB}/v2026.1.0/mise-v2026.1.0-linux-x64.tar.gz",
                    f"{GITHUB}/v2026.1.0/SHASUMS256.txt",
                ])

    def test_github_mirror_replaces_github(self):
        gh_mirror = "https://gh.example.invalid/github/jdx/mise/releases/download"
        for value in ["https://gh.example.invalid/github/", "gh.example.invalid/github"]:
            with self.subTest(value=value):
                result, urls, installed = self.install(
                    fail_urls=[MIRROR + "/v{v}/{a}"], MISE_INSTALL_GITHUB_MIRROR=value
                )
                self.assert_installed(result, installed)
                self.assertEqual(urls, [
                    f"{MIRROR}/v2026.1.0/mise-v2026.1.0-linux-x64.tar.gz",
                    f"{gh_mirror}/v2026.1.0/mise-v2026.1.0-linux-x64.tar.gz",
                    f"{gh_mirror}/v2026.1.0/SHASUMS256.txt",
                ])

        result, urls, installed = self.install(
            MISE_INSTALL_FROM_GITHUB="1", MISE_INSTALL_GITHUB_MIRROR="gh.example.invalid/github"
        )
        self.assert_installed(result, installed)
        self.assertEqual(urls, [
            f"{gh_mirror}/v2026.1.0/mise-v2026.1.0-linux-x64.tar.gz",
            f"{gh_mirror}/v2026.1.0/SHASUMS256.txt",
        ])

    def test_only_mirror_requests_read_netrc(self):
        gh_mirror = "https://gh.example.invalid/github/jdx/mise/releases/download"
        result, urls, installed = self.install(
            fail_urls=[MIRROR + "/v{v}/{a}"], MISE_INSTALL_GITHUB_MIRROR="gh.example.invalid/github"
        )
        self.assert_installed(result, installed)
        self.assertEqual(self.netrc_urls, [
            f"{gh_mirror}/v2026.1.0/mise-v2026.1.0-linux-x64.tar.gz",
            f"{gh_mirror}/v2026.1.0/SHASUMS256.txt",
        ])

        # neither mise.jdx.dev nor github.com gets netrc credentials
        result, urls, installed = self.install(fail_urls=[MIRROR + "/v{v}/{a}"])
        self.assert_installed(result, installed)
        self.assertEqual(len(urls), 3)
        self.assertEqual(self.netrc_urls, [])

    def test_github_mirror_requires_https(self):
        for value in ["http://gh.example.invalid", "ftp://gh.example.invalid"]:
            with self.subTest(value=value):
                result, urls, installed = self.install(MISE_INSTALL_GITHUB_MIRROR=value)
                self.assert_refused(result, installed, "MISE_INSTALL_GITHUB_MIRROR must use https")
                self.assertEqual(urls, [])

    def test_github_mirror_rejects_credentials(self):
        for value in [
            "https://user:s3cret@gh.example.invalid/github",
            "user:s3cret@gh.example.invalid",
            "http://user:s3cret@gh.example.invalid",
        ]:
            with self.subTest(value=value):
                result, urls, installed = self.install(MISE_INSTALL_GITHUB_MIRROR=value)
                self.assert_refused(result, installed, "MISE_INSTALL_GITHUB_MIRROR must not contain credentials")
                self.assertNotIn("s3cret", result.stderr)
                self.assertEqual(urls, [])

        result, urls, installed = self.install(
            fail_urls=[MIRROR + "/v{v}/{a}"], MISE_INSTALL_GITHUB_MIRROR="https://gh.example.invalid/a@b"
        )
        self.assert_installed(result, installed)

    def test_builtin_version_uses_static_checksum_and_falls_back(self):
        result, urls, installed = self.install(version="2026.10.0")
        self.assert_installed(result, installed)
        self.assertEqual(urls, [f"{MIRROR}/v2026.10.0/mise-v2026.10.0-linux-x64.tar.gz"])

        result, urls, installed = self.install(version="2026.10.0", fail_urls=[MIRROR + "/v{v}/{a}"])
        self.assert_installed(result, installed)
        self.assertEqual(urls, [
            f"{MIRROR}/v2026.10.0/mise-v2026.10.0-linux-x64.tar.gz",
            f"{GITHUB}/v2026.10.0/mise-v2026.10.0-linux-x64.tar.gz",
        ])

    def test_tarball_url_override_has_no_fallback(self):
        custom = "https://example.invalid/mise-v2026.10.0-linux-x64.tar.gz"
        result, urls, installed = self.install(version="2026.10.0", MISE_TARBALL_URL=custom)
        self.assert_installed(result, installed)
        self.assertEqual(urls, [custom])

        result, urls, installed = self.install(
            version="2026.10.0", MISE_TARBALL_URL=custom, fail_urls=[custom]
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(urls, [custom])

        # the override only applies to the built-in version, as before
        result, urls, installed = self.install(MISE_TARBALL_URL=custom)
        self.assert_installed(result, installed)
        self.assertEqual(urls[0], f"{MIRROR}/v2026.1.0/mise-v2026.1.0-linux-x64.tar.gz")


class WgetRedirectTests(unittest.TestCase):
    """download_file and fetch_text through a fake wget, which cannot refuse a
    redirect off https itself, so the installer checks its -S headers."""

    WGET = r"""#!/bin/sh
if [ "$1" = --version ]; then
    # BusyBox wget has no --version
    [ -n "$WGET_VERSION" ] || { echo "wget: unrecognized option '--version'" >&2; exit 1; }
    echo "$WGET_VERSION"
    exit 0
fi
case "$1" in
    -SO | -qSO) out=$2 ;;
    *) echo "wget called without -S: $*" >&2; exit 99 ;;
esac
printf '  HTTP/1.1 302 Found\n  %s\n  HTTP/1.1 200 OK\n' "$WGET_HEADER" >&2
if [ "$out" = - ]; then echo body; else echo body >"$out"; fi
"""

    def run_wget(self, header, command, netrc=False, **variables):
        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            bin_dir = tmp / "bin"
            bin_dir.mkdir()
            # no curl on PATH, so the installer uses wget
            for tool in ["awk", "basename", "cat", "dirname", "grep", "mktemp", "rm"]:
                (bin_dir / tool).symlink_to(shutil.which(tool))
            wget = bin_dir / "wget"
            wget.write_text(self.WGET)
            wget.chmod(0o755)
            if netrc:
                (tmp / ".netrc").write_text("machine gh.example.invalid login user password s3cret\n")
            result = subprocess.run(
                [shutil.which("sh"), "-c", SCRIPT + "\n" + command],
                env={"PATH": str(bin_dir), "TMPDIR": str(tmp), "HOME": str(tmp), "DIR": str(tmp),
                     "WGET_HEADER": header, **variables},
                text=True, capture_output=True, check=False,
            )
            downloaded = (tmp / "mise.tar.gz").exists()
            leftovers = sorted(p.name for p in tmp.iterdir() if p.name not in ("bin", "mise.tar.gz", ".netrc"))
            return result, downloaded, leftovers

    def test_wget_without_netrc_support_warns_for_mirror(self):
        command = "resolve_release() { echo 2026.1.0; }; download_file() { return 1; }; install_mise"
        variables = dict(MISE_VERSION="2026.1.0", MISE_CURRENT_VERSION="v2026.10.0", MISE_INSTALL_OS="linux",
                         MISE_INSTALL_ARCH="x64", MISE_INSTALL_EXT="tar.gz")
        for case, wget_version, mirror, netrc, warns in [
            ("busybox wget", "", "gh.example.invalid", True, True),
            ("gnu wget", "GNU Wget 1.21.4 built on linux-gnu.", "gh.example.invalid", True, False),
            ("gnu wget2", "GNU Wget2 2.1.0 - multithreaded metalink/file/website downloader", "gh.example.invalid", True, False),
            ("no netrc", "", "gh.example.invalid", False, False),
            ("no mirror", "", "", True, False),
        ]:
            with self.subTest(case=case):
                result, _, _ = self.run_wget(
                    "", command, netrc=netrc, WGET_VERSION=wget_version, MISE_INSTALL_GITHUB_MIRROR=mirror, **variables
                )
                self.assertIn("failed to download", result.stderr)
                self.assertEqual("this wget cannot read ~/.netrc" in result.stderr, warns, result.stderr)

    def download(self, header):
        return self.run_wget(header, 'download_file https://gh.example.invalid/mise.tar.gz "$DIR"')

    def fetch(self, header):
        result, _, leftovers = self.run_wget(header, "fetch_text https://gh.example.invalid/SHASUMS256.txt")
        return result, leftovers

    def test_redirect_off_https_is_refused(self):
        for header in [
            "Location: http://evil.invalid/mise.tar.gz",
            "location:HTTP://evil.invalid/mise.tar.gz",
            "Location: ftp://evil.invalid/mise.tar.gz",
            # BusyBox wget fetches a Location without a scheme over http
            "Location: evil.invalid/mise.tar.gz",
        ]:
            with self.subTest(header=header):
                result, downloaded, leftovers = self.download(header)
                self.assertNotEqual(result.returncode, 0, result.stderr)
                self.assertFalse(downloaded)
                self.assertEqual(leftovers, [])
                self.assertIn("refusing redirect to non-HTTPS URL", result.stderr)

                result, leftovers = self.fetch(header)
                self.assertNotEqual(result.returncode, 0, result.stderr)
                self.assertEqual(result.stdout, "")
                self.assertEqual(leftovers, [])
                self.assertIn("refusing redirect to non-HTTPS URL", result.stderr)

    def test_redirect_within_https_is_followed(self):
        for header in [
            "Location: https://objects.example.invalid/mise.tar.gz",
            "Location: HTTPS://objects.example.invalid/mise.tar.gz",
            "Location: /mise.tar.gz",
            "Content-Type: application/octet-stream",
        ]:
            with self.subTest(header=header):
                result, downloaded, leftovers = self.download(header)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertTrue(downloaded)
                self.assertEqual(leftovers, [])
                self.assertTrue(result.stdout.strip().endswith("/mise.tar.gz"), result.stdout)

                result, leftovers = self.fetch(header)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(result.stdout, "body\n")
                self.assertEqual(leftovers, [])


PWSH = os.environ.get("PWSH") or shutil.which("pwsh")


@unittest.skipUnless(PWSH, "pwsh is not installed (set PWSH to its path)")
class PowerShellInstallerTests(unittest.TestCase):
    """Loads the functions of install.ps1, which only runs whole on Windows,
    and calls them with Invoke-WebRequest replaced by a stub."""

    PRELUDE = r"""
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 3.0
$quiet = $true
$debugOut = $false
$errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile($env:INSTALL_PS1, [ref]$null, [ref]$errors)
if ($errors) { throw "install.ps1 does not parse: $($errors -join '; ')" }
$isFunction = { param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] }
foreach ($f in $ast.FindAll($isFunction, $true)) { . ([scriptblock]::Create($f.Extent.Text)) }
function Invoke-WebRequest([switch] $UseBasicParsing, $Uri, $Headers, $OutFile) {
  [pscustomobject]@{ Content = (@{ Uri = $Uri; Authorization = if ($Headers) { $Headers.Authorization } else { $null } } | ConvertTo-Json -Compress) }
}
"""

    def pwsh(self, command, netrc=None, windows_netrc=None, **variables):
        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            if netrc is not None:
                (tmp / ".netrc").write_text(netrc)
            if windows_netrc is not None:
                (tmp / "_netrc").write_text(windows_netrc)
            script = tmp / "test.ps1"
            script.write_text(self.PRELUDE + command)
            env = {k: v for k, v in os.environ.items() if not k.startswith("MISE_")}
            env.update(HOME=str(tmp), INSTALL_PS1=str(ROOT / "packaging/standalone/install.ps1"), **variables)
            return subprocess.run(
                [PWSH, "-NoProfile", "-NonInteractive", "-File", str(script)],
                env=env, text=True, capture_output=True, check=False,
            )

    def credential(self, netrc=None, windows_netrc=None, host="gh.example.invalid"):
        result = self.pwsh(
            f"$c = Get-NetrcCredential '{host}'; if ($c) {{ \"$($c.Login)|$($c.Password)\" }} else {{ 'none' }}",
            netrc=netrc, windows_netrc=windows_netrc,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        return result.stdout.strip()

    def test_netrc_entry_for_the_host(self):
        for case, netrc, expected in [
            ("one line", "machine gh.example.invalid login user password s3cret\n", "user|s3cret"),
            ("multi line, other host first",
             "machine other.invalid\n  login a\n  password b\nmachine GH.example.invalid\n  login user\n  password s3cret\n",
             "user|s3cret"),
            ("first match wins",
             "machine gh.example.invalid login first password one\nmachine gh.example.invalid login second password two\n",
             "first|one"),
            ("default after other machines",
             "machine other.invalid login a password b\ndefault login anon password p\n", "anon|p"),
            ("quoted password", 'machine gh.example.invalid login user password "s3 cr\\"et"\n', 'user|s3 cr"et'),
            ("login without password", "machine gh.example.invalid login token\n", "token|"),
            ("comment and account",
             "# machine gh.example.invalid login evil password evil\nmachine gh.example.invalid account x login user password s3cret\n",
             "user|s3cret"),
            ("macdef body is skipped",
             "macdef init\nmachine gh.example.invalid login evil password evil\n\nmachine gh.example.invalid login user password s3cret\n",
             "user|s3cret"),
            ("no entry", "machine other.invalid login a password b\n", "none"),
        ]:
            with self.subTest(case=case):
                self.assertEqual(self.credential(netrc=netrc), expected)

    def test_netrc_file_lookup(self):
        self.assertEqual(self.credential(), "none")
        # _netrc, the Windows name mise reads first, wins over .netrc
        self.assertEqual(
            self.credential(
                netrc="machine gh.example.invalid login dot password one\n",
                windows_netrc="machine gh.example.invalid login underscore password two\n",
            ),
            "underscore|two",
        )

    def download(self, url, mirror="gh.example.invalid/github", netrc="machine gh.example.invalid login user password s3cret\n"):
        variables = {"MISE_INSTALL_GITHUB_MIRROR": mirror} if mirror else {}
        result = self.pwsh(f"(Invoke-Download '{url}').Content", netrc=netrc, **variables)
        self.assertEqual(result.returncode, 0, result.stderr)
        return json.loads(result.stdout)

    def test_only_mirror_requests_send_netrc_credentials(self):
        basic = "Basic dXNlcjpzM2NyZXQ="  # user:s3cret
        mirror_url = "https://gh.example.invalid/github/jdx/mise/releases/download/v2026.1.0/SHASUMS256.txt"
        self.assertEqual(self.download(mirror_url), {"Uri": mirror_url, "Authorization": basic})
        self.assertEqual(self.download(mirror_url.replace("gh.example", "GH.example"))["Authorization"], basic)
        for case, url, mirror in [
            ("mise.jdx.dev", "https://mise.jdx.dev/v2026.1.0/mise-v2026.1.0-windows-x64.zip", "gh.example.invalid/github"),
            ("same host, outside the mirror path", "https://gh.example.invalid/other/x", "gh.example.invalid/github"),
            ("no mirror", "https://github.com/jdx/mise/releases/download/v2026.1.0/SHASUMS256.txt", None),
        ]:
            with self.subTest(case=case):
                self.assertIsNone(self.download(url, mirror=mirror, netrc="default login user password s3cret\n")["Authorization"])
        self.assertIsNone(self.download(mirror_url, netrc="machine other.invalid login a password b\n")["Authorization"])

    def test_github_base(self):
        for mirror, expected in [
            (None, "https://github.com"),
            ("gh.example.invalid/github/", "https://gh.example.invalid/github"),
            ("https://gh.example.invalid", "https://gh.example.invalid"),
            ("http://gh.example.invalid", "mise: MISE_INSTALL_GITHUB_MIRROR must use https: http://gh.example.invalid"),
        ]:
            with self.subTest(mirror=mirror):
                variables = {"MISE_INSTALL_GITHUB_MIRROR": mirror} if mirror else {}
                result = self.pwsh("try { Get-GitHubBase } catch { $_.Exception.Message }", **variables)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(result.stdout.strip(), expected)


if __name__ == "__main__":
    unittest.main()
