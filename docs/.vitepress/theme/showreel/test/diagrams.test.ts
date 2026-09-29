// The diagram kit (kit/diagrams): each diagram reads its strings from the
// capture set (never a version of its own), shows only what the take
// recorded, and is a pure function of its inputs that comes to rest after
// its last cue. The models are checked against the real captures, the
// drawing on a recording context through the kit's ink tap, and the
// version-dependent parts again under made-up versions, so nothing of
// today's run is baked in.

import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import {
  capture,
  CUT_ON,
  fileOf,
  fixture,
  lastBefore,
  type ReelData,
  screenAt,
  stepOf,
} from "../captures";
import { setInkSink } from "../kit/ink";
import { ciDock, ciPanel, ciRows } from "../kit/diagrams/ci-panel";
import { folders, cwdState } from "../kit/diagrams/folders";
import {
  lanes,
  LANES_BADGE,
  laneModel,
  taskLines,
} from "../kit/diagrams/lanes";
import {
  ledger,
  ledgerRows,
  lockChipSize,
  lockEntries,
} from "../kit/diagrams/ledger";
import { riverNames, rivers } from "../kit/diagrams/rivers";
import {
  installPath,
  linkedRows,
  projectPath,
  skillsLink,
} from "../kit/diagrams/skills-link";
import {
  abridgeKey,
  type Packslip,
  packslipOf,
  slip,
  SLIP_BADGE,
  slipRows,
} from "../kit/diagrams/slip";
import { bootstrapLabels, tiles, tileState } from "../kit/diagrams/tiles";
import { run, type Run } from "../kit/term";
import { loadFacts } from "../load";
import { REPO, SHOWREEL } from "./repo";

// A recording 2D context: every call and property set, in order, with
// numbers rounded, so two draws can be compared for equality.

interface Recorder {
  ctx: CanvasRenderingContext2D;
  ops: string[];
}

function recorder(): Recorder {
  const ops: string[] = [];
  const fmt = (v: unknown): string =>
    typeof v === "number"
      ? String(Math.round(v * 1000) / 1000)
      : typeof v === "string"
        ? v
        : v && typeof v === "object" && "stops" in v
          ? `gradient(${(v as { stops: string[] }).stops.join(",")})`
          : typeof v;
  const gradient = () => {
    const stops: string[] = [];
    return { stops, addColorStop: (_: number, c: string) => stops.push(c) };
  };
  const target: Record<string | symbol, unknown> = {
    canvas: { width: 1920, height: 1080 },
    font: "10px sans-serif",
    globalAlpha: 1,
    measureText(text: string) {
      const px = Number(
        /(\d+(?:\.\d+)?)px/.exec(String(target.font))?.[1] ?? 10,
      );
      return { width: Array.from(text).length * px * 0.56 };
    },
    createLinearGradient: gradient,
    createRadialGradient: gradient,
    createPattern: () => null,
    getTransform: () => ({ a: 1, b: 0, c: 0, d: 1, e: 0, f: 0 }),
  };
  const ctx = new Proxy(target, {
    get: (o, k) =>
      k in o
        ? typeof o[k] === "function" && k !== "measureText"
          ? (...a: unknown[]) => {
              ops.push(`${String(k)}(${a.map(fmt).join(",")})`);
              return (o[k] as (...x: unknown[]) => unknown)(...a);
            }
          : o[k]
        : (...a: unknown[]) => {
            ops.push(`${String(k)}(${a.map(fmt).join(",")})`);
          },
    set: (o, k, v) => {
      ops.push(`${String(k)}=${fmt(v)}`);
      o[k] = v;
      return true;
    },
  });
  return { ctx: ctx as unknown as CanvasRenderingContext2D, ops };
}

/**
 * Draw `fn` on a recorder, returning its ops and every string it inked.
 * It draws once first, unrecorded, so type.ts's measurement cache is warm
 * and two draws of the same frame record the same ops.
 */
function draw(fn: (ctx: CanvasRenderingContext2D) => void): {
  ops: string[];
  inked: string[];
} {
  const g = globalThis as { document?: unknown };
  const had = g.document;
  // fx.glow makes its sprite on a canvas of its own.
  g.document = {
    createElement: () => ({
      width: 0,
      height: 0,
      getContext: () => recorder().ctx,
    }),
  };
  const inked: string[] = [];
  const prev = setInkSink(null);
  try {
    fn(recorder().ctx);
    setInkSink((text) => inked.push(text));
    const r = recorder();
    fn(r.ctx);
    return { ops: r.ops, inked };
  } finally {
    setInkSink(prev);
    g.document = had;
  }
}

const { facts, report } = loadFacts(REPO);
const d = facts as ReelData | null;

/** Skip a test that needs the capture set when there is none (unless it is required). */
function needFacts(t: { skip: (m: string) => void }): ReelData | null {
  if (d) return d;
  if (process.env.SHOWREEL_REQUIRE_CAPTURES) assert.fail(report.join("; "));
  t.skip(`no capture set: ${report.join("; ")}`);
  return null;
}

const text = (runs: readonly Run[]) => runs.map((r) => r.text).join("");

test("no diagram writes a version number or a Node major: they come from the capture set", () => {
  const dir = join(SHOWREEL, "kit/diagrams");
  const files = readdirSync(dir).filter((f) => f.endsWith(".ts"));
  assert.ok(files.length >= 8, `only ${files.length} diagram modules`);
  for (const f of files) {
    const src = readFileSync(join(dir, f), "utf8");
    assert.doesNotMatch(
      src,
      /\b\d+\.\d+\.\d+\b/,
      `${f} writes a version number`,
    );
    assert.doesNotMatch(
      src,
      /["'`](?:2[0-9]|3[0-9])["'`]/,
      `${f} writes a Node major`,
    );
  }
});

// Rivers.

test("rivers: node and terraform each ride their stream exactly once, crossing the middle on their beat", (t) => {
  const f = needFacts(t);
  if (!f) return;
  const names = f.registry.names;
  for (const b of [0, 1.5, 3, 4.5, 6, 7.9]) {
    const placed = riverNames({ names, b, cross: 4.5 });
    const lit = placed.filter((p) => p.lit);
    for (const [name, river] of [
      ["node", 1],
      ["terraform", 2],
    ] as const) {
      const all = placed.filter((p) => p.name === name);
      assert.equal(all.length, 1, `${name} at b${b}: ${all.length} times`);
      assert.ok(all[0].lit, `${name} at b${b} is not lit`);
      assert.equal(all[0].river, river, `${name} rides stream ${all[0].river}`);
    }
    assert.equal(lit.length, 2);
    for (const p of placed)
      assert.ok(names.includes(p.name), `${p.name} is not a registry name`);
  }
  // On its beat a lit name's centre is at x 650, the middle of x 540–760.
  for (const p of riverNames({ names, b: 4.5, cross: 4.5 }).filter(
    (x) => x.lit,
  ))
    assert.ok(Math.abs(p.x - 650) < 1, `${p.name} crosses at x ${p.x}`);
  // Names the reel may never show stay out of the streams.
  const bad = ["asdf-plugin", "trustfall", ...names.slice(0, 40)];
  for (const b of [0, 2, 4])
    assert.ok(
      riverNames({ names: bad, b, cross: 2 }).every(
        (p) => !/asdf|trust/.test(p.name),
      ),
    );
});

test("rivers: a pure function of the names and the beat; the underline waits for its cue", (t) => {
  const f = needFacts(t);
  if (!f) return;
  const o = {
    names: f.registry.names,
    b: 3.3,
    cross: 4.5,
    named: { node: 3.5 },
  };
  const a = draw((ctx) => rivers(ctx, o));
  const b = draw((ctx) => rivers(ctx, o));
  assert.deepEqual(a.ops, b.ops);
  assert.ok(a.inked.includes("node") && a.inked.includes("terraform"));
  // Before its cue there is no underline: the same ops with or without it.
  const none = draw((ctx) => rivers(ctx, { ...o, named: {} }));
  assert.deepEqual(a.ops, none.ops);
  const after = draw((ctx) => rivers(ctx, { ...o, b: 4.2 }));
  const afterNone = draw((ctx) => rivers(ctx, { ...o, b: 4.2, named: {} }));
  assert.notDeepEqual(after.ops, afterNone.ops);
});

// Lanes.

/** C11's task lines, each printing half a beat after the last. */
function c11(f: ReelData) {
  const c = capture(f, "C11")!;
  const ci = stepOf(c, "ci");
  const until = lastBefore(c, CUT_ON, ci.type, ci.end);
  const lines = taskLines(c, { after: ci.type, until });
  return { c, ci, until, lines };
}

test("lanes: one per task, in the order the take printed them, with its prefix and lines as captured", (t) => {
  const f = needFacts(t);
  if (!f) return;
  const { ci, until, lines } = c11(f);
  // Every line printed inside the run, in order.
  lines.forEach((l, i) => {
    assert.ok(l.t > ci.type && l.t <= until + 1e-9, `line ${i} at ${l.t}`);
    if (i) assert.ok(l.t >= lines[i - 1].t);
  });
  const model = laneModel(lines.map((l) => ({ runs: l.runs, at: 0 })));
  const order = [
    ...new Set(lines.map((l) => /^\[(\w+)\]/.exec(text(l.runs))![1])),
  ];
  assert.deepEqual(
    model.map((l) => l.name),
    order,
  );
  assert.deepEqual(
    model.filter((l) => l.root).map((l) => l.name),
    ["ci"],
  );
  for (const lane of model) {
    const own = lines.filter((l) => text(l.runs).startsWith(`[${lane.name}] `));
    assert.equal(text(lane.label), `[${lane.name}]`);
    // The label keeps the capture's own colour and weight.
    assert.deepEqual(
      lane.label.map((r) => [r.color, r.bold]),
      [[own[0].runs[0].color, own[0].runs[0].bold]],
    );
    assert.deepEqual(
      lane.lines.map((l) => text(l.runs)),
      own.map((l) => text(l.runs).slice(lane.name.length + 3)),
    );
  }
});

test("lanes: the root waits until it prints; the badge and the skip stamp come on their cues and then rest", (t) => {
  const f = needFacts(t);
  if (!f) return;
  const { lines } = c11(f);
  const timed = lines.map((l, k) => ({ runs: l.runs, at: 0.5 + 0.5 * k }));
  const ciFirst = timed.find((l) => text(l.runs).startsWith("[ci] "))!.at;
  const before = draw((ctx) =>
    lanes(ctx, { lines: timed, b: ciFirst - 0.01, badgeAt: 10 }),
  );
  assert.ok(
    !before.inked.some((s) => s.startsWith("$ echo")),
    "ci printed early",
  );
  assert.ok(!before.inked.includes(LANES_BADGE));
  const done = draw((ctx) => lanes(ctx, { lines: timed, b: 12, badgeAt: 10 }));
  for (const l of timed)
    assert.ok(done.inked.includes(text(l.runs).replace(/^\[\w+\] /, "")));
  assert.ok(done.inked.includes(LANES_BADGE));
  assert.deepEqual(
    done.ops,
    draw((ctx) => lanes(ctx, { lines: timed, b: 20, badgeAt: 10 })).ops,
  );
  // skip: the hero's stamp says "skipped" from its beat, and nothing else is stamped.
  const hero = { task: "build", at: 1, stampAt: 2 };
  const pre = draw((ctx) => lanes(ctx, { lines: timed, b: 1.9, hero }));
  const post = draw((ctx) => lanes(ctx, { lines: timed, b: 2.1, hero }));
  assert.ok(!pre.inked.includes("skipped"));
  assert.equal(post.inked.filter((s) => s === "skipped").length, 1);
});

// The ledger and the CI panel.

test("ledger: only node opens; each other tool folds to what its lock records", (t) => {
  const f = needFacts(t);
  if (!f) return;
  const lock = fileOf(f, "C17", "work/api/mise.lock");
  assert.ok(lock, "C17 left no mise.lock");
  const entries = lockEntries(lock);
  const lines = lock.split("\n").map((l) => l.trim());
  for (const e of entries) {
    const at = lines.indexOf(e.head);
    const next = lines.findIndex((l, i) => i > at && /^\[\[tools\./.test(l));
    const body = lines.slice(at + 1, next < 0 ? undefined : next);
    assert.equal(
      e.provenance,
      body.some((l) => /^provenance\s*=/.test(l)),
      `${e.name} provenance`,
    );
    assert.equal(
      e.signer,
      body.some((l) => /^signer\s*=/.test(l)),
      `${e.name} signer`,
    );
    assert.equal(
      e.aube,
      body.some((l) => /^aube\s*=/.test(l)),
      `${e.name} aube`,
    );
    assert.equal(
      e.platforms.length,
      body.filter((l) => /^\[tools\..+\."platforms\./.test(l)).length,
    );
  }
  const node = entries.find((e) => e.name === "node")!;
  assert.ok(
    node.version?.includes(`"${f.versions.node.lts_version}"`),
    node.version ?? "",
  );
  const rows = ledgerRows(entries, "node");
  assert.ok(
    rows.every((r) => r.kind === "head" || r.entry === "node"),
    "another tool opened",
  );
  const sums = rows.filter((r) => r.kind === "sum");
  assert.equal(sums.length, Math.min(4, node.platforms.length));
  for (const s of sums)
    assert.ok(
      s.kind === "sum" && lock.includes(`"${s.checksum}"`),
      "a checksum the lock lacks",
    );
  const more = rows.find((r) => r.kind === "more");
  if (node.platforms.length > 4)
    assert.ok(
      more &&
        more.kind === "more" &&
        more.text === `${node.platforms.length - 4} more platforms`,
    );
  // The drawing: the recorded version and specifiers, and a provenance
  // badge for each entry that records it and no other.
  const drawn = draw((ctx) =>
    ledger(ctx, {
      lock,
      toml: fileOf(f, "C17", "work/api/mise.toml"),
      b: 5,
      enter: 0,
      unfold: 1,
      thread: 2,
    }),
  );
  assert.ok(drawn.inked.includes(node.version!));
  assert.ok(drawn.inked.includes(node.specifiers!));
  assert.equal(
    drawn.inked.filter((s) => s === "provenance").length,
    entries.filter((e) => e.provenance).length,
  );
});

test("ledger: made-up versions draw as themselves; the chip docks where it is sent and rests", () => {
  const lock = [
    "[[tools.node]]",
    'version = "31.4.1"',
    'specifiers = ["31"]',
    "",
    '[tools.node."platforms.linux-x64"]',
    'checksum = "sha256:abc"',
    "",
    '[[tools."npm:prettier"]]',
    'aube = { path = "x", digest = "sha256:def" }',
  ].join("\n");
  const toml = '[tools]\nnode = "31"\n';
  const dock = { x: 1111, y: 555 };
  const o = { lock, toml, b: 3, enter: 0, unfold: 0.5, thread: 1 };
  const drawn = draw((ctx) => ledger(ctx, o));
  assert.ok(drawn.inked.includes('version = "31.4.1"'));
  assert.ok(drawn.inked.includes('node = "31"'));
  assert.ok(!drawn.inked.includes("provenance"));
  let chip = null as ReturnType<typeof ledger>["chip"];
  const end = draw((ctx) => {
    chip = ledger(ctx, { ...o, b: 10, chip: { at: 4, dock } }).chip;
  });
  assert.ok(chip);
  const c = chip as { x: number; y: number; w: number; h: number };
  assert.equal(c.x, dock.x);
  assert.equal(c.y + c.h / 2, dock.y);
  assert.deepEqual(c.w, lockChipSize().w);
  assert.deepEqual(
    end.ops,
    draw((ctx) => ledger(ctx, { ...o, b: 30, chip: { at: 4, dock } })).ops,
  );
});

test("CI panel: the fixture's lines verbatim, inside the panel, lit on their cues; the dock is beside its row", (t) => {
  const f = needFacts(t);
  if (!f) return;
  const yml = fixture(f, "api/.github/workflows/ci.yml");
  assert.ok(yml, "no CI workflow fixture");
  const rows = ciRows(yml);
  assert.deepEqual(
    rows.map((r) => r.text),
    yml.replace(/\s+$/, "").split("\n"),
  );
  for (const r of rows) assert.ok(r.top >= 120 && r.top + r.h <= 120 + 570);
  const lights = [
    { match: /install_args: --locked/, at: 2 },
    { match: /run: mise run ci/, at: 4.5 },
  ];
  const drawn = draw((ctx) =>
    ciPanel(ctx, { yml, b: 8, enter: 0, push: 1, lights }),
  );
  for (const l of rows)
    assert.ok(drawn.inked.includes(l.text) || !l.text.trim());
  const staged = ciPanel(recorder().ctx, {
    yml,
    b: 8,
    enter: 0,
    push: 1,
    lights,
  });
  const dock = ciDock(staged, /install_args: --locked/)!;
  const row = staged.find((r) => /install_args/.test(r.text))!;
  assert.ok(
    dock.x > row.end && Math.abs(dock.y - (row.top + row.h / 2)) < 1e-9,
  );
  // The push-in lasts six beats, then the panel is at rest.
  assert.deepEqual(
    draw((ctx) => ciPanel(ctx, { yml, b: 7, enter: 0, push: 1, lights })).ops,
    draw((ctx) => ciPanel(ctx, { yml, b: 9, enter: 0, push: 1, lights })).ops,
  );
});

// Tiles.

test("tiles: pending, started, done; a step the shot leaves running never ticks; no Watcher without systemd", () => {
  const tools = { label: "Tools", appear: 4, start: 4 };
  assert.equal(tileState(tools, 3.9), "hidden");
  assert.equal(tileState(tools, 4), "started");
  assert.equal(tileState(tools, 99), "started");
  const dot = { label: "Dotfiles", appear: 1, done: 2 };
  assert.deepEqual(
    [0, 1, 1.9, 2].map((b) => tileState(dot, b)),
    ["hidden", "pending", "pending", "done"],
  );
  assert.deepEqual(bootstrapLabels("systemd"), [
    "Dotfiles",
    "Config",
    "Watcher",
    "Tools",
  ]);
  assert.deepEqual(bootstrapLabels("plain"), ["Dotfiles", "Config", "Tools"]);
  const items = [dot, { label: "Config", appear: 1, done: 2 }, tools];
  const a = draw((ctx) => tiles(ctx, { tiles: items, b: 6 }));
  assert.deepEqual(a.inked, ["Dotfiles", "Config", "Tools"]);
  assert.deepEqual(
    a.ops,
    draw((ctx) => tiles(ctx, { tiles: items, b: 60 })).ops,
  );
});

// The slip and the drawn link.

/** A packslip statement's predicate, as far as the slip test reads it. */
interface RawPredicate {
  resources: { kind: string; shell?: string; name?: string }[];
  identity: { scheme: string; key_id: string };
}

test("slip: hk's real packslip keys and values, abridged, for each version; the version stamps on the flip", (t) => {
  const f = needFacts(t);
  if (!f) return;
  const { hk_old: before, hk_new: after } = f.versions.tools;
  for (const v of [before, after]) {
    const file: string = fileOf(f, "C8", `offcam/packslip-hk-${v}.json`)!;
    const raw: RawPredicate = (JSON.parse(file) as { predicate: RawPredicate })
      .predicate;
    const p: Packslip = packslipOf(f, "hk", v)!;
    assert.equal(p.version, v);
    assert.deepEqual(
      p.completions,
      raw.resources.filter((r) => r.kind === "completion").map((r) => r.shell),
    );
    assert.deepEqual(
      p.skills,
      raw.resources.filter((r) => r.kind === "skill").map((r) => r.name),
    );
    const rows = slipRows(p);
    assert.deepEqual(
      [...new Set(rows.map((r) => r.key).filter(Boolean))],
      ["version", "resources", "identity"],
    );
    // Each abridged value is the statement's, less what `…` stands for.
    const id = rows.find((r) => r.key === "identity")!.values;
    assert.equal(id[0], raw.identity.scheme);
    for (const part of id[1].split("…"))
      assert.ok(raw.identity.key_id.includes(part.replace(/^\/|\/$/g, "")));
  }
  const o = { d: f, before, after, flip: 5, b: 3, enter: 0 };
  const pre = draw((ctx) => slip(ctx, o));
  assert.ok(pre.inked.includes(SLIP_BADGE));
  assert.ok(pre.inked.includes(before) && !pre.inked.includes(after));
  const post = draw((ctx) => slip(ctx, { ...o, b: 7 }));
  assert.ok(post.inked.includes(after) && !post.inked.includes(before));
  assert.deepEqual(post.ops, draw((ctx) => slip(ctx, { ...o, b: 9 })).ops);
  assert.equal(
    abridgeKey("https://github.com/a/b/.github/workflows/r.yml@refs/tags/x"),
    "a/b/…/r.yml",
  );
});

test("drawn link: the take's wrapped linked rows, and only the active version lit", (t) => {
  const f = needFacts(t);
  if (!f) return;
  const c = capture(f, "C8")!;
  const sc = screenAt(c, stepOf(c, "sync").end);
  const links = linkedRows(sc.lines);
  assert.ok(links.length >= 2, `${links.length} links`);
  const v = f.versions.tools.hk_new;
  for (const l of links) {
    assert.match(projectPath(l.from), /^\.claude\/skills\/[\w-]+$/);
    const p = installPath(l.to, v);
    assert.equal(p.text, `…/installs/hk/${v}/…`);
    assert.deepEqual(
      Array.from(p.text)
        .slice(...p.lit!)
        .join(""),
      v,
    );
    assert.equal(installPath(l.to, "0.0.0-not-there").lit, null);
  }
  // A row wrapped at 80 columns reads as one.
  const head = "linked /x/.claude/skills/s -> /y/";
  const wrapped = linkedRows([
    head + "z".repeat(80 - head.length),
    "/installs/t/9/skills/s",
  ]);
  assert.equal(wrapped.length, 1);
  assert.ok(wrapped[0].to.endsWith("/installs/t/9/skills/s"));
  const o = { links, active: v, b: 4, enter: 0, draw: 0.5 };
  const drawn = draw((ctx) => skillsLink(ctx, o));
  for (const l of links) assert.ok(drawn.inked.includes(projectPath(l.from)));
  assert.ok(drawn.inked.includes(v));
  assert.deepEqual(
    drawn.ops,
    draw((ctx) => skillsLink(ctx, { ...o, b: 40 })).ops,
  );
});

// Folders.

test("folders: each folder's own node line; the dot follows the cwd; each version lands on its folder", (t) => {
  const f = needFacts(t);
  if (!f) return;
  const v = f.versions.node;
  const folderList = [
    {
      name: "api/",
      toml: fileOf(f, "C7", "work/api/mise.toml"),
      version: {
        runs: [run(`v${v.lts_version}`)],
        at: 4,
        from: { x: 188, y: 400, size: 26 },
      },
    },
    {
      name: "dashboard/",
      toml: fileOf(f, "C7", "work/dashboard/mise.toml"),
      version: { runs: [run(`v${v.other_version}`)], at: 2 },
    },
  ];
  const cds = [
    { at: 1, to: 1 },
    { at: 3, to: 0 },
  ];
  const o = { folders: folderList, cwd: 0, cds, root: "~/work/" };
  assert.deepEqual(cwdState({ ...o, b: 0 }).current, [1, 0]);
  assert.deepEqual(cwdState({ ...o, b: 2 }).current, [0, 1]);
  assert.deepEqual(cwdState({ ...o, b: 9 }).current, [1, 0]);
  const rest = draw((ctx) => folders(ctx, { ...o, b: 0 }));
  for (const fl of folderList) {
    const line = fl.toml!.split("\n").find((l) => /^node\s*=/.test(l))!;
    assert.ok(rest.inked.includes(line), line);
  }
  assert.ok(
    !rest.inked.some((s) => s.startsWith("v")),
    "a version before it printed",
  );
  let out: ReturnType<typeof folders> | null = null;
  const poster = draw((ctx) => {
    out = folders(ctx, { ...o, b: 9 });
  });
  assert.ok(
    poster.inked.includes(`v${v.lts_version}`) &&
      poster.inked.includes(`v${v.other_version}`),
  );
  const chips = (out as unknown as ReturnType<typeof folders>).chips;
  chips.forEach((c, i) => {
    assert.ok(c, `folder ${i} has no chip`);
    const card = (out as unknown as ReturnType<typeof folders>).cards[i];
    assert.ok(
      c.x >= card.x && c.y + c.h <= card.y + card.h,
      "a chip off its folder",
    );
  });
  assert.deepEqual(
    poster.ops,
    draw((ctx) => folders(ctx, { ...o, b: 30 })).ops,
  );
});
