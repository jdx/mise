// What the reel must never show (plan v3 §8 forbidden.test, and jdx's
// standing rules): trust or untrust, asdf or .tool-versions, experimental
// status, a `mise WARN`, a task run's `Finished in`, an install summary
// (`installed … in`), or a transfer rate. Checked over every caption, under
// the capture set's facts, and over every string the reel draws: the whole
// reel is rendered at 30 fps on a recording canvas in node, with the kit's
// text tap (kit/ink.ts) collecting each terminal line, label and caption as
// it is set, and every raw fillText besides (test/record.ts). Scenes and captions are also
// held to writing no version number themselves: those come from the
// capture run's versions file.
//
// Needs the capture set (docs/.vitepress/showreel-capture/out, or
// SHOWREEL_CAPTURES); without one the drawn-text checks are skipped, unless
// SHOWREEL_REQUIRE_CAPTURES is set.

import assert from "node:assert/strict";
import { readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import { type ReelFacts, sec, SECTIONS } from "../bible";
import { loadFacts } from "../load";
import { scenes } from "../scenes";
import { plain } from "../type";
import { inkFrames } from "./record";
import { REPO, SHOWREEL } from "./repo";

/** Everything the reel may never show, by name. */
export const FORBIDDEN: readonly (readonly [string, RegExp])[] = [
  ["trust", /trust/i],
  ["untrust", /untrust/i],
  ["asdf", /asdf/i],
  [".tool-versions", /\.tool-versions/i],
  ["experimental", /experimental/i],
  ["mise WARN", /mise WARN/],
  ["Finished in", /Finished in/],
  ["installed … in", /\binstalled\b.*\bin\b/i],
  ["a transfer rate (MB/s)", /\d(?:\.\d+)?\s?[kKMG]i?B\/s/],
];

function violations(text: string): string[] {
  return FORBIDDEN.filter(([, re]) => re.test(text)).map(([name]) => name);
}

test("the forbidden patterns catch what they name, and pass what they do not", () => {
  const bad = [
    "mise trust ~/work/api",
    "mise untrust",
    "asdf:foo",
    "legacy .tool-versions file",
    "daemons       [experimental] Manage project daemons",
    "mise WARN  missing: npm:prettier@3.9.9",
    "Finished in 63.8ms",
    "✓ installed 1 tool in 1.1s: jq@1.8.2",
    "mise by @jdx  0/1 · 11.6 MB/s · 2.8s",
    "jq@1.8.2 downloading 128 kB/s",
  ];
  for (const t of bad) assert.ok(violations(t).length, `not caught: ${t}`);
  const good = [
    "fresh machine · mise installed",
    "✔ [shop/postgres] started on port 5432",
    "[build] sources up-to-date, skipping",
    "jq@1.8.2 downloading  1.7/2.3 MB",
    "mise ERROR Invalid choice for arg env: prod, expected one of staging, production",
  ];
  for (const t of good) assert.deepEqual(violations(t), [], `caught: ${t}`);
});

const { facts, report } = loadFacts(REPO);

test("no caption says any of it, under the capture set's facts", (t) => {
  const sets: [string, ReelFacts | null][] = [
    ["no facts", null],
    ["the capture set", facts as unknown as ReelFacts],
  ];
  for (const [what, f] of sets)
    for (const s of scenes)
      for (const c of s.captions?.(f) ?? [])
        for (const l of c.lines)
          assert.deepEqual(
            violations(plain(l.text)),
            [],
            `${s.id} (${what}): "${plain(l.text)}"`,
          );
  t.diagnostic(report.join("; "));
});

test("nothing the reel draws, at any frame of a 30 fps render, says any of it", (t) => {
  if (!facts) {
    if (process.env.SHOWREEL_REQUIRE_CAPTURES) assert.fail(report.join("; "));
    t.skip(`no capture set: ${report.join("; ")}`);
    return;
  }
  const seen = new Map<string, { t: number; kind: string }>();
  const frames = inkFrames(facts as unknown as ReelFacts, 30, (at, inked) => {
    for (const { text, kind } of inked)
      if (!seen.has(text)) seen.set(text, { t: at, kind });
  });
  t.diagnostic(`${frames} frames drawn; ${seen.size} distinct strings`);
  const sectionAt = (at: number) =>
    SECTIONS.find(({ id }) => at < sec(id).end)?.id ?? "end";
  // For review: every string drawn, with the first moment it was.
  if (process.env.SHOWREEL_DUMP_INK)
    writeFileSync(
      process.env.SHOWREEL_DUMP_INK,
      [...seen]
        .sort((a, b) => a[1].t - b[1].t)
        .map(
          ([text, { t: at, kind }]) =>
            `${at.toFixed(3)}\t${sectionAt(at)}\t${kind}\t${text}`,
        )
        .join("\n"),
    );
  const bad: string[] = [];
  for (const [text, { t: at, kind }] of seen) {
    const v = violations(text);
    if (v.length)
      bad.push(
        `${at.toFixed(3)} s (${sectionAt(at)}, ${kind}): ${v.join(", ")} in ${JSON.stringify(text)}`,
      );
  }
  assert.deepEqual(
    bad,
    [],
    `the reel draws forbidden text:\n${bad.join("\n")}`,
  );
  // Every take's terminal lines were drawn: the tap saw the reel's panes.
  const term = [...seen.values()].filter((x) => x.kind === "term").length;
  assert.ok(term > 200, `only ${term} terminal lines were drawn`);
});

/** Every .ts file under `dir`, recursively. */
function tsFiles(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((e) =>
    e.isDirectory()
      ? tsFiles(join(dir, e.name))
      : e.name.endsWith(".ts")
        ? [join(dir, e.name)]
        : [],
  );
}

test("no scene writes a version number: they come from the capture run", () => {
  // Every scene and its helpers (scenes/g1-brand … g6-machine), every kit
  // module and diagram, and the modules that place them.
  const files = [
    ...tsFiles(join(SHOWREEL, "scenes")),
    ...tsFiles(join(SHOWREEL, "kit")),
    ...["handoff.ts", "storyboard.ts", "compose.ts", "whip.ts", "type.ts"].map(
      (f) => join(SHOWREEL, f),
    ),
  ];
  assert.ok(files.length > 60, `only ${files.length} files scanned`);
  for (const f of files) {
    let src = readFileSync(f, "utf8");
    // The chef's SVG path data (logo-dark.svg's `d` attributes) is numbers
    // run together, not versions.
    if (f.endsWith("kit/chef.ts"))
      src = src.replace(/export const CHEF_PATHS[\s\S]*?\n\];/, "");
    assert.doesNotMatch(
      src,
      /\b\d+\.\d+\.\d+\b/,
      `${f} writes a version number`,
    );
    // A Node major as a string literal ("24", "26").
    assert.doesNotMatch(
      src,
      /["'`](?:2[0-9]|3[0-9])["'`]/,
      `${f} writes a Node major`,
    );
  }
});
