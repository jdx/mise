// clone (STORYBOARD.md Act VII; ART.md §12.2, §12.10; STORYBOARD.md
// "Pacing"): the reel's payoff, one focal motion at a time, half a beat
// apart (kit/pace.ts Pace), whatever is not the focus dimmed. In the same
// shell the breath cleared, never activated, `git clone
// https://github.com/you/api && cd api && mise run ci` (C19) types at
// TYPE_RATE, `you/api` badged as an illustration from its first `you/`.
// The line runs past the pane's 56 columns, so before Enter its end, `mise
// run ci`, scales up from its own cells at the pane's edge (move 1/2,
// never past its 120 px), holds at full size (rule 7) and leaves; then
// Enter. Git's real lines print, dimmed to texture; after half a beat
// mise's install rows for the lock's tools move at 3× under the time-lapse
// badge, the two the reel installed earlier (npm:prettier, hk) banded in
// the tools pink. The rows fade while they still run (cut before any
// rate), and the window holds the command it ran, dimmed, under the badge
// for the time-lapse's 1.5 s; in that tail, once the rows are gone, the
// two tools the file asks for as "latest" (jq and npm:prettier) lift their
// versions from where their rows stood as value chips, round under the
// card's rows and up beside their own: what the lock pins. [tools] lights,
// and caption 1 follows, the window holding its command, dimmed, until
// the lanes take its column. The run's task lines come as lanes, in the
// take's order (not to scale, and badged so); ci's waits for the others;
// they hold to be read; then the lanes fly into the card's task header, taking its
// band's colour, and it lights as they land; then the card folds to its
// three headers. With the bell, rung twice, the take's `[ci] api ready:
// node v…, APP_ENV=api` lands as big type in its own colours, two lines at
// 88 px, centred, one on each bell, the node version underlined in the
// tools pink; then `APP_ENV=api` is underlined in the env gold as [env]
// lights: every table lit, each as it was used. Caption 2 follows; the
// line holds, slowly pushing in, with a glow behind it, while it is read;
// then it goes home: `[ci]`, the node version and `APP_ENV=api` each fly
// to the header of the table they came from, round the card's rows, and
// fade into it at its edge as it ignites; the rest fades where it stands.
// The card, its three headers lit, holds still into the morph (kit/rest.ts
// clone|morph).

import { type LitRect, PALETTE, PILLAR, sec, TERM } from "../bible";
import {
  type Capture,
  capture,
  CUT_ON,
  fileOf,
  firstWith,
  lastBefore,
  type ReelData,
  reelData,
  screenAt,
  stepOf,
} from "../captures";
import { glow } from "../fx";
import {
  type LaneLine,
  laneModel,
  lanes,
  settleHandover,
  taskLines,
} from "../kit/diagrams/lanes";
import {
  arc,
  bump,
  curveAt,
  enter,
  exit,
  lerpRect,
  type Pt,
  type Rect,
  slam,
  span,
} from "../kit/motion";
import {
  along,
  beatOf,
  bigRuns,
  CARD,
  card,
  type CardRows,
  clipTime,
  cut,
  type G,
  greyScene,
  keep,
  LAPSE,
  LEFT,
  missing,
  paneWindow,
  type Play,
  play,
  playsEnd,
  rectOf,
  rowRunsMatch,
  shot,
  splitRuns,
  step,
  underline,
} from "../kit/grey";
import {
  type FocusPlan,
  GAP,
  lapseUp,
  Pace,
  readHold,
  restOut,
  unfocus,
} from "../kit/pace";
import { CLONE_OPEN, COMPACT, dimWraps, OPENS } from "../kit/rest";
import {
  CARD_ART,
  CLIMAX,
  DUR,
  EASE,
  GLOW,
  LAYOUT,
  MOTION,
  PILLAR_BAND,
  STROKE,
  TYPE,
} from "../kit/style";
import {
  advance,
  drawTermLine,
  lineText,
  type Run,
  termLayout,
  termLit,
} from "../kit/term";
import { lerp, progress } from "../math";
import type { SceneCues } from "../score/cues";
import {
  focusPlan,
  inFocus,
  moveOr,
  perFacts,
  takeIn,
  termAct,
} from "./g5-dotfiles-lock/pacing";
import { liftValue, scrollOfTake, type ValueLift } from "./g6-machine/shot";

// The picture's beats: the section's focal motions one at a time
// (kit/pace.ts Pace), from the take's own times.

/**
 * `mise run ci` in big type: it scales up from its cells (move 1/2), holds
 * at full size (rule 7), and rises and fades (DUR.exit), all inside the
 * pause before Enter, half a beat clear of the typing and of Enter.
 */
const LIFT_UP = DUR.half;
const LIFT_DUR = LIFT_UP + DUR.bigHold + DUR.exit;
const ENTER_WAIT = GAP + LIFT_DUR + GAP;
/** The lanes: one line every LANE_GAP, each entering over DUR.enter. */
const LANE_GAP = 5 / 16;
/**
 * The lanes hold, whole, this long before they fly into [tasks] (rule 3):
 * 2.5 s, which their lines, printing one at a time from the first, have
 * had from it and more (readHold of six lines is 3.5 s).
 */
const LANES_READ = readHold(4);
/** Git's lines, texture under the command (the payoff is mise's): dimmed as daemons dims Postgres's log. */
const GIT_DIM = dimWraps(
  0.25,
  /^(Cloning into |remote: |Receiving objects|Resolving deltas|Unpacking objects)/,
);
const onGrid = (b: number): number => Math.ceil(b * 16 - 1e-6) / 16;
/** The pieces of the `[ci]` line leave for home this long apart ([ci] first), and land in reading order. */
const HOME_LAG = DUR.flick;
/** Home: the last piece lands (DUR.move after it leaves, and a sixteenth after the one before) and its header ignites. */
const HOME_DUR = HOME_LAG + DUR.move + DUR.stagger + DUR.ignite;
/** The height the chips run along under the card's rows, and the gap they keep right of the longest row. */
const CHIP_VIA_Y = 648;
const CHIP_GAP = 30;
/** A value chip's lift: it forms and rises (DUR.flick), flies (DUR.lift) and lands with a pop (DUR.tick). */
const CHIP_LIFT = DUR.flick + DUR.lift + DUR.tick;

/** A tool whose version lifts onto the card: its row's name, its version's runs and column, and when its chip forms. */
interface Pinned {
  tool: string;
  runs: Run[];
  col: number;
  /** The take time its chip forms on (its row shows), and the beat. */
  ct: number;
  at: number;
}

/** When everything happens, and the take's frames the shot, the chips and the lanes read. */
interface Take {
  p: Pace;
  plays: Play[];
  focus: FocusPlan;
  /** The typed line's last key, and the frame with every key typed. */
  lastKey: number;
  typed: number;
  /** `mise run ci` scales up from its cells, lands at full size, leaves; Enter. */
  lift: number;
  land: number;
  holdEnd: number;
  release: number;
  /** Where the rows cut (the plays' end), and where the fast play starts: the time-lapse badge. */
  cut: number;
  lapse: number;
  /** The time-lapse's tail ends: its badge goes. */
  tail: number;
  /** The window leaves with its text from here, as the lanes take its column. */
  out: number;
  /** The beat the tool rows appear. */
  rows: number;
  /** [tools] lights. */
  toolsLit: number;
  /** git's first line. */
  git: number;
  /** The first lane's beat, and the task lines on the lanes' beats. */
  lane0: number;
  lines: LaneLine[];
  /** The lanes fly into the card's task header, and [tasks] lights as the first hands over. */
  settle: number;
  tasksLit: number;
  /** The card folds to its three headers; the pinned chips leave with its tables. */
  fold: number;
  chipsOut: number;
  /** The `[ci]` line's first line lands with the bell, its second on the second; the node version's underline. */
  ready: number;
  line2: number;
  underTools: number;
  /** `APP_ENV=api` is underlined and [env] lights. */
  envLit: number;
  /** The slow push-in, from caption 2's landing to the line going home. */
  push: readonly [number, number];
  /** The pieces leave for home, and land. */
  home: readonly [number, number, number];
  homeLand: readonly [number, number, number];
  /** The `[ci]` line as the take printed it. */
  readyRuns: Run[] | null;
  /** The versions the lock pins for the tools the file asks for as a non-version ("latest"). */
  pinned: Pinned[];
}

/**
 * The section's schedule: a pure function of the facts (the take and the
 * cloned project's `api/mise.toml` it carries), worked out once per facts.
 * Without the take, the storyboard's plan.
 */
const takeOf = perFacts((d: ReelData | null): Take => {
  const c = takeIn(d, "C19");
  const toml = c ? fileOf(d, "C19", "api/mise.toml") : null;
  const p = new Pace("clone", { d, from: 0 });
  const ci = c ? stepOf(c, "ci") : null;
  // Git's lines, up to the frame before mise's install header: that and
  // its rows only ever move, under the time-lapse badge.
  const header = c && ci ? lastBefore(c, /^mise by @jdx/, ci.enter, ci.end) : 0;
  const type = termAct(
    p,
    "type",
    c,
    (c, at) => [step(c, "ci", at, { until: header, pause: ENTER_WAIT })],
    { lines: 0 },
  );
  const release = type.enter;
  const lift = release - GAP - LIFT_DUR;
  const land = lift + LIFT_UP;
  const holdEnd = land + DUR.bigHold;
  // The rows at LAPSE, half a beat after git's lines, cut before any rate.
  const rowsAt = p.next();
  const stop = c && ci ? lastBefore(c, CUT_ON, ci.enter, ci.end) : 0;
  const rowsPlay = c ? [play(rowsAt, header, stop, LAPSE)] : [];
  const plays: Play[] = c
    ? [cut(0, OPENS.C19!(c)), ...type.plays, ...rowsPlay]
    : [];
  const cutAt = c ? playsEnd(plays) : rowsAt;
  const rowsT = c && ci ? firstWith(c, /^ \S+@/, ci.enter) : null;
  const rowsBeat = rowsT === null ? rowsAt : beatOf(plays, rowsT);
  // The chips form once the rows are gone, in the time-lapse's tail.
  const pinned =
    c && rowsT !== null ? pinnedOf(c, rowsT, stop, toml, cutAt + DUR.tick) : [];
  const chipsIn = pinned.length ? pinned[pinned.length - 1].at + CHIP_LIFT : 0;
  // The rows, the command held under the badge for the time-lapse's 1.5 s,
  // and in that tail the chips onto the card.
  const rows = moveOr(
    p,
    c,
    "rows",
    Math.max(cutAt - rowsAt, lapseUp(), chipsIn - rowsAt),
  );
  const toolsLit = moveOr(p, c, "toolsLit", DUR.light);
  p.caption(0);
  // The window gives its column to the lanes (DUR.tick), then the lanes
  // come, one line every LANE_GAP.
  const lane0 = p.next();
  const first = lane0 + DUR.tick;
  const lines =
    c && ci
      ? taskLines(c, {
          after: ci.enter,
          until: lastBefore(c, /Finished in/, ci.enter, ci.end),
        }).map((l, k): LaneLine => ({ runs: l.runs, at: first + k * LANE_GAP }))
      : [];
  const lanesDur =
    DUR.tick + Math.max(0, lines.length - 1) * LANE_GAP + DUR.enter;
  const lanesS = moveOr(p, c, "lanes", lanesDur);
  // They hold to be read, then fly into [tasks] (a sixteenth apart,
  // DUR.move each), which lights as the first hands over.
  const settleAt = p.next(lanesS.end + LANES_READ);
  const tasksLitAt = onGrid(settleHandover(settleAt, 0));
  const nLanes = laneModel(lines).length || 4;
  moveOr(
    p,
    c,
    "tasksLit",
    Math.max(
      (nLanes - 1) * DUR.stagger + DUR.move,
      tasksLitAt + DUR.light - settleAt,
    ),
    { after: settleAt },
  );
  const fold = moveOr(p, c, "fold", Math.max(DUR.move, DUR.fold));
  const payoff = moveOr(p, c, "payoff", DUR.short + DUR.flick + DUR.half);
  const ready = payoff.at + DUR.short;
  const env = moveOr(p, c, "envLit", DUR.half);
  const c2 = p.caption(1);
  // Home once caption 2 is read, if the section has the room for it and
  // its rest; else as late as it has.
  const beats = sec("clone").beats;
  const latest = beats - restOut() - HOME_DUR;
  const homeAt = Math.max(p.next(), Math.min(c2.out, latest));
  const home = moveOr(p, c, "home", HOME_DUR, { after: homeAt });
  const homes = [home.at, home.at + HOME_LAG, home.at + HOME_LAG] as const;
  const lastKey =
    c && ci
      ? beatOf(
          plays,
          c.keys.filter(([t]) => t < ci.enter).at(-1)?.[0] ?? ci.enter,
        )
      : type.at;
  const gitT = c && ci ? firstWith(c, /^Cloning into/, ci.enter) : null;
  return {
    p,
    plays,
    lastKey,
    typed: ci ? ci.enter - 1e-3 : 0,
    lift,
    land,
    holdEnd,
    release,
    cut: cutAt,
    lapse: rowsAt,
    tail: rows.end,
    out: lane0,
    rows: rowsBeat,
    toolsLit: toolsLit.at,
    git: gitT === null ? release : beatOf(plays, gitT),
    lane0,
    lines,
    settle: settleAt,
    tasksLit: tasksLitAt,
    fold: fold.at,
    chipsOut: fold.at,
    ready,
    line2: ready + DUR.flick,
    underTools: ready + DUR.flick,
    envLit: env.at,
    push: [c2.end, home.at],
    home: homes,
    homeLand: [
      homes[0] + DUR.move,
      homes[1] + DUR.move,
      homes[1] + DUR.move + DUR.stagger,
    ],
    readyRuns:
      c && ci
        ? rowRunsMatch(
            c,
            lastBefore(c, /Finished in/, ci.enter, ci.end),
            /^\[ci\] api ready: /,
          )
        : null,
    pinned,
    focus: focusPlan(
      [
        [type.at, "term"],
        [rows.at, "card"],
        [lane0, "lanes"],
        [settleAt, "card"],
        [payoff.at, "big"],
        [env.at, ["big", "card"]],
        [home.at, "*"],
      ],
      [[type.at, rows.end]],
    ),
  };
});

/** A TOML row's key, quotes dropped (`"npm:prettier" = …` is npm:prettier), or null. */
const keyOf = (row: string): string | null =>
  /^"?([^"=\s]+)"?\s*=/.exec(row)?.[1] ?? null;

/** The tools a mise.toml's [tools] asks for by something other than a version (`jq = "latest"`), by name. */
function looseTools(text: string | null): Set<string> {
  const out = new Set<string>();
  let inTools = false;
  for (const line of (text ?? "").split("\n")) {
    if (/^\[/.test(line)) inTools = line.trim() === "[tools]";
    const key = inTools ? keyOf(line) : null;
    const value = /=\s*"([^"]*)"/.exec(line)?.[1];
    if (key && value !== undefined && !/^\d/.test(value)) out.add(key);
  }
  return out;
}

/**
 * The install rows whose versions lift onto the card (M6): each tool the
 * file asks for loosely, in the order their rows stand, the upper first,
 * each chip forming from beat `from` (once the rows are gone), a sixteenth
 * after the one before, where its row last stood, so the upper one is past
 * the lower's place by the time that lands.
 */
function pinnedOf(
  c: Capture,
  rowsT: number,
  stop: number,
  toml: string | null,
  from: number,
): Pinned[] {
  const loose = looseTools(toml);
  const sc = screenAt(c, rowsT);
  const out: Pinned[] = [];
  for (const line of sc.lines) {
    const m = /^ (\S+)@(\S+)/.exec(lineText(line));
    if (!m || !loose.has(m[1])) continue;
    const col = 1 + Array.from(m[1]).length + 1;
    const runs = splitRuns(splitRuns(line, col)[1], Array.from(m[2]).length)[0];
    const at = from + out.length * DUR.stagger;
    // Where its row last stood before the cut: the chip forms there.
    const shows = (t: number) =>
      screenAt(c, t).lines.some((l) => lineText(l).startsWith(` ${m[1]}@`));
    let ct = rowsT;
    for (const f of c.frames)
      if (f.t >= rowsT && f.t <= stop && shows(f.t)) ct = f.t;
    if (!shows(ct)) continue;
    out.push({ tool: m[1], runs, col, ct, at });
  }
  return out;
}

/** The window: up, holding the command it ran after the cut, then gone with its text (fall and fade, quickly), for good. */
const windowOf = (x: Take, b: number) => exit(b, x.out, DUR.tick);

/** The install rows' fade: out over the plays' last 1/8 beat, while they still move (kit/grey.ts LapseTail). */
const rowsAlpha = (x: Take, b: number): number =>
  1 - EASE.glide(progress(x.cut - DUR.flick, x.cut, b));

/** Each lane's entrance: the beat its first line prints. */
function laneAt(x: Take, name: string): number | null {
  return laneModel(x.lines).find((l) => l.name === name)?.first ?? null;
}

export const cues: SceneCues<"clone"> = (facts) => {
  const d = reelData(facts);
  if (!capture(d, "C19")) return {};
  const x = takeOf(d);
  return {
    slam: x.land,
    git: x.git,
    tools: x.toolsLit,
    test: laneAt(x, "test"),
    lint: laneAt(x, "lint"),
    build: laneAt(x, "build"),
    ci: laneAt(x, "ci"),
    tasks: x.tasksLit,
    ready: x.ready,
    env: x.envLit,
    fold: x.fold,
    home: [...x.homeLand],
  };
};

/** Runs set in big type's weight (ART.md §4: mono 700), colours as captured. */
const heavy = (runs: readonly Run[]): Run[] =>
  runs.map((r) => ({ ...r, bold: true }));

const runsWidth = (runs: readonly Run[], size: number): number =>
  runs.reduce((w, r) => w + Array.from(r.text).length, 0) * advance(size);

/**
 * `mise run ci`, scaled up from its cells at the pane's edge into big type
 * (ART.md §12.10): place and size together on `move` over the half beat
 * before it lands, along a shallow arc to LAYOUT.bigX/bigY, growing to
 * TYPE.big.kinetic and never past it (no slam squash: nothing overshoots
 * between two rests, ART.md §13), with the slam's impact light on the
 * downbeat. The copy is clipped to the pane's body, so it comes out of the
 * pane's own edge, where its cells stand past the 56th column, and never
 * over the card; its cells there give way under it as it comes up. It
 * holds, then rises 16 px and fades (leave, glide) before Enter.
 */
function liftCommand(g: G, c: Capture, x: Take): void {
  const { ctx, b } = g;
  const from = Math.max(x.lift, x.lastKey);
  if (b < from || b >= x.release) return;
  const sc = screenAt(c, x.typed);
  const line = sc.lines[0];
  if (!line) return;
  const text = lineText(line);
  const col = text.search(/mise run ci\s*$/);
  if (col < 0) return;
  const runs = heavy(splitRuns(line, col)[1]).map((r) => ({
    ...r,
    text: r.text.trimEnd(),
  }));
  const L = termLayout(LEFT, sc.lines.length);
  const cell = L.cell(0, col);
  const size0 = LEFT.size;
  const size1 = TYPE.big.kinetic;
  const a = {
    x: cell.x + runsWidth(runs, size0) / 2,
    y: L.baseline(0) - 0.36 * size0,
  };
  const z = { x: LAYOUT.bigX, y: LAYOUT.bigY };
  const u = span(b, from, x.land - from, "move");
  const p = curveAt(arc(a, z, -MOTION.arcLift / 2), u);
  const size = lerp(size0, size1, u);
  const impact = slam(b, x.land).impact;
  const gone = exit(b, x.holdEnd);
  // Up to full over its first 1/8 beat: its cells in the pane are faded
  // at the pane's edge, so a full-strength copy would pop in over them.
  const alpha = span(b, from, DUR.flick, "arrive") * gone.alpha;
  if (alpha <= 0) return;
  ctx.save();
  if (impact > 0)
    glow(
      ctx,
      z.x,
      z.y,
      GLOW.slam.radius,
      PALETTE.paper,
      GLOW.slam.alpha * impact,
    );
  const body = L.body;
  ctx.beginPath();
  ctx.rect(body.x, body.y, body.w, body.h);
  ctx.clip();
  // Its own cells give way under it, at its alpha: the window's body over
  // them from their first column to the pane's edge, so the growing copy
  // never doubles its faded glyphs there, and they come back as it leaves.
  ctx.save();
  ctx.globalAlpha *= alpha;
  ctx.fillStyle = TERM.window;
  ctx.fillRect(cell.x, cell.y, body.x + body.w - cell.x, cell.h);
  ctx.restore();
  const cy = p.y - gone.dy;
  bigRuns(ctx, runs, p.x, cy + 0.36 * size, size, alpha);
  ctx.restore();
}

/**
 * npm:prettier and hk, installed earlier in the reel: their rows banded in
 * the tools pink as they move, fading with the rows (rowsAlpha).
 */
function bandRows(g: G, c: Capture, x: Take): void {
  const { ctx, b } = g;
  if (b < x.rows || b >= x.cut) return;
  const { ct } = clipTime(x.plays, b);
  const sc = screenAt(c, ct);
  const L = termLayout(LEFT, sc.lines.length, scrollOfTake(c, ct, LEFT));
  const k = EASE.arrive(progress(x.rows, x.rows + DUR.tick, b));
  ctx.save();
  ctx.globalAlpha *= rowsAlpha(x, b);
  sc.lines.forEach((l, i) => {
    if (!/^ (npm:prettier|hk)@/.test(lineText(l))) return;
    const cell = L.cell(i, 0);
    const r = { x: LEFT.x + 12, y: cell.y + 3, w: LEFT.w - 24, h: cell.h - 6 };
    ctx.fillStyle = PILLAR_BAND.tools;
    ctx.fillRect(r.x, r.y, r.w * k, r.h);
    ctx.fillStyle = PILLAR.tools;
    const bh = r.h * k;
    ctx.fillRect(r.x, r.y + (r.h - bh) / 2, STROKE.headerBar, bh);
  });
  ctx.restore();
}

/**
 * The versions the lock pins for the tools the file asks for loosely,
 * lifted off their install rows (ART.md §9) onto the card beside the rows
 * that asked for them: `jq = "latest"` gets the version jq resolved to,
 * the one the lock pins. Each chip runs down
 * under its row (the pane's rows below it are fading), along under the
 * card's rows through its empty lower part, and up a column right of the
 * longest row, so it crosses no text, landing beside its own row (M6).
 * They leave before the card's tables fold.
 */
function pinnedLifts(c: Capture, x: Take, rows: CardRows): ValueLift[] {
  const bodies = rows.filter((r) => r.kind === "body");
  const right = Math.max(...bodies.map((r) => r.x + (r.w ?? 0)));
  // As large as a lifted value is set (TYPE.chip.lift, to read on a
  // phone), or as large as the widest chip fits between the column and
  // the card's right inset: a chip n characters wide at size s is
  // (0.6 n + 1) s (kit/diagrams/common.ts chipRect).
  const chars = Math.max(
    1,
    ...x.pinned.map(
      (p) => Array.from(p.runs.map((r) => r.text).join("")).length,
    ),
  );
  const room = CARD.x + CARD.w - CARD_ART.inset - (right + CHIP_GAP);
  const size = Math.max(
    TYPE.chip.value,
    Math.min(TYPE.chip.lift, Math.floor(room / (advance(1) * chars + 1))),
  );
  const colX = right + CHIP_GAP + size / 2;
  return x.pinned.flatMap((p): ValueLift[] => {
    const row = bodies.find((r) => keyOf(r.text) === p.tool);
    if (!row) return [];
    const sc = screenAt(c, p.ct);
    const i = sc.lines.findIndex((l) => lineText(l).startsWith(` ${p.tool}@`));
    if (i < 0) return [];
    const L = termLayout(LEFT, sc.lines.length, scrollOfTake(c, p.ct, LEFT));
    const from = { x: L.col(p.col), y: L.baseline(i), size: LEFT.size };
    const to = { x: colX, y: row.y, size };
    const dip = Math.max(0, CHIP_VIA_Y - from.y);
    const via: Pt[] = [
      { x: from.x + dip + 24, y: CHIP_VIA_Y },
      { x: colX, y: CHIP_VIA_Y },
    ];
    return [{ runs: p.runs, from, to, at: p.at, out: x.chipsOut, via }];
  });
}

/** The card's lights, each as its table is used: [tools] by the install, [tasks] by the lanes, [env] by `APP_ENV=api`. */
function lights(x: Take, b: number) {
  const tools = x.toolsLit;
  return {
    lit: {
      "[tools]": along(b, tools, tools + DUR.light),
      "[env]": along(b, x.envLit, x.envLit + DUR.light),
      "[tasks": along(b, x.tasksLit, x.tasksLit + DUR.light),
    },
    // Each ignites as it lights, and again as its piece of the `[ci]` line comes home.
    ignite: {
      "[tools]":
        bump(b, tools, DUR.ignite) + bump(b, x.homeLand[1], DUR.ignite),
      "[env]":
        bump(b, x.envLit, DUR.ignite) + bump(b, x.homeLand[2], DUR.ignite),
      "[tasks":
        bump(b, x.tasksLit, DUR.ignite) + bump(b, x.homeLand[0], DUR.ignite),
    },
  };
}

/** A piece of the `[ci]` line that goes home to the table it came from. */
interface Piece {
  /** Where it sits in its line (columns), and its runs. */
  col: number;
  runs: Run[];
  /** The card header it flies into, and when it leaves and lands. */
  header: RegExp;
  at: number;
  land: number;
  /** Its underline's colour and draw-on beat, if it has one. */
  under?: { color: string; at: number };
}

/** The `[ci]` line split into its two big lines, and the pieces that go home. */
interface Headline {
  lines: { runs: Run[]; y: number; from: number; pieces: Piece[] }[];
}

/**
 * The take's `[ci] api ready: node v…, APP_ENV=…` as the climax sets it:
 * `[ci] api ready:` over `node v…, APP_ENV=…`, and the three pieces that
 * name a table: `[ci]` (a task), the node version (a tool) and `APP_ENV=…`
 * (an env var), each with the card header it goes home to.
 */
function headline(runs: readonly Run[], x: Take): Headline | null {
  const text = runs.map((r) => r.text).join("");
  const at = text.indexOf(": ");
  if (at < 0) return null;
  const [head, rest] = splitRuns(runs, at + 1);
  const tail = splitRuns(rest, 1)[1];
  const tailText = tail.map((r) => r.text).join("");
  const piece = (
    line: readonly Run[],
    lineText: string,
    re: RegExp,
    header: RegExp,
    at: number,
    land: number,
    under?: Piece["under"],
  ): Piece[] => {
    const m = re.exec(lineText);
    if (!m) return [];
    const runs = splitRuns(splitRuns(line, m.index)[1], m[0].length)[0];
    return [{ col: m.index, runs, header, at, land, under }];
  };
  const headText = head.map((r) => r.text).join("");
  return {
    lines: [
      {
        runs: heavy(head),
        y: CLIMAX.y[0],
        from: x.ready - DUR.short,
        pieces: piece(
          heavy(head),
          headText,
          /^\[[^\]]+\]/,
          /^\[tasks/,
          x.home[0],
          x.homeLand[0],
        ),
      },
      {
        runs: heavy(tail),
        y: CLIMAX.y[1],
        from: x.line2 - DUR.short,
        pieces: [
          ...piece(
            heavy(tail),
            tailText,
            /node v[^,\s]+/,
            /^\[tools\]/,
            x.home[1],
            x.homeLand[1],
            {
              color: PILLAR.tools,
              at: x.underTools,
            },
          ),
          ...piece(
            heavy(tail),
            tailText,
            /APP_ENV=\S+/,
            /^\[env\]/,
            x.home[2],
            x.homeLand[2],
            {
              color: PILLAR.env,
              at: x.envLit,
            },
          ),
        ],
      },
    ],
  };
}

/**
 * Where a piece goes home: its header's middle, at the card's left edge,
 * where the header ignites (ART.md §7 "Header lighting"). A piece `w` px
 * wide when it gets there ends with its right edge a hair left of the
 * card, so it fades into the header's side without crossing the card's
 * text (m2).
 */
function homeOf(
  rows: CardRows,
  cardX: number,
  re: RegExp,
  w: number,
): Pt | null {
  const r = rows.find((x) => x.kind === "header" && re.test(x.text));
  if (!r) return null;
  const top = r.top ?? r.y - 0.8 * TYPE.card.size;
  return {
    x: cardX - HOME_GAP - w / 2,
    y: top + (r.h ?? TYPE.card.lineH) / 2,
  };
}

/** A piece shrinks to this fraction of its size as it goes home. */
const HOME_SCALE = 0.12;
/** How far left of the card's edge a piece fades out, px. */
const HOME_GAP = 6;

/**
 * A piece's way home, from its place in the big type to its header's side
 * of the card: an arc (lift 0.2) for a piece that starts well left of the
 * card; one that starts under it (`APP_ENV=…`, right of centre) first runs
 * left under the card's bottom edge, then rises to its header, so it never
 * crosses the card's lower rows (m2).
 */
function homePath(from: Pt, home: Pt): (u: number) => Pt {
  if (from.x < home.x - 200) {
    const c = arc(from, home, MOTION.arcLift);
    return (u) => curveAt(c, u);
  }
  const c = { a: from, b: home, c: { x: home.x - 100, y: from.y } };
  return (u) => curveAt(c, u);
}

/**
 * The `[ci]` line in big type (ART.md §12.10): its two lines rising in,
 * the first with caption 2 and the bell; its underlines; the glow behind
 * and a slow push-in over the hold. Then, as the song's line comes back,
 * it goes home: `[ci]`, the node version and `APP_ENV=…` each fly into
 * the card header of the table they came from, shrinking to the card's
 * type and fading into it (the header ignites as each lands), while the
 * words that name no table fade where they stand. By the morph only the
 * card is left, its three headers lit.
 */
function climax(g: G, h: Headline, rows: CardRows, x: Take): void {
  const { ctx, b } = g;
  if (b < x.ready - DUR.short) return;
  const size = TYPE.big.climax;
  const adv = advance(size);
  const mid = {
    x: CLIMAX.x,
    y: (CLIMAX.y[0] + CLIMAX.y[1]) / 2 - 0.36 * size,
  };
  const [push0, push1] = x.push;
  const push =
    1 + MOTION.pushIn * span(b, push0, Math.max(0, push1 - push0), "glide");
  // The words that name no table fade as the first piece leaves, gone
  // before the other two leave across where they stood.
  const rest = 1 - span(b, x.home[0], DUR.tick, "glide");
  ctx.save();
  // The light behind swells as the line lands, and goes as it breaks up.
  glow(
    ctx,
    mid.x,
    mid.y,
    GLOW.climax.radius,
    PALETTE.paper,
    GLOW.climax.alpha *
      span(b, x.ready - DUR.short, 2, "glide") *
      (1 - span(b, x.home[0], DUR.beat, "glide")),
  );
  // And blooms once as its first line lands with the bell (the slam's
  // impact light, decaying over half a beat).
  const bloom = slam(b, x.ready).impact;
  if (bloom > 0)
    glow(
      ctx,
      CLIMAX.x,
      CLIMAX.y[0] - 0.36 * size,
      GLOW.slam.radius,
      PALETTE.paper,
      GLOW.slam.alpha * bloom,
    );
  // Where a point of the line stands under the push-in.
  const pushed = (px: number, py: number) => ({
    x: mid.x + (px - mid.x) * push,
    y: mid.y + (py - mid.y) * push,
  });
  for (const line of h.lines) {
    const e = enter(b, line.from, DUR.short);
    const x0 = CLIMAX.x - runsWidth(line.runs, size) / 2;
    const y = line.y + e.dy;
    // The line, less its pieces once they have gone.
    if (rest > 0 && e.alpha > 0) {
      ctx.save();
      ctx.translate(mid.x, mid.y);
      ctx.scale(push, push);
      ctx.translate(-mid.x, -mid.y);
      ctx.globalAlpha *= e.alpha * rest;
      for (const [part, at] of cutAt(line.runs, line.pieces))
        if (!line.pieces.some((p) => p.col === at))
          drawTermLine(ctx, part, x0 + at * adv, y, size);
      ctx.restore();
    }
    // The pieces: in place, underlined, then home.
    for (const p of line.pieces) {
      const k = EASE.travel(progress(p.at, p.land, b));
      if (k >= 1) continue;
      const w = runsWidth(p.runs, size);
      const from = pushed(x0 + p.col * adv + w / 2, y - 0.36 * size);
      const home =
        homeOf(rows, COMPACT.x, p.header, w * push * HOME_SCALE) ?? from;
      const c = homePath(from, home)(k);
      const s = push * lerp(1, HOME_SCALE, k);
      const a = e.alpha * (1 - progress(0.55, 1, k));
      if (a <= 0) continue;
      ctx.save();
      ctx.globalAlpha *= a;
      ctx.translate(c.x, c.y);
      ctx.scale(s, s);
      ctx.translate(-w / 2, 0.36 * size);
      drawTermLine(ctx, p.runs, 0, 0, size);
      if (p.under)
        underline(
          ctx,
          0,
          0,
          w,
          span(b, p.under.at, DUR.half, "arrive"),
          p.under.color,
        );
      ctx.restore();
    }
  }
  ctx.restore();
}

/** A line's runs cut at its pieces: each stretch, and the column it starts at. */
function cutAt(
  runs: readonly Run[],
  pieces: readonly Piece[],
): [Run[], number][] {
  const cuts = [
    ...new Set(
      pieces.flatMap((p) => [
        p.col,
        p.col + Array.from(p.runs.map((r) => r.text).join("")).length,
      ]),
    ),
  ].sort((a, z) => a - z);
  const out: [Run[], number][] = [];
  let left: Run[] = [...runs];
  let at = 0;
  for (const c of cuts) {
    if (c <= at) continue;
    const [a, z] = splitRuns(left, c - at);
    out.push([a, at]);
    left = z;
    at = c;
  }
  if (left.length) out.push([left, at]);
  return out;
}

export const scene = greyScene(
  "clone",
  (g) => {
    const { ctx, b } = g;
    const c = capture(g.d, "C19");
    const toml = fileOf(g.d, "C19", "api/mise.toml");
    const x = takeOf(g.d);
    // The card, held from the breath with [tools] and [env] open: lit as
    // each table is used, its tables folding and the card shrinking to its
    // three headers once the lanes have flown into it.
    const rect = lerpRect(CARD, COMPACT, span(b, x.fold, DUR.move, "move"));
    const folded = span(b, x.fold, DUR.fold, "move");
    const rows: CardRows = keep(g, () =>
      inFocus(ctx, x.focus, "card", b, () =>
        card(ctx, rect, {
          title: "~/api/mise.toml",
          text: toml,
          from: "C19",
          open: [],
          unfold: Object.fromEntries(CLONE_OPEN.map((h) => [h, 1 - folded])),
          ...lights(x, b),
        }),
      ),
    );
    // The terminal: the take; after the rows' cut, the command it ran,
    // dimmed, under its badges; then the window leaves with its text and
    // badges, all together (M3: never an empty window).
    const w = windowOf(x, b);
    if (w.alpha > 0)
      keep(g, () => {
        ctx.save();
        ctx.translate(0, w.dy);
        paneWindow(g, LEFT, w.alpha);
        ctx.restore();
      });
    if (c) {
      if (w.alpha > 0)
        keep(g, () => {
          ctx.save();
          ctx.translate(0, w.dy);
          ctx.globalAlpha *= w.alpha;
          bandRows(g, c, x);
          ctx.restore();
        });
      ctx.save();
      ctx.translate(0, w.dy);
      shot(g, "C19", LEFT, -1, x.out + DUR.tick, () => x.plays, {
        bare: true,
        keep: true,
        fade: 0,
        dim: () => w.alpha,
        focus: (b) =>
          Math.max(
            span(b, x.cut, DUR.tick, "glide"),
            unfocus(x.focus, "term", b),
          ),
        lineAlpha: GIT_DIM,
        tail: { badgeUntil: x.tail + DUR.tick },
      });
      ctx.restore();
      liftCommand(g, c, x);
      // The versions the lock pins, onto the card beside their rows.
      if (b >= (x.pinned[0]?.at ?? Infinity) && b < x.chipsOut + DUR.exit)
        keep(g, () => {
          for (const l of pinnedLifts(c, x, rows)) liftValue(ctx, l, b);
        });
      // The lanes settle into the card's task header's band, taking its
      // colour as they fly (kit/diagrams/lanes.ts), and it lights as the
      // first hands over.
      const head = rows.find(
        (r) => r.kind === "header" && /^\[tasks/.test(r.text),
      );
      const band = {
        x: rect.x + CARD_ART.bandInset,
        w: rect.w - 2 * CARD_ART.bandInset,
      };
      const to: Rect =
        head && head.top !== undefined && head.h !== undefined
          ? { ...band, y: head.top, h: head.h }
          : { ...band, y: rect.y + CARD_ART.tabH, h: TYPE.card.lineH };
      inFocus(ctx, x.focus, "lanes", b, () =>
        lanes(ctx, {
          lines: x.lines,
          b,
          badgeAt: x.lane0,
          settle: { at: x.settle, to, band: PILLAR_BAND.tasks },
        }),
      );
      const h = x.readyRuns ? headline(x.readyRuns, x) : null;
      if (h) climax(g, h, rows, x);
    } else if (w.alpha > 0) {
      // No take: the labelled box in the window, leaving with it.
      keep(g, () => missing(ctx, rectOf(LEFT), "C19", w.alpha));
    }
  },
  {
    // The window is the lit screen until it leaves.
    lit: (b, d): LitRect | null => {
      const w = windowOf(takeOf(d), b);
      const r = rectOf(LEFT);
      return w.alpha > 0 ? termLit({ ...r, y: r.y + w.dy }, w.alpha) : null;
    },
    events: (d) => takeOf(d).p.events(),
  },
);

/** The schedule, for the pacing checks. */
export const cloneSchedule = takeOf;
