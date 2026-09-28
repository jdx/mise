// Loads the capture set into the reel's facts (captures.ts ReelData), in
// node: the renderers and the tests call it, and hand the result to the page
// as JSON. Never bundled into the page.
//
// A capture set is what `mise run docs:showreel-capture` writes: versions.json,
// C4.json (the registry names the pinned mise lists), and for each take in
// runs/<run>/<id>/ its frames, marks and keys, the files it left
// (files/), the files as it started (files-start/, where steps.json asks),
// and what the rig saved off camera (offcam/); plus the fixtures the takes
// started from. The committed reference set
// (theme/showreel/test/captures, export.py) has the same layout, with each
// take's files bundled into one files.json and the fixtures into
// fixtures.json, so nothing captured acts on the checkout; both read alike.
//
// Which set: `dir`, else SHOWREEL_CAPTURES, else the rig's out/ when a run
// has published there, else the reference set, so PR CI (no Docker, no
// network) and a fresh checkout still render and test real captures. A take
// the set lacks is listed in `missing`, and the scene that needs it draws a
// labelled "CAPTURE MISSING" box instead.

import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { basename, join, relative } from "node:path";
import {
  type Capture,
  CAPTURE_IDS,
  type CaptureId,
  type ReelData,
  type Versions,
} from "./captures";

export interface LoadOptions {
  /** The capture set's directory (default: see captureSet). */
  dir?: string;
  /** Which recorded run (default: SHOWREEL_CAPTURE_RUN, else "a"). */
  run?: string;
}

/** Where a capture run publishes, under the checkout at `repo`. */
export const runDir = (repo: string): string =>
  join(repo, "docs/.vitepress/showreel-capture/out");

/** The committed reference set, under the checkout at `repo`. */
export const referenceDir = (repo: string): string =>
  join(repo, "docs/.vitepress/theme/showreel/test/captures");

/** The capture set a load reads by default, and why. */
export function captureSet(repo: string): { dir: string; why: string } {
  const env = process.env.SHOWREEL_CAPTURES;
  if (env) return { dir: env, why: "SHOWREEL_CAPTURES" };
  const run = runDir(repo);
  if (existsSync(join(run, "versions.json")))
    return { dir: run, why: "the last capture run" };
  return { dir: referenceDir(repo), why: "the committed reference set" };
}

interface RawFrame {
  t: number;
  rows: number[];
  cursor: [number, number];
  cursor_hidden?: boolean;
}

interface RawFrames {
  width: number;
  height: number;
  styles: { fg: string; bg: string; bold: boolean; dim: boolean }[];
  rows: [string, number][][];
  frames: RawFrame[];
}

const json = <T>(file: string): T =>
  JSON.parse(readFileSync(file, "utf8")) as T;

/** Every file under `dir`, keyed by its path relative to it. */
function walk(dir: string, into: Record<string, string>, prefix: string): void {
  if (!existsSync(dir)) return;
  const visit = (d: string) => {
    for (const name of readdirSync(d).sort()) {
      const p = join(d, name);
      if (statSync(p).isDirectory()) visit(p);
      else into[`${prefix}${relative(dir, p)}`] = readFileSync(p, "utf8");
    }
  };
  visit(dir);
}

/**
 * A take's files as the facts key them: what it left as "C8/work/api/…",
 * what it started with as "C8/start/work/api/…", and what the rig saved off
 * camera as "C8/offcam/…". From its directories, or its files.json bundle.
 */
const FILE_DIRS: readonly [string, string][] = [
  ["files", ""],
  ["files-start", "start/"],
  ["offcam", "offcam/"],
];

function takeFiles(
  dir: string,
  id: CaptureId,
  into: Record<string, string>,
): void {
  const bundle = join(dir, "files.json");
  if (existsSync(bundle)) {
    const all = json<Record<string, string>>(bundle);
    for (const path of Object.keys(all).sort()) {
      const hit = FILE_DIRS.find(([sub]) => path.startsWith(`${sub}/`));
      if (!hit) throw new Error(`${bundle}: unexpected entry ${path}`);
      into[`${id}/${hit[1]}${path.slice(hit[0].length + 1)}`] = all[path];
    }
    return;
  }
  for (const [sub, prefix] of FILE_DIRS)
    walk(join(dir, sub), into, `${id}/${prefix}`);
}

/** One take's screen, marks and keys, or null when its directory lacks them. */
function loadCapture(dir: string, id: CaptureId): Capture | null {
  const frames = join(dir, "frames.json");
  const marks = join(dir, "marks.json");
  if (!existsSync(frames) || !existsSync(marks)) return null;
  const f = json<RawFrames>(frames);
  const input = join(dir, "input.json");
  return {
    id,
    width: f.width,
    height: f.height,
    styles: f.styles.map((s) => ({
      fg: s.fg,
      bg: s.bg,
      bold: s.bold,
      dim: s.dim,
    })),
    rows: f.rows,
    frames: f.frames.map((x) => ({
      t: x.t,
      rows: x.rows,
      cursor: x.cursor,
      hidden: Boolean(x.cursor_hidden),
    })),
    marks: json<{ name: string; t: number }[]>(marks).map((m) => ({
      name: m.name,
      t: m.t,
    })),
    keys: existsSync(input) ? json<[number, string][]>(input) : [],
  };
}

/**
 * The facts for a render from a capture set (see captureSet), or null when
 * there is none (no versions.json). `report` says what was read.
 */
export function loadFacts(
  repo: string,
  o: LoadOptions = {},
): { facts: ReelData | null; report: string[] } {
  const set = o.dir
    ? { dir: o.dir, why: "the given directory" }
    : captureSet(repo);
  const out = set.dir;
  const runName = o.run ?? process.env.SHOWREEL_CAPTURE_RUN ?? "a";
  const report: string[] = [];
  const versionsFile = join(out, "versions.json");
  if (!existsSync(versionsFile)) {
    report.push(`no capture set: ${versionsFile} is missing`);
    return { facts: null, report };
  }
  const versions = json<Versions & { registry_count?: number }>(versionsFile);
  const runPath = join(out, "runs", runName);
  const captures: Partial<Record<CaptureId, Capture>> = {};
  const files: Record<string, string> = {};
  const missing: string[] = [];
  for (const id of CAPTURE_IDS) {
    const dir = join(runPath, id);
    const c = loadCapture(dir, id);
    if (c) captures[id] = c;
    else missing.push(id);
    takeFiles(dir, id, files);
  }
  const fixtures: Record<string, string> = {};
  const fixtureBundle = join(runPath, "fixtures.json");
  if (existsSync(fixtureBundle))
    Object.assign(fixtures, json<Record<string, string>>(fixtureBundle));
  else walk(join(runPath, "fixtures"), fixtures, "");
  let variant = "systemd";
  const run2 = join(runPath, "run-machine2.json");
  if (existsSync(run2)) {
    const r = json<{ variant?: string; machine2?: string }>(run2);
    variant = r.variant ?? r.machine2 ?? variant;
  }
  // C4: the names the pinned mise's `mise registry` lists. A set recorded
  // before the rig saved them falls back to this checkout's registry/.
  const c4 = join(out, "C4.json");
  const c4doc = existsSync(c4)
    ? json<{ count: number; names?: string[] }>(c4)
    : null;
  if (!c4doc) missing.push("C4");
  let names = c4doc?.names ?? [];
  if (c4doc && !c4doc.names) {
    const registryDir = join(repo, "registry");
    names = existsSync(registryDir)
      ? readdirSync(registryDir)
          .filter((f) => f.endsWith(".toml"))
          .map((f) => basename(f, ".toml"))
          .sort()
      : [];
    report.push(
      "registry names from this checkout's registry/ (the set predates C4.json's names)",
    );
  }
  const count = c4doc?.count ?? versions.registry_count ?? names.length;
  report.unshift(
    `capture set: ${relative(repo, out) || out} (${set.why})`,
    `captures from runs/${runName}: ${Object.keys(captures).join(" ")}`,
    missing.length ? `missing: ${missing.join(" ")}` : "missing: none",
    `node ${versions.node.lts_major}/${versions.node.other_major} (${versions.node.lts_version}, ${versions.node.other_version}), mise ${versions.mise.version}, machine 2 ${variant}`,
  );
  return {
    facts: {
      schema: 1,
      run: runName,
      variant,
      versions: {
        mise: versions.mise,
        node: versions.node,
        tools: versions.tools,
      },
      captures,
      files,
      fixtures,
      registry: { count, names },
      missing,
    },
    report,
  };
}
