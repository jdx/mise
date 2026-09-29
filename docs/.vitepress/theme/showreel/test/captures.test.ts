// The capture sets the reel is rendered and tested with (load.ts). PR CI has
// no Docker and no network, so the checkout carries a reference set,
// test/captures: one real run of docs/.vitepress/showreel-capture, trimmed
// by its export.py to what the loader reads. When no capture run has
// published on the machine, the loader reads it, so every test that needs
// captures (forbidden, timing, handoff frames) runs on real output there too.
//
// These tests hold the reference set to the rig as it is now (a change to
// the rig means recording it again: `mise run docs:showreel-capture --
// --update-reference`), to the cache key's documented formula, and, where a
// real capture run is on the machine, hold the loader to reading the two
// alike: the same takes, marks, files and fields, and, when the reference
// set was exported from that very run, the same facts exactly.

import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import {
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
import { type Capture, CAPTURE_IDS, type ReelData } from "../captures";
import { captureSet, loadFacts, referenceDir, runDir } from "../load";
import { REPO } from "./repo";

const RIG = join(REPO, "docs/.vitepress/showreel-capture");
const REF = referenceDir(REPO);

const json = <T>(file: string): T =>
  JSON.parse(readFileSync(file, "utf8")) as T;
const sha256 = (data: string | Buffer) =>
  createHash("sha256").update(data).digest("hex");

interface VersionsDoc {
  key: string;
  keyed: Record<string, string | number | null>;
  rig: { sha256: string; files: Record<string, string> };
}

interface Source {
  key: string;
  recorded_at: string;
  shape_ok: boolean;
  captures: string[];
  machine2: string;
}

/** JSON with sorted keys and no spaces: Python's json.dumps(sort_keys=True, separators=(",", ":")). */
function canonical(v: unknown): string {
  if (v === null || typeof v !== "object") return JSON.stringify(v);
  if (Array.isArray(v)) return `[${v.map(canonical).join(",")}]`;
  const o = v as Record<string, unknown>;
  return `{${Object.keys(o)
    .sort()
    .map((k) => `${JSON.stringify(k)}:${canonical(o[k])}`)
    .join(",")}}`;
}

const reference = loadFacts(REPO, { dir: REF });

test("the reference set holds every take the reel shows", () => {
  const f = reference.facts;
  assert.ok(f, reference.report.join("; "));
  assert.deepEqual(f.missing, [], "takes missing from the reference set");
  for (const id of CAPTURE_IDS) {
    const c: Capture | undefined = f.captures[id];
    assert.ok(c, `no ${id}`);
    assert.ok(c.frames.length > 0 && c.marks.length > 0, `${id} is empty`);
  }
  assert.ok(
    f.registry.count >= 1000 && f.registry.names.length === f.registry.count,
    `C4 lists ${f.registry.names.length} names (count ${f.registry.count}); the reel says 1,000+`,
  );
  assert.ok(Object.keys(f.fixtures).length > 0, "no fixtures");
  // What scenes read beyond the screens: the packslip statements mise saved,
  // the file before C8's upgrade, and the history after the rollback.
  const { hk_old: oldV, hk_new: newV } = f.versions.tools;
  for (const v of [oldV, newV]) {
    const slip: string | undefined = f.files[`C8/offcam/packslip-hk-${v}.json`];
    assert.ok(slip, `no packslip statement for hk ${v}`);
    assert.equal(
      (JSON.parse(slip) as { predicate: { version: string } }).predicate
        .version,
      v,
    );
  }
  const start = f.files["C8/start/work/api/mise.toml"] ?? "";
  assert.ok(
    start.includes(`hk = "${oldV}"`),
    `C8 starts with hk = "${oldV}":\n${start}`,
  );
  assert.ok(f.files["C15/offcam/history-after.txt"], "no history after");
});

test("the reference set was recorded by the capture rig as it is now", () => {
  const v = json<VersionsDoc>(join(REF, "versions.json"));
  const source = json<Source>(join(REF, "source.json"));
  const changed = Object.entries(v.rig.files)
    .filter(([name, sum]) => {
      const p = join(RIG, name);
      return !existsSync(p) || sha256(readFileSync(p)) !== sum;
    })
    .map(([name]) => name);
  assert.deepEqual(
    changed,
    [],
    `the capture rig changed since the reference set was recorded (${changed.join(", ")}): ` +
      "record it again with `mise run docs:showreel-capture -- --update-reference` and commit " +
      "docs/.vitepress/theme/showreel/test/captures",
  );
  // versions.py's rig hash: each file's name, a NUL, its bytes and a NUL
  const h = createHash("sha256");
  for (const name of Object.keys(v.rig.files)) {
    h.update(`${name}\0`);
    h.update(readFileSync(join(RIG, name)));
    h.update("\0");
  }
  assert.equal(h.digest("hex"), v.rig.sha256, "rig hash");
  assert.equal(v.keyed.rig, v.rig.sha256, "the key hashes the rig");
  assert.equal(source.key, v.key);
  assert.equal(source.shape_ok, true, "exported from a run that passed");
});

test("the cache key is the sha256 of the keyed JSON, and only the keyed JSON", () => {
  const v = json<VersionsDoc & { not_keyed: Record<string, unknown> }>(
    join(REF, "versions.json"),
  );
  assert.equal(sha256(canonical(v.keyed)), v.key);
  // The key follows Node's majors and the rig, never patch releases or the
  // mise version (versions.py: "The cache key").
  assert.deepEqual(Object.keys(v.keyed).sort(), [
    "node_lts_major",
    "node_other_major",
    "rig",
    "schema",
  ]);
  for (const k of ["mise", "node_lts_version", "node_other_version", "jq"])
    assert.ok(k in v.not_keyed, `not_keyed lacks ${k}`);
});

test("the loader reads SHOWREEL_CAPTURES, else a published run, else the reference set", () => {
  const saved = process.env.SHOWREEL_CAPTURES;
  const repo = mkdtempSync(join(tmpdir(), "showreel-captures-"));
  try {
    delete process.env.SHOWREEL_CAPTURES;
    assert.equal(captureSet(repo).dir, referenceDir(repo));
    mkdirSync(runDir(repo), { recursive: true });
    assert.equal(captureSet(repo).dir, referenceDir(repo), "an empty out/");
    writeFileSync(join(runDir(repo), "versions.json"), "{}");
    assert.equal(captureSet(repo).dir, runDir(repo));
    process.env.SHOWREEL_CAPTURES = "/elsewhere";
    assert.equal(captureSet(repo).dir, "/elsewhere");
  } finally {
    if (saved === undefined) delete process.env.SHOWREEL_CAPTURES;
    else process.env.SHOWREEL_CAPTURES = saved;
    rmSync(repo, { recursive: true, force: true });
  }
});

/** A set's shape: everything but the values that differ between two runs. */
function shape(f: ReelData) {
  return {
    missing: [...f.missing],
    variant: f.variant,
    versions: {
      mise: Object.keys(f.versions.mise).sort(),
      node: Object.keys(f.versions.node).sort(),
      tools: Object.keys(f.versions.tools).sort(),
    },
    captures: Object.fromEntries(
      CAPTURE_IDS.map((id) => {
        const c = f.captures[id];
        return [
          id,
          c && {
            size: [c.width, c.height],
            marks: c.marks.map((m) => m.name),
            style: Object.keys(c.styles[0] ?? {}).sort(),
            frame: Object.keys(c.frames[0] ?? {}).sort(),
            framed: c.frames.length > 0 && c.rows.length > 0,
            keyed: c.keys.length > 0,
          },
        ];
      }),
    ),
    files: Object.keys(f.files).sort(),
    fixtures: Object.keys(f.fixtures).sort(),
    registry: f.registry.names.length > 0 && f.registry.count > 0,
  };
}

test("a real capture run and the reference set agree on shape", (t) => {
  const set = captureSet(REPO);
  if (set.dir === REF) {
    t.skip(
      "no capture run on this machine (the loader reads the reference set)",
    );
    return;
  }
  const real = loadFacts(REPO, { dir: set.dir });
  assert.ok(real.facts, real.report.join("; "));
  assert.ok(reference.facts, reference.report.join("; "));
  if (real.facts.variant !== reference.facts.variant) {
    t.skip(
      `the run recorded machine 2 as ${real.facts.variant}, the reference set as ${reference.facts.variant}`,
    );
    return;
  }
  assert.deepEqual(shape(real.facts), shape(reference.facts));
  // Exported from this very run: the loader must read the trimmed set into
  // exactly the same facts.
  const source = json<Source>(join(REF, "source.json"));
  const meta = join(set.dir, "runs/a", CAPTURE_IDS[0], "meta.json");
  const started = existsSync(meta)
    ? json<{ started_at?: string }>(meta).started_at
    : undefined;
  if (started === source.recorded_at)
    assert.deepEqual(real.facts, reference.facts);
  else
    t.diagnostic(
      `the run (${started}) is not the one the reference set came from (${source.recorded_at}); shapes only`,
    );
});
