import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { REPO } from "./repo";

for (const overview of [false, true]) {
  test(`cache fallback restores a ${overview ? "two-film" : "legacy"} render and clears stale optional files`, () => {
    const root = mkdtempSync(join(tmpdir(), "mise-showreel-cache-"));
    try {
      const scripts = join(root, "xtasks/docs");
      const publicDir = join(root, "docs/public");
      const cache = join(root, "cache");
      const kept = join(cache, "renders/good");
      for (const dir of [scripts, publicDir, kept])
        mkdirSync(dir, { recursive: true });
      const script = join(scripts, "showreel");
      copyFileSync(join(REPO, "xtasks/docs/showreel"), script);
      writeFileSync(join(cache, "last-good"), "good\n");
      const required = [
        "showreel-120.mp4",
        "showreel.mp4",
        "showreel-poster.jpg",
      ];
      const optional = [
        "showreel-overview.mp4",
        "showreel-overview-chapters.vtt",
      ];
      for (const name of [
        ...required,
        "showreel-chapters.vtt",
        ...(overview ? optional : []),
      ])
        writeFileSync(join(kept, name), `kept ${name}`);
      for (const name of optional)
        writeFileSync(join(publicDir, name), "stale");
      const run = spawnSync("bash", [script, "--cache", cache, "--fallback"], {
        env: { ...process.env, GITHUB_ENV: "", GITHUB_OUTPUT: "" },
        encoding: "utf8",
      });
      assert.equal(run.status, 0, run.stderr);
      for (const name of required)
        assert.equal(
          readFileSync(join(publicDir, name), "utf8"),
          `kept ${name}`,
        );
      assert.equal(
        readFileSync(join(publicDir, "showreel-chapters.vtt"), "utf8"),
        "kept showreel-chapters.vtt",
      );
      for (const name of optional) {
        assert.equal(existsSync(join(publicDir, name)), overview);
        if (overview)
          assert.equal(
            readFileSync(join(publicDir, name), "utf8"),
            `kept ${name}`,
          );
      }
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });
}
