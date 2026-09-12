import errno
import json
import os
from pathlib import Path
import platform
import pty
import shlex
import shutil
import stat
import subprocess
import threading
import unittest


class PackagesWhere(unittest.TestCase):
    """Exercise local package lookup and its startup isolation through the public CLI."""

    @classmethod
    def setUpClass(cls):
        """Require the harness binary and identify hosts supported by local Homebrew lookup."""
        cls.binary = shutil.which("mise")
        assert cls.binary is not None, "the e2e harness must provide mise"
        cls.supported = (platform.system(), platform.machine()) in {
            ("Darwin", "arm64"),
            ("Linux", "x86_64"),
            ("Linux", "aarch64"),
        }

    def setUp(self):
        """Seed isolated state that exposes unexpected configuration, network, or housekeeping work."""
        self.base = Path.cwd() / self.id().rsplit(".", 1)[-1]
        self.base.mkdir()
        self.prefix = self.base / "prefix with spaces"
        self.project = self.base / "project"
        self.project.mkdir()
        self.env = {
            key: value
            for key, value in os.environ.items()
            if not key.startswith(("MISE_", "GITHUB_", "GH_", "CI", "GIT_"))
        }
        self.env.update({
            "HOME": str(self.base / "home"),
            "MISE_CONFIG_DIR": str(self.base / "config"),
            "MISE_GLOBAL_CONFIG_FILE": str(self.base / "config/config.toml"),
            "MISE_DATA_DIR": str(self.base / "data"),
            "MISE_CACHE_DIR": str(self.base / "cache"),
            "MISE_STATE_DIR": str(self.base / "state"),
            "MISE_SYSTEM_BREW_PREFIX": str(self.prefix),
            "MISE_AUTO_UPDATE": "1",
            "MISE_AUTO_UPDATE_CHECK_DURATION": "0s",
            "MISE_CACHE_PRUNE_AGE": "0s",
            "MISE_CI": "0",
            "MISE_OFFLINE": "0",
            "MISE_PREFER_OFFLINE": "0",
            "MISE_COLOR": "0",
            "MISE_FRIENDLY_ERROR": "1",
            "MISE_TRUSTED_CONFIG_PATHS": str(self.base),
            "HTTP_PROXY": "http://127.0.0.1:1",
            "HTTPS_PROXY": "http://127.0.0.1:1",
            "ALL_PROXY": "http://127.0.0.1:1",
            "NO_PROXY": "",
        })
        for directory in ["home", "config", "data", "cache", "state", "sentinel-bin"]:
            (self.base / directory).mkdir()
        self.write("sentinel-bin/brew", "#!/bin/sh\ntouch \"$HOME/brew-ran\"\nexit 91\n", executable=True)
        self.env["PATH"] = str(self.base / "sentinel-bin") + os.pathsep + self.env["PATH"]
        self.write("data/plugins/gradle/.git/HEAD", "ref: refs/heads/main\n")
        self.write("data/plugins/gradle/.git/config", '[core]\nrepositoryformatversion = 0\n[remote "origin"]\nurl = https://github.com/rfrancis/asdf-gradle.git\n')
        (self.base / "data/plugins/gradle/.git/objects").mkdir()
        (self.base / "data/plugins/gradle/.git/refs").mkdir()
        self.write("cache/stale/content", "preserve stale cache\n")
        self.write("cache/auto-update-last-check", "preserve update timestamp\n")
        self.write("cache/.auto_prune", "preserve prune timestamp\n")
        for path in (self.base / "cache").rglob("*"):
            os.utime(path, (1, 1))
        retired = self.base / "data/installs/dummy/retired"
        self.write("data/installs/dummy/retired/payload", "preserve deferred tool\n")
        self.write("state/tool-purgatory.json", json.dumps({
            "schema_version": 1,
            "entries": {"fixture": {"install_path": str(retired), "display": "dummy@retired", "remove_after": 1}},
        }))
        self.seed_config_sentinels()

    def write(self, relative, content, executable=False):
        """Create a fixture file under the isolated root, optionally as a sentinel executable."""
        path = self.base / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content)
        if executable:
            path.chmod(0o755)
        return path

    def seed_config_sentinels(self):
        """Expose configuration loading through invalid syntax and commands that create marker files."""
        for scope, path in [("global", "config/config.toml"), ("project", "project/mise.toml")]:
            marker = self.base / "home" / (scope + "-settings-ran")
            expression = '{{ exec(command="touch ' + str(marker) + '") }}'
            self.write(path, "[settings.age]\nkey_file = '" + expression + "'\n")
        marker = self.base / "home/config-ran"
        config = self.base / "project/mise.toml"
        config.write_text(config.read_text() + "\n[env]\nQUERY_SENTINEL = '{{ exec(command=\"touch " + str(marker) + "\") }}'\n")
        self.write("project/.miserc.toml", "invalid-miserc-sentinel = [\n")

    def keg(self, name="widget", version="active", target=None):
        """Create a formula rack and active opt link without Homebrew or installation receipts."""
        keg = self.prefix / "Cellar" / name / version
        keg.mkdir(parents=True)
        opt = self.prefix / "opt" / name
        opt.parent.mkdir(parents=True, exist_ok=True)
        opt.symlink_to(keg if target is None else target)
        return keg, opt

    def snapshot(self):
        """Capture file kinds, modification times, contents, and raw links to detect query mutations."""
        result = {}
        for path in self.base.rglob("*"):
            metadata = path.lstat()
            content = os.readlink(path) if path.is_symlink() else path.read_bytes() if path.is_file() else None
            result[str(path.relative_to(self.base))] = (stat.S_IFMT(metadata.st_mode), metadata.st_mtime_ns, content)
        return result

    def run_mise(self, args, extra_env=None):
        """Separate stdout from terminal diagnostics and require the query to preserve fixture state."""
        before = self.snapshot()
        env = self.env | (extra_env or {})
        master, slave = pty.openpty()
        diagnostics = bytearray()
        reader_errors = []

        def read_diagnostics():
            """Drain stderr concurrently, treating terminal EIO as closure and retaining other errors."""
            while True:
                try:
                    data = os.read(master, 65536)
                except OSError as error:
                    if error.errno == errno.EIO:
                        return
                    reader_errors.append(error)
                    return
                if not data:
                    return
                diagnostics.extend(data)

        reader = threading.Thread(target=read_diagnostics)
        reader.start()
        try:
            result = subprocess.run([self.binary, *args], cwd=self.project, env=env, stdout=subprocess.PIPE, stderr=slave, timeout=20)
        finally:
            os.close(slave)
            reader.join(timeout=5)
            os.close(master)
        self.assertFalse(reader.is_alive(), "stderr reader must finish")
        if reader_errors:
            raise reader_errors[0]
        self.assertEqual(self.snapshot(), before, "lookup must preserve prefix, config, and housekeeping state")
        return result, diagnostics.decode("utf-8", errors="replace")

    def success(self, args=None, extra_env=None, expected=None):
        """Require one exact opt-path line and return the path for subsequent executable checks."""
        if args is None:
            args = ["bootstrap", "packages", "where", "brew:widget"]
        result, stderr = self.run_mise(args, extra_env)
        self.assertEqual(result.returncode, 0, stderr)
        root = self.prefix / "opt/widget" if expected is None else expected
        self.assertEqual(result.stdout, os.fsencode(root) + b"\n")
        return root

    def failure(self, args, diagnostic, extra_env=None):
        """Require empty stdout, failure status, and the query diagnostic instead of configuration errors."""
        result, stderr = self.run_mise(args, extra_env)
        self.assertNotEqual(result.returncode, 0, stderr)
        self.assertEqual(result.stdout, b"", stderr)
        self.assertIn(diagnostic.lower(), stderr.lower())
        self.assertNotIn("invalid-miserc-sentinel", stderr)
        self.assertNotIn("failed to render settings", stderr)

    def require_supported(self):
        """Run filesystem lookup cases only on hosts supported by the built-in Homebrew manager."""
        if not self.supported:
            self.skipTest("local brew lookup requires macOS arm64 or Linux x86_64/arm64")

    def test_keg_only_root_executes_fixture_without_brew(self):
        """Demonstrate that the returned root makes a keg-only executable usable without Homebrew."""
        self.require_supported()
        keg, _ = self.keg(target="../Cellar/widget/active")
        binary = keg / "bin/widget-fixture"
        binary.parent.mkdir()
        binary.write_text("#!/bin/sh\nprintf 'keg-only-ok\\n'\n")
        binary.chmod(0o755)
        root = self.success()
        result = subprocess.run(["widget-fixture"], env=self.env | {"PATH": str(root / "bin")}, capture_output=True, check=True)
        self.assertEqual(result.stdout, b"keg-only-ok\n")

    def test_names_are_literal_and_qualified_names_share_the_local_rack(self):
        """Exercise literal version suffixes and qualified formula names through the public CLI."""
        self.require_supported()
        for name in ["widget", "openssl@3", "widget@latest", "widget@1.2"]:
            _, opt = self.keg(name)
            for request in [name, "homebrew/core/" + name, "owner/tap/" + name]:
                with self.subTest(request=request):
                    self.success(["bootstrap", "packages", "where", "brew:" + request], expected=opt)

    def test_active_opt_selection_and_retargeting_keep_stable_output(self):
        """Keep output stable across active-keg changes without selecting versions by their spelling."""
        self.require_supported()
        _, opt = self.keg(version="old-channel")
        new = self.prefix / "Cellar/widget/2099.12.31"
        new.mkdir()
        self.success()
        opt.unlink()
        opt.symlink_to(new)
        self.success()

    def test_global_flag_positions_and_parent_options(self):
        """Keep query isolation intact across inherited flags and command-like flag values."""
        self.require_supported()
        self.keg()
        for args in [
            ["--quiet", "--cd", str(self.project), "bootstrap", "packages", "where", "brew:widget"],
            ["bootstrap", "--yes", "--only", "packages", "packages", "where", "brew:widget"],
            ["bootstrap", "packages", "where", "--quiet", "--cd", str(self.project), "brew:widget"],
            ["bootstrap", "packages", "where", "brew:widget", "--quiet", "--cd=" + str(self.project)],
            ["-C" + str(self.project), "bootstrap", "packages", "where", "--", "brew:widget"],
            ["--env", "bootstrap", "bootstrap", "packages", "where", "brew:widget"],
        ]:
            with self.subTest(args=args):
                self.success(args)

    def test_source_flags_conflict_with_query(self):
        """Reject repository source options before a local lookup can ignore their requested behavior."""
        for flag in ["--from", "--adopt", "--from-git"]:
            with self.subTest(flag=flag):
                self.failure(
                    ["bootstrap", flag, "packages", "packages", "where", "brew:widget"],
                    "cannot be used with a bootstrap subcommand",
                )

    def test_cd_applies_before_resolving_relative_prefix(self):
        """Resolve relative Homebrew prefixes from the directory selected by the CLI."""
        self.require_supported()
        self.keg()
        self.success(["--cd", str(self.base), "bootstrap", "packages", "where", "brew:widget"], {"MISE_SYSTEM_BREW_PREFIX": "prefix with spaces"})

    def test_symlinked_prefix_preserves_its_spelling(self):
        """Expose the configured prefix alias while accepting its valid canonical installation."""
        self.require_supported()
        self.keg()
        alias = self.base / "prefix alias"
        alias.symlink_to(self.prefix)
        self.success(extra_env={"MISE_SYSTEM_BREW_PREFIX": str(alias)}, expected=alias / "opt/widget")

    def test_missing_and_invalid_opt_records_fail_without_stdout(self):
        """Exercise the script-facing failure contract for missing records and invalid opt targets."""
        self.require_supported()
        self.failure(["bootstrap", "packages", "where", "brew:widget"], "apply brew:widget")
        keg, opt = self.keg()
        opt.unlink()
        self.failure(["bootstrap", "packages", "where", "brew:widget"], "apply brew:widget")
        linked = self.prefix / "var/homebrew/linked/widget"
        linked.parent.mkdir(parents=True)
        linked.symlink_to(keg)
        self.failure(["bootstrap", "packages", "where", "brew:widget"], "apply brew:widget")
        foreign = self.prefix / "Cellar/other/active"
        foreign.mkdir(parents=True)
        nested = keg / "nested"
        nested.mkdir()
        file = self.prefix / "Cellar/widget/file"
        file.write_text("untouched")
        for target in [foreign, nested, keg.parent, file, keg / "missing"]:
            with self.subTest(target=target):
                opt.symlink_to(target)
                self.failure(["bootstrap", "packages", "where", "brew:widget"], "widget")
                opt.unlink()
        opt.mkdir()
        self.failure(["bootstrap", "packages", "where", "brew:widget"], "opt")
        opt.rmdir()
        opt.write_text("untouched")
        self.failure(["bootstrap", "packages", "where", "brew:widget"], "opt")

    def test_argument_and_initialization_errors_keep_original_diagnostics(self):
        """Report argument and environment errors even when unrelated configuration is malformed."""
        for args, diagnostic, extra in [
            (["bootstrap", "packages", "where"], "package", {}),
            (["bootstrap", "packages", "where", "brew:widget", "brew:other"], "brew:other", {}),
            (["bootstrap", "packages", "where", "--unknown-query-flag", "brew:widget"], "--unknown-query-flag", {}),
            (["--cd", str(self.base / "missing-cd"), "bootstrap", "packages", "where", "brew:widget"], "missing-cd", {}),
            (["bootstrap", "packages", "where", "brew:widget"], "invalid-query-bool", {"MISE_YES": "invalid-query-bool"}),
        ]:
            with self.subTest(args=args, extra=extra):
                self.failure(args, diagnostic, extra)

    def test_nearby_non_queries_retain_normal_miserc_initialization(self):
        """Limit isolation to the exact query path rather than matching words in arbitrary arguments."""
        for args in [
            ["exec", "--", "bootstrap", "packages", "where", "brew:widget"],
            ["run", "bootstrap", "packages", "where", "brew:widget"],
            ["--env", "bootstrap", "packages", "where", "brew:widget"],
            ["bootstrap", "--from", "packages", "where", "brew:widget"],
            ["bootstrap", "packages", "status"],
            ["--", "bootstrap", "packages", "where", "brew:widget"],
        ]:
            with self.subTest(args=args):
                result = subprocess.run([self.binary, *args], cwd=self.project, env=self.env, capture_output=True, timeout=20)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(b".miserc.toml", result.stderr)

    def test_config_exec_and_settings_file_errors_are_outside_query_inputs(self):
        """Keep lookup independent of settings-file syntax and executable configuration templates."""
        self.require_supported()
        self.keg()
        (self.project / ".miserc.toml").unlink()
        for relative in ["config/config.toml", "project/mise.toml"]:
            with self.subTest(relative=relative):
                path = self.base / relative
                original = path.read_text()
                path.write_text("invalid-settings-file = [\n")
                self.success()
                path.write_text(original)
        (self.base / "config/config.toml").write_text("")
        marker = self.base / "home/config-ran"
        (self.project / "mise.toml").write_text("[env]\nQUERY_SENTINEL = '{{ exec(command=\"touch " + str(marker) + "\") }}'\n")
        self.success()

    def test_query_help_uses_informational_output(self):
        """Allow help to print usage successfully through the isolated query startup path."""
        result, stderr = self.run_mise(["bootstrap", "packages", "where", "--help"])
        self.assertEqual(result.returncode, 0, stderr)
        self.assertIn(b"where", result.stdout)
        self.assertIn(b"PACKAGE", result.stdout)

    def test_managers_and_identifiers_fail_locally(self):
        """Reject unsupported managers and malformed names with local, query-specific diagnostics."""
        self.require_supported()
        for spec, diagnostic in [
            ("widget", "brew:"), ("brew:", "brew:"), ("unknown:widget", "brew:"),
            ("brew-cask:widget", "brew:"), ("brew:homebrew/cask/widget", "cask"),
            ("brew:../widget", "brew:"), ("brew:/widget", "brew:"),
            ("brew:a//widget", "brew:"), ("brew:a/widget", "brew:"),
            ("brew:a/b/c/widget", "brew:"), ("brew:a/b/widget/", "brew:"),
            ("brew:a/b/..", "brew:"), ("brew:widget\\name", "brew:"),
            ("brew:widget:name", "brew:"), ("brew:wid get", "brew:"),
            ("brew:widget\n", "brew:"), ("brew:widget\r", "brew:"),
        ]:
            with self.subTest(spec=spec):
                self.failure(["bootstrap", "packages", "where", spec], diagnostic)

    def test_output_prefix_rejects_newline_and_carriage_return(self):
        """Protect single-line stdout even when an installation exists under a multiline prefix."""
        self.require_supported()
        for suffix in ["newline\n", "carriage\r"]:
            with self.subTest(suffix=suffix):
                self.prefix = self.base / suffix
                self.keg()
                self.failure(["bootstrap", "packages", "where", "brew:widget"], "UTF-8", {"MISE_SYSTEM_BREW_PREFIX": str(self.prefix)})

    def test_non_utf8_prefix_environment_preserves_encoding_error(self):
        """Preserve non-UTF-8 environment bytes long enough to report the intended encoding error."""
        self.require_supported()
        prefix_bytes = os.fsencode(self.base) + b"/prefix-\xff"
        prefix = os.fsdecode(prefix_bytes)
        self.assertEqual(os.fsencode(prefix), prefix_bytes)
        self.failure(
            ["bootstrap", "packages", "where", "brew:widget"],
            "UTF-8",
            {"MISE_SYSTEM_BREW_PREFIX": prefix},
        )

    def test_local_validation_error_does_not_invoke_github_credentials(self):
        """Keep error text resembling a GitHub response from triggering credential commands."""
        self.require_supported()
        credential = self.write(
            "sentinel-bin/github-credential",
            '#!/bin/sh\ntouch "$HOME/credential-ran"\nprintf "fixture-token\\n"\n',
            executable=True,
        )
        for token in ["GITHUB_TOKEN", "GH_TOKEN", "MISE_GITHUB_TOKEN", "GITHUB_API_TOKEN"]:
            self.assertNotIn(token, self.env)
        spec = "brew:HTTP status client error (403 Forbidden) for url (https://api.github.com"
        self.failure(
            ["bootstrap", "packages", "where", spec],
            "brew:<formula>",
            {"MISE_GITHUB_CREDENTIAL_COMMAND": shlex.quote(str(credential))},
        )
        self.assertFalse((self.base / "home/credential-ran").exists())

    def test_unsupported_platform_is_registered_and_diagnosed(self):
        """Keep the command discoverable on unsupported hosts with an explicit platform diagnostic."""
        if self.supported:
            self.skipTest("unsupported-platform branch runs on other hosts")
        self.failure(["bootstrap", "packages", "where", "brew:widget"], "macOS")


if __name__ == "__main__":
    unittest.main(verbosity=2)
