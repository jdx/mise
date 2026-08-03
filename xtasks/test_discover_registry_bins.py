import importlib.util
import sys
import tempfile
import unittest
from pathlib import Path


MODULE_PATH = Path(__file__).with_name("discover_registry_bins.py")
SPEC = importlib.util.spec_from_file_location("discover_registry_bins", MODULE_PATH)
assert SPEC and SPEC.loader
discovery = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = discovery
SPEC.loader.exec_module(discovery)


class DiscoverRegistryBinsTest(unittest.TestCase):
    def test_normalize_bins(self):
        entries = [
            {"name": "tool.exe"},
            {"name": "helper"},
            {"name": "helper"},
            {"name": "nested/tool"},
            {"name": ""},
            {},
        ]
        self.assertEqual(discovery.normalize_bins(entries), ["helper", "tool"])

    def test_parse_sections(self):
        stdout = """ignored
@@MISE_REGISTRY_BIN_DISCOVERY_TOOL@@
{"backend":"aqua:owner/tool"}
@@MISE_REGISTRY_BIN_DISCOVERY_VERSIONS@@
{"tool":[{"version":"1.2.3"}]}
@@MISE_REGISTRY_BIN_DISCOVERY_BINS@@
[{"name":"tool","path":"/state/tool","symlink":false}]
"""
        parsed = discovery.parse_sections(stdout)
        self.assertEqual(parsed["tool"]["backend"], "aqua:owner/tool")
        self.assertEqual(discovery.installed_version("tool", parsed["versions"]), "1.2.3")

    def test_installed_version_accepts_mise_ls_array(self):
        versions = [{"version": "1.2.3", "installed": True}]
        self.assertEqual(discovery.installed_version("tool", versions), "1.2.3")

    def test_container_has_no_host_mounts(self):
        options = discovery.SandboxOptions(
            engine="docker",
            image="test-image",
            platform="linux/amd64",
            memory="1g",
            cpus="1",
            pids=64,
            state_size="2g",
        )
        args = discovery.container_base_args(options)
        self.assertIn("--read-only", args)
        self.assertIn("--cap-drop=ALL", args)
        self.assertNotIn("--volume", args)
        self.assertNotIn("-v", args)
        self.assertEqual(args[-1], "test-image")

    def test_resume_shard_and_collision_artifact(self):
        existing = {
            "a": {"tool": "a", "status": "success", "bins": ["shared"]},
            "b": {"tool": "b", "status": "failed"},
        }
        selected = discovery.select_tools(
            ["a", "b", "c", "d"],
            ["a", "b", "c", "d"],
            existing,
            shard_count=1,
            shard_index=0,
            limit=None,
            skip_failures=False,
        )
        self.assertEqual(selected, ["b", "c", "d"])

        results = {
            **existing,
            "c": {"tool": "c", "status": "success", "bins": ["shared", "unique"]},
        }
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "results.json"
            discovery.write_artifact(path, "linux/amd64", results)
            loaded = discovery.load_results(path, "linux/amd64")
            self.assertEqual(set(loaded), {"a", "b", "c"})
            self.assertEqual(discovery.collisions(loaded), {"shared": ["a", "c"]})


if __name__ == "__main__":
    unittest.main()
