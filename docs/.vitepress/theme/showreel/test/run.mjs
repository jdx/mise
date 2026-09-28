// from jdx/hk@37937824 docs/.vitepress/theme/showreel/test/run.mjs
// Runs the showreel's tests: every *.test.ts under the showreel directory,
// bundled with esbuild the way the renderer bundles the reel (types are
// stripped, not checked; `aube run typecheck:showreel` checks them) and run
// with node's test runner. A new test file is picked up by its name alone.

import { spawnSync } from "node:child_process";
import { globSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { build } from "esbuild";

const root = dirname(dirname(fileURLToPath(import.meta.url)));
// `run.mjs handoff captions` runs only the test files whose paths contain one
// of the words; with no words it runs them all.
const only = process.argv.slice(2);
const tests = globSync("**/*.test.ts", { cwd: root })
  .filter((t) => !only.length || only.some((word) => t.includes(word)))
  .sort();
if (!tests.length) {
  console.error(
    `no *.test.ts files under ${root}${only.length ? ` matching ${only.join(", ")}` : ""}`,
  );
  process.exit(1);
}

const out = mkdtempSync(join(tmpdir(), "showreel-test-"));
try {
  await build({
    entryPoints: tests.map((t) => join(root, t)),
    outbase: root,
    outdir: out,
    outExtension: { ".js": ".mjs" },
    bundle: true,
    platform: "node",
    format: "esm",
    target: "node22",
    sourcemap: "inline",
    logLevel: "error",
  });
  const run = spawnSync(
    process.execPath,
    [
      "--enable-source-maps",
      "--test",
      ...tests.map((t) => join(out, t.replace(/\.ts$/, ".mjs"))),
    ],
    { stdio: "inherit" },
  );
  process.exitCode = run.status ?? 1;
} finally {
  rmSync(out, { recursive: true, force: true });
}
