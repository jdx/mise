// from jdx/hk@37937824 docs/.vitepress/theme/showreel/test/chapters.test.ts
// The served chapters follow the landing-page edit; the source chapters
// still describe the complete scene clock. When they change, rewrite with
// `UPDATE_CHAPTERS=1 aube run test:showreel` and commit both tracks.

import assert from "node:assert/strict";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import { ACTS, CHAPTERS, chaptersVtt, DURATION } from "../timeline";
import { REPO } from "./repo";
import { filmChaptersVtt } from "../edit";

for (const [edition, name] of [
  ["tour", "showreel-chapters.vtt"],
  ["overview", "showreel-overview-chapters.vtt"],
] as const) {
  test(`docs/public/${name} follows the delivered ${edition}`, () => {
    const file = join(REPO, "docs/public", name);
    const expected = filmChaptersVtt(edition);
    if (process.env.UPDATE_CHAPTERS) writeFileSync(file, expected);
    const served = existsSync(file) ? readFileSync(file, "utf8") : "";
    assert.equal(
      served,
      expected,
      "the chapters track is stale: run `UPDATE_CHAPTERS=1 aube run test:showreel`",
    );
  });
}

test("the chapters track has one cue per act, end to end", () => {
  const blocks = chaptersVtt().trimEnd().split("\n\n");
  assert.equal(blocks[0], "WEBVTT");
  const cues = blocks.slice(1).map((b) => b.split("\n"));
  assert.equal(cues.length, ACTS.length);
  assert.equal(cues.length, 9);
  const time = (s: string) => {
    const [h, m, sec] = s.split(":");
    return Number(h) * 3600 + Number(m) * 60 + Number(sec);
  };
  let last = 0;
  cues.forEach(([id, span, label], i) => {
    const [a, b] = span.split(" --> ").map(time);
    assert.equal(id, ACTS[i].id);
    assert.equal(label, ACTS[i].label);
    assert.ok(Math.abs(a - CHAPTERS[i].start) < 5e-4);
    assert.ok(Math.abs(b - CHAPTERS[i].end) < 5e-4);
    assert.ok(
      Math.abs(a - last) < 5e-4,
      `${id} starts where the last cue ends`,
    );
    last = b;
    assert.match(span, /^\d\d:\d\d:\d\d\.\d{3} --> \d\d:\d\d:\d\d\.\d{3}$/);
  });
  assert.ok(Math.abs(last - DURATION) < 5e-4);
});
