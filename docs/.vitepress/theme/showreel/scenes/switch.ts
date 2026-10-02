// switch (plan v3 §3, Act II; ART.md §12.8; STORYBOARD.md "Pacing"): the
// folder diagram holds from the Versions ticket, api/ current, each folder
// card holding its own `node` line as C7 left the files, beside the
// terminal at `~/work/api $`. C7's four commands play one at a time,
// typed at TYPE_RATE, each result held for its read, the diagram dimmed
// while the terminal acts and the terminal dimmed while the diagram
// answers: `cd ../dashboard` (the chrome's cwd dot hops to a marimba run
// up the bed's F minor), then the diagram's cwd dot hops down the tree to
// dashboard/; `node --version`, and the version it printed lifts out of
// its row onto dashboard/ as a value chip; the caption. `cd ../api` (a run
// up C minor), the dot hops back to api/, and `node --version` again, whose
// chip lands on api/ before the poster frame (beat 18): both versions on
// their folders, api current, the caption up. After the poster each
// version's thread draws from the row that printed it to its chip (ART.md
// §9 step 5), dashboard's then api's. Then the diagram folds away: the
// chips and threads go, dashboard/ and the tree fall away, and api/ grows
// into the station card's place with the card coming in over it,
// `~/work/api/mise.toml` as C7 left it, the folder becoming the card
// packslip carries on with. The terminal's last screen holds through the
// bar line (kit/rest.ts switch|packslip), and the card with it when that
// bar line keeps it (restCard); when it does not, the card dips out over
// the last quarter beat and packslip brings it straight back in the same
// place.

import type { ReelFacts } from "../bible";
import {
  type Capture,
  capture,
  type CaptureId,
  type ReelData,
  reelData,
  screenAt,
} from "../captures";
import {
  becomeRect,
  type Folder,
  folders,
  type Lift,
} from "../kit/diagrams/folders";
import {
  beatOf,
  card,
  clipTime,
  cut,
  findRow,
  greyScene,
  keep,
  type Play,
  shot,
  step,
  threadAlong,
} from "../kit/grey";
import { span } from "../kit/motion";
import { focusOf, focusPlan, Pace, readsOf, unfocus } from "../kit/pace";
import { cardFit } from "../kit/parts";
import { drawPaneWindow, OPENS, SWITCH_ROOT, switchFolders } from "../kit/rest";
import { DUR, THREAD } from "../kit/style";
import { type Run, termLayout } from "../kit/term";
import type { SceneCues } from "../score/cues";
import {
  cardOptions,
  restPane,
  STATION,
  STATION_KEPT,
} from "./g3-versions-env/rests";
import { resultOf } from "./g3-versions-env/take";
import { perFacts, takeIn } from "./g2-tools/common";

/**
 * The versions' lift onto their folders (ART.md §9's form, rise, flight
 * and settle): a beat from the chip forming to its rest on the folder.
 */
const LIFT: Lift = {
  form: DUR.flick,
  rise: DUR.flick,
  fly: DUR.half,
  settle: DUR.tick,
};
const LIFT_BEATS = LIFT.form + LIFT.rise + LIFT.fly + LIFT.settle;

/**
 * The poster frame (2:08, reel.ts POSTER_TIME): both chips on their
 * folders, api current. The threads draw on after it.
 */
export const POSTER = 18;

/** The terminal: the pane both bar lines keep (kit/rest.ts versions|switch, switch|packslip). */
const PANE = restPane("versions|switch").pane;

/** C7's steps, in the order the take ran them, and each one's lines to read. */
const STEPS = [
  ["cdDashboard", "cd-dashboard", 0],
  ["nodeDashboard", "node-dashboard", 1],
  ["cdApi", "cd-api", 0],
  ["nodeApi", "node-api", 1],
] as const;

/**
 * When everything happens (kit/pace.ts): C7's commands and the diagram's
 * answers one at a time, the caption anchored on dashboard's version, and
 * the focus. Without the take, the plan's own beats (sections.json).
 */
const schedule = perFacts((d: ReelData | null) => {
  const c = takeIn(d, "C7");
  const p = new Pace("switch", { d });
  const term = (i: number, dur: number) => {
    const [name, st, lines] = STEPS[i];
    return c
      ? p.term(name, (at) => [step(c, st, at)], { lines })
      : { ...p.move(name, dur), plays: [] as Play[] };
  };
  const cdDashboard = term(0, 2.23);
  const toDashboard = p.move("toDashboard", DUR.move);
  const nodeDashboard = term(1, 2.12);
  const dashVersion = p.move("dashVersion", LIFT_BEATS);
  p.caption(0);
  const cdApi = term(2, 1.69);
  const toApi = p.move("toApi", DUR.move);
  const nodeApi = term(3, 2.12);
  const apiVersion = p.move("apiVersion", LIFT_BEATS);
  // The poster holds, then each version's thread draws on, a quarter beat apart.
  const threads = p.move("threads", DUR.tick + DUR.thread, {
    after: POSTER + DUR.tick,
  });
  const fold = p.move("fold", DUR.flick + DUR.move);
  const plays: Play[] = c
    ? [
        cut(0, OPENS.C7!(c)),
        ...cdDashboard.plays,
        ...nodeDashboard.plays,
        ...cdApi.plays,
        ...nodeApi.plays,
      ]
    : [];
  // The beat each result lands on (the score's cues): each `cd`'s new
  // prompt and each version's line.
  const landed = (name: string, fallback: number) =>
    c ? beatOf(plays, resultOf(c, name)) : fallback;
  const at = {
    cdDashboard: landed("cd-dashboard", cdDashboard.end),
    dashboard: landed("node-dashboard", nodeDashboard.end),
    cdApi: landed("cd-api", cdApi.end),
    api: landed("node-api", nodeApi.end),
  };
  // The terminal has the focus while it acts, the diagram while it answers
  // and the caption reads, the terminal kept lit until each output has
  // been read (rule 3); both from api's landing, for the poster, to the
  // bar line.
  const focus = focusPlan(
    [
      [cdDashboard.at, "term"],
      [toDashboard.at, "diagram"],
      [nodeDashboard.at, "term"],
      [dashVersion.at, "diagram"],
      [cdApi.at, "term"],
      [toApi.at, "diagram"],
      [nodeApi.at, "term"],
      [apiVersion.at, "diagram"],
      [apiVersion.end, "*"],
    ],
    readsOf(cdDashboard, nodeDashboard, cdApi, nodeApi),
  );
  return {
    p,
    plays,
    at,
    hops: { dashboard: toDashboard.at, api: toApi.at },
    lifts: { dashboard: dashVersion.at, api: apiVersion.at },
    threads: { dashboard: threads.at, api: threads.at + DUR.tick },
    leave: fold.at,
    focus,
  };
});

/**
 * When the bar line keeps no card (kit/rest.ts switch|packslip holds the
 * terminal only): the card dips out over a quarter beat, gone a frame or
 * so before the bar line (the last frame must already be the rest), and
 * packslip brings it back in place over its first eighth.
 */
const dipAt = (beats: number): number => beats - 0.05 - DUR.tick;

/**
 * What a `node --version` printed on beat `beat`, as its folder's chip: the
 * row's runs, where the row sits in the pane, and an x past every row
 * shown then (the chip runs right to it before it turns for its folder);
 * and where the row ends, for its thread.
 */
function printed(
  c: Capture,
  plays: readonly Play[],
  beat: number,
): { version: Folder["version"]; end: { x: number; y: number } } | null {
  // A millisecond past the result, clear of float error at its frame.
  const ct = clipTime(plays, beat).ct + 1e-3;
  const sc = screenAt(c, ct);
  const row = findRow(sc, /^v\d/);
  if (!row) return null;
  const runs: Run[] = sc.lines[row.line].map((r) => ({
    ...r,
    text: r.text.trimEnd(),
  }));
  const L = termLayout(PANE, sc.lines.length);
  const cols = (i: number) =>
    Array.from(
      sc.lines[i]
        .map((r) => r.text)
        .join("")
        .trimEnd(),
    ).length;
  const widest = Math.max(...sc.lines.map((_, i) => cols(i)));
  const cell = L.cell(row.line, cols(row.line));
  return {
    version: {
      runs,
      at: beat,
      from: { x: PANE.x0, y: L.baseline(row.line), size: PANE.size },
      clear: L.col(widest) + 48,
    },
    end: { x: cell.x + THREAD.exit, y: cell.y + cell.h / 2 },
  };
}

export const scene = greyScene(
  "switch",
  (g) => {
    const { ctx, b } = g;
    const c = capture(g.d, "C7");
    const S = schedule(g.d);
    const { plays, at } = S;
    // The terminal: the window and its text hold across both bar lines,
    // from the ticket's `~/work/api $` to the take's last screen (kit/rest.ts
    // HOLDS.C7), which packslip dissolves into its own prompt; dimmed while
    // the diagram has the focus.
    const dim = unfocus(S.focus, "term", b);
    keep(g, () => drawPaneWindow(ctx, PANE, dim));
    shot(g, "C7", PANE, -1, Infinity, () => plays, {
      bare: true,
      keep: true,
      focus: () => dim,
    });
    // The folder diagram, over the terminal so the versions lift out of it.
    const lifts =
      c && plays.length
        ? {
            api: printed(c, plays, at.api),
            dashboard: printed(c, plays, at.dashboard),
          }
        : { api: null, dashboard: null };
    const list = switchFolders(g.d).map((f) => {
      const name = f.name === "api/" ? "api" : "dashboard";
      const v = lifts[name]?.version;
      // Each version lifts once its line has held, half a beat after the dot's hop.
      return { ...f, version: v ? { ...v, at: S.lifts[name] } : undefined };
    });
    // api/ becomes the station card: the card comes in over it as it grows,
    // to the card's own height where the bar line fits it to its rows.
    const cover = span(b, S.leave + DUR.flick, DUR.half, "glide");
    const dip = STATION_KEPT
      ? 1
      : 1 - span(b, dipAt(g.s.beats), DUR.tick, "glide");
    const station = cardOptions(g.d, STATION);
    const become = {
      index: 0,
      rect: station.fit ? cardFit(STATION.rect, station) : STATION.rect,
      cover,
      alpha: dip,
    };
    const layout = keep(g, () => {
      const out = folders(ctx, {
        folders: list,
        b,
        cwd: 0,
        cds: [
          { at: S.hops.dashboard, to: 1 },
          { at: S.hops.api, to: 0 },
        ],
        root: SWITCH_ROOT,
        lift: LIFT,
        leave: S.leave,
        become,
        alpha: focusOf(S.focus, "diagram", b),
      });
      const r = becomeRect({ b, leave: S.leave, become });
      if (r && cover > 0) card(ctx, r, { ...station, alpha: cover * dip });
      return out;
    });
    // Each version's thread, from the end of the row that printed it to its
    // chip: out along the row, down or up the gutter, into the chip's left
    // edge (ART.md §9), in the take's order; gone as the diagram folds.
    const fade = 1 - span(b, S.leave, DUR.threadFade, "glide");
    (["dashboard", "api"] as const).forEach((name) => {
      const from = lifts[name]?.end;
      const chip = layout.chips[name === "api" ? 0 : 1];
      const k = span(b, S.threads[name], DUR.thread, "arrive");
      if (!from || !chip || k <= 0 || fade <= 0) return;
      const y = chip.y + chip.h / 2;
      threadAlong(
        ctx,
        [
          from,
          { x: THREAD.gutterX, y: from.y },
          { x: THREAD.gutterX, y },
          { x: chip.x, y },
        ],
        k,
        fade,
      );
    });
  },
  { events: (d) => schedule(d).p.events() },
);

/** The score's cues (score/cues.ts): each `cd` landing and each version printing, where the picture has them. */
export const cues: SceneCues<"switch"> = (facts) => {
  const { at } = schedule(reelData(facts));
  return {
    cdDashboard: at.cdDashboard,
    dashboard: at.dashboard,
    cdApi: at.cdApi,
    api: at.api,
  };
};

/** The section's schedule (kit/pace.ts Pace) and its takes' plays, for the pacing checks. */
export const pacing = (
  facts: ReelFacts | null,
): { pace: Pace; takes: Partial<Record<CaptureId, readonly Play[]>> } => {
  const S = schedule(reelData(facts));
  return { pace: S.p, takes: { C7: S.plays } };
};
