// redact (plan v3 §3, Act III; ART.md §12.13, §12.16): the card holds from
// vars ([env] open and lit), and vars' last screen dissolves into C10's
// prompt as it comes up. Then one thing at a time (STORYBOARD.md "Pacing",
// kit/pace.ts): the card's `_.file` line lifts out of it into a panel the
// stage's width, growing to 44 px as the card dims; in the panel the line
// is rewritten in place, the new one (C10's: `.env` plus `.env.deploy`
// with `redact = true`) seating where the old one stood; the card folds
// away into its tab strip. `.env.deploy`'s file card flies out of its name
// in the panel (tagged "gitignored", the fixture's token) and, a quarter
// beat later, the deploy task's file card slides in under it, cropped to
// the end of its one line, where it prints the token. `mise run deploy
// staging` types (at TYPE_RATE) and prints its real line with
// `[redacted]`, held for its read; a thread runs from the token to it and
// a stamp lands round it, and caption 1 says what happened. The footnote
// says what redaction does and does not do, and holds as long as the
// section shows `[redacted]`. Then it all goes back where it came from, in
// one cascade: the marks go, the task card leaves and `.env.deploy` folds
// back into its name, the card unfolds from its tab as the panel shrinks
// back into its folded row, and [env] folds and unlights, so the card
// (every table folded), the terminal on its `[redacted]` line and the
// footnote hold into the Tasks ticket (kit/rest.ts redact|tasks). Whatever
// is not the focus dims (PACE.focusDim); all of it is back up for the rest.

import { PALETTE } from "../bible";
import {
  type Capture,
  capture,
  fileOf,
  fixture,
  type ReelData,
  reelData,
  stepOf,
} from "../captures";
import {
  beatOf,
  type CardRows,
  cut,
  ENV_CARD,
  ENV_PANE,
  fileCard,
  findRow,
  footnote,
  greyScene,
  gridRuns,
  keep,
  type Play,
  raised,
  restScreen,
  stamp,
  step,
  threadAlong,
  tomlRuns,
  win,
} from "../kit/grey";
import { lerpRect, type Rect, span } from "../kit/motion";
import { type FocusPlan, focusOf, Pace, unfocus } from "../kit/pace";
import { roundedRect } from "../fx";
import { STAMP_LAND } from "../kit/parts";
import { drawPaneWindow, HOLDS, REDACT_NOTE } from "../kit/rest";
import {
  CARD_ART,
  COLOR,
  DUR,
  FILE_ART,
  LAYOUT,
  MOTION,
  RADIUS,
  SHADOW,
  STAMP,
  STROKE,
  THREAD,
  TYPE,
  ZOOM_ART,
} from "../kit/style";
import { advance, run } from "../kit/term";
import { clamp, lerp } from "../math";
import type { SceneCues } from "../score/cues";
import type { SceneEvents } from "../storyboard";
import { foldedCard } from "./g3-versions-env/card";
import { fileBetween, outAndBack } from "./g3-versions-env/lift";
import { term } from "./g3-versions-env/take";

/**
 * Putting it all back, one cascade (beats after its start): the marks go,
 * then the task card leaves and `.env.deploy` flies back into its name,
 * then the card unfolds from its tab strip as the panel shrinks back into
 * its folded row, then [env] folds and unlights.
 */
const BACK = {
  unmark: 0,
  fold: DUR.tick,
  card: DUR.tick + DUR.std,
  env: DUR.tick + DUR.std + DUR.zoom,
} as const;
const BACK_DUR = BACK.env + DUR.fold;
/** The two file cards come in a quarter beat apart: `.env.deploy` flying (3/4), the task's sliding (1/2). */
const FILES_GAP = DUR.tick;
const FILES_DUR = Math.max(DUR.std, FILES_GAP + DUR.enter);

/** The file cards in the right column, under the panel: one row each, this far apart. */
const FILE_H = FILE_ART.tabH + 2 * FILE_ART.top + TYPE.file.lineH;
const FILE_GAP = 30;
const DEPLOY_ENV: Rect = {
  x: LAYOUT.right.x,
  y: LAYOUT.zoom.y + LAYOUT.zoom.h + FILE_GAP + 6,
  w: LAYOUT.right.w,
  h: FILE_H,
};
const DEPLOY_SCRIPT: Rect = {
  x: LAYOUT.right.x,
  y: DEPLOY_ENV.y + FILE_H + FILE_GAP,
  w: LAYOUT.right.w,
  h: FILE_H,
};

/** The `_.file` line of api's mise.toml as a take left it. */
const fileLine = (text: string | null): string | null =>
  text?.split("\n").find((l) => /^_\.file = /.test(l)) ?? null;

/** The size the panel sets its line at: ZOOM_ART's, or less for a line too wide for it. */
const zoomSize = (line: string): number =>
  Math.min(
    ZOOM_ART.size,
    Math.floor(
      (LAYOUT.zoom.w - ZOOM_ART.inset - 72) /
        (Array.from(line).length * advance(1)),
    ),
  );

/** The frame the run's `[redacted]` line first shows. */
function firstRedacted(c: Capture): number {
  const f = c.frames.find((x) =>
    x.rows.some((r) => c.rows[r].some(([text]) => text.includes("[redacted]"))),
  );
  return f ? f.t : c.frames[c.frames.length - 1].t;
}

/** The take, when the set has it (a set of versions alone has no captures). */
const take = (d: ReelData | null): Capture | null =>
  d?.captures ? capture(d, "C10") : null;

/** The section's beats: when each motion starts and stops, and where the take's beats fall. */
interface Beats {
  dissolve: { at: number; end: number };
  zoom: { at: number; end: number };
  rewrite: { at: number; end: number };
  fold: { at: number; end: number };
  files: { at: number; end: number };
  stamp: { at: number; end: number };
  foot: { at: number; end: number };
  back: { at: number; end: number };
  redacted: number;
}

/**
 * The section's schedule (kit/pace.ts), in the order STORYBOARD.md's event
 * plan gives: one focal motion at a time, half a beat apart; the run typed
 * at TYPE_RATE and its line held for its read before the thread runs.
 */
function plan(d: ReelData | null, c: Capture) {
  const p = new Pace("redact", { d, from: 0 });
  const dissolve = p.move("dissolve", DUR.half);
  const zoom = p.move("zoom", DUR.zoom);
  const rewrite = p.move("rewrite", DUR.seatGap + DUR.seat);
  const fold = p.move("fold", DUR.fold);
  const files = p.move("files", FILES_DUR);
  const deploy = p.term(
    "deploy",
    (at) => [step(c, "deploy", at, { until: HOLDS.C10!(c) })],
    { lines: 2 },
  );
  // The thread and the stamp annotate the line where it stands: the output
  // itself holds still under them for its read (kit/pace.ts).
  // The thread, the stamp's approach, and its ring (half a beat from the landing).
  const st = p.move("stamp", DUR.thread + STAMP_LAND + DUR.half);
  p.caption(0);
  const foot = p.move("foot", DUR.enter);
  const back = p.move("foldBack", BACK_DUR);
  const plays: Play[] = [cut(0, stepOf(c, "deploy").type), ...deploy.plays];
  const focus: FocusPlan = [
    [zoom.at, ["panel", "card"]],
    [files.at, ["panel", "files"]],
    [deploy.at, "term"],
    [st.at, "*"],
  ];
  const beats: Beats = {
    dissolve,
    zoom,
    rewrite,
    fold,
    files,
    stamp: st,
    foot,
    back,
    redacted: beatOf(plays, firstRedacted(c)),
  };
  return { pace: p, plays, focus, events: p.events(), beats };
}

type Plan = ReturnType<typeof plan>;
const PLANS = new WeakMap<Capture, Plan>();
/** The schedule for a take (a pure function of it and the facts' captions); null without it. */
export function schedule(d: ReelData | null): Plan | null {
  const c = take(d);
  if (!c) return null;
  let s = PLANS.get(c);
  if (!s) PLANS.set(c, (s = plan(d, c)));
  return s;
}

/** Without the take: the storyboard's event plan (sections.json). */
const FALLBACK: Beats = {
  dissolve: { at: 0, end: 0.5 },
  zoom: { at: 1, end: 1.75 },
  rewrite: { at: 2.25, end: 3 },
  fold: { at: 3.5, end: 4 },
  files: { at: 4.5, end: 5.25 },
  stamp: { at: 9.2, end: 10.45 },
  foot: { at: 12.5, end: 13 },
  back: { at: 13.5, end: 13.5 + BACK_DUR },
  redacted: 8.3,
};

export const scene = greyScene(
  "redact",
  (g) => {
    const { ctx, b } = g;
    const s = schedule(g.d);
    const B = s?.beats ?? FALLBACK;
    const focus: FocusPlan = s?.focus ?? [];
    const plays = s?.plays ?? [];
    const f = B.back.at;
    const before = fileOf(g.d, "C9", "work/api/mise.toml");
    const after = fileOf(g.d, "C10", "work/api/mise.toml");
    const was = fileLine(before);
    const now = fileLine(after);
    // The card: at rest from vars, dimming as its line lifts out, folding
    // away into its tab strip; back later as C10 left the file, then [env]
    // folds and unlights into the rest.
    const lift = span(b, B.zoom.at, DUR.zoom, "move");
    const land = span(b, f + BACK.card, DUR.zoom, "move");
    const back = b >= f + BACK.card;
    const dimK = back ? 1 - land : lift;
    const cardAlpha =
      (1 - (1 - ZOOM_ART.cardDim) * dimK) * focusOf(focus, "card", b);
    const away = back
      ? 1 - span(b, f + BACK.card, DUR.enter, "arrive")
      : span(b, B.fold.at, DUR.fold, "move");
    const envOpen = back ? 1 - span(b, f + BACK.env, DUR.fold, "move") : 1;
    const envLit = back ? 1 - span(b, f + BACK.env, DUR.exit, "leave") : 1;
    const rows: CardRows = keep(g, () =>
      foldedCard(
        ctx,
        ENV_CARD,
        {
          title: "~/work/api/mise.toml",
          text: back ? after : before,
          from: back ? "C10" : "C9",
          open: envOpen >= 1 ? ["[env]"] : [],
          unfold: envOpen > 0 && envOpen < 1 ? { "[env]": envOpen } : undefined,
          lit: envLit > 0 ? { "[env]": envLit } : undefined,
          alpha: cardAlpha,
        },
        away,
      ),
    );
    // The terminal: vars' last screen, held on the bar line, dissolves into
    // C10's prompt as it comes up.
    const termDim = unfocus(focus, "term", b);
    keep(g, () => drawPaneWindow(ctx, ENV_PANE, termDim));
    const up = span(b, B.dissolve.at, B.dissolve.end - B.dissolve.at, "arrive");
    restScreen(g, "vars|redact", 1 - up);
    const sh = term(g, "C10", ENV_PANE, () => plays, {
      alpha: up,
      dim: termDim,
      keep: true,
    });
    // The panel: the card's `_.file` line lifting out, rewritten in place,
    // and shrinking back into the card's folded row.
    ctx.save();
    ctx.globalAlpha *= focusOf(focus, "panel", b);
    const zoomed = was && now ? zoomPanelAt(ctx, b, B, was, now, rows) : null;
    ctx.restore();
    // The marks (the thread, the stamp, the outlines): on with the thread, gone as the cascade starts.
    const marksOn = (fade: number) =>
      win(b, B.stamp.at, f + BACK.unmark + fade, fade);
    const filesA = focusOf(focus, "files", b);
    ctx.save();
    ctx.globalAlpha *= filesA;
    // `.env.deploy`, out of its name in the panel and back into it.
    const deployEnv = fixture(g.d, "api/.env.deploy");
    const envLines = deployEnv === null ? null : deployEnv.trim().split("\n");
    let envRows: CardRows = [];
    if (zoomed?.token) {
      const at = outAndBack(b, B.files.at, f + BACK.fold);
      if (at.k >= 1 && at.alpha >= 1)
        envRows = fileCard(ctx, DEPLOY_ENV, {
          title: ".env.deploy",
          tag: "gitignored",
          lines: envLines,
          from: "fixtures",
          hl: [{ re: /=/, a: marksOn(DUR.tick) }],
        });
      else
        // Out from under the panel's bottom edge below its name, and back
        // under it: it never covers the line it comes out of.
        fileBetween(
          ctx,
          zoomed.token,
          DEPLOY_ENV,
          at,
          {
            title: ".env.deploy",
            tag: "gitignored",
            lines: envLines,
            from: "fixtures",
          },
          zoomed.rect,
        );
    }
    // The deploy task: its one line, cropped to its end, where it prints the token.
    const script = fixture(g.d, "api/mise-tasks/deploy");
    const sIn = span(b, B.files.at + FILES_GAP, DUR.enter, "arrive");
    const sOut = span(b, f + BACK.fold, DUR.exit, "leave");
    // The variable it prints, outlined while the thread runs from its value
    // to `[redacted]`: defined, printed, masked.
    const key = envLines?.find((l) => /=/.test(l))?.split("=")[0] ?? null;
    if (sIn > 0 && sOut < 1) {
      ctx.save();
      ctx.globalAlpha *= sIn * (1 - sOut);
      ctx.translate(MOTION.slideIn * (1 - sIn), MOTION.fallOut * sOut);
      scriptCard(
        ctx,
        script,
        key === null ? null : `$${key}`,
        marksOn(DUR.tick),
      );
      ctx.restore();
    }
    ctx.restore();
    // The thread from the token to `[redacted]`, and the stamp round it.
    const row = sh ? findRow(sh.screen, /\[redacted\]/) : null;
    const tokenRow = envRows.find((r) => /=/.test(r.text));
    if (sh && row && b >= B.redacted) {
      const col = row.text.indexOf("[redacted]");
      const cell = sh.layout.cell(row.line, col);
      const end = sh.layout.cell(row.line, col + "[redacted]".length);
      const midY = cell.y + cell.h / 2;
      if (tokenRow?.top !== undefined && tokenRow.h !== undefined) {
        const ty = tokenRow.top + tokenRow.h / 2;
        threadAlong(
          ctx,
          [
            { x: DEPLOY_ENV.x + CARD_ART.anchorX, y: ty },
            { x: THREAD.gutterX, y: ty },
            { x: THREAD.gutterX, y: midY },
            // Ending clear of the stamp that lands round it.
            { x: end.x + STAMP.pad + THREAD.exit, y: midY },
          ],
          span(b, B.stamp.at, DUR.thread, "arrive"),
          marksOn(DUR.half),
        );
      }
      stamp(
        ctx,
        {
          x: cell.x - STAMP.pad,
          y: cell.y + 2,
          w: end.x - cell.x + 2 * STAMP.pad,
          h: cell.h - 4,
        },
        b - (B.stamp.at + DUR.thread),
        // It appears by landing (stamp()'s own approach), and fades as the
        // cascade starts.
        { alpha: 1 - span(b, f + BACK.unmark, DUR.tick, "glide") },
      );
    }
    // What redaction does, from its turn on to the bar line: kept, so it
    // holds with `[redacted]` into the Tasks ticket (kit/rest.ts
    // redact|tasks).
    keep(g, () =>
      footnote(ctx, REDACT_NOTE, span(b, B.foot.at, DUR.enter, "arrive")),
    );
  },
  { events: (d): SceneEvents | null => schedule(d)?.events ?? null },
);

/**
 * The panel on beat `b` (ART.md §12.13): from the card's `_.file` row it
 * grows to LAYOUT.zoom (move 3/4), the line growing from the card's 26 px
 * to its own; the line is rewritten in place, the new one seating where
 * the old one stood (it fades over the seat's gap, and the new one drops
 * in over the seat: one line changing, never typed out faster than it can
 * be read); later it shrinks back into the card's folded row and fades as
 * it lands. Returns where `".env.deploy"` sits in it, for the file card
 * flying out of it.
 */
function zoomPanelAt(
  ctx: CanvasRenderingContext2D,
  b: number,
  B: Beats,
  was: string,
  now: string,
  rows: CardRows,
): { token: Rect | null; rect: Rect } | null {
  const unzoom = B.back.at + BACK.card;
  const up = span(b, B.zoom.at, DUR.zoom, "move");
  const down = span(b, unzoom, DUR.zoom, "move");
  if (up <= 0 || down >= 1) return null;
  const Z = LAYOUT.zoom;
  const size = zoomSize(now);
  // Its source going up (the seated row), its target coming down (the fold).
  const src = rows.find((r) => /^_\.file = /.test(r.text));
  const dst = rows.find((r) => r.kind === "fold");
  const rowRect = (r: CardRows[number] | undefined): Rect =>
    r?.top !== undefined && r.h !== undefined
      ? {
          x: ENV_CARD.x + CARD_ART.bandInset,
          y: r.top,
          w: ENV_CARD.w - 2 * CARD_ART.bandInset,
          h: r.h,
        }
      : {
          x: ENV_CARD.x,
          y: ENV_CARD.y + 120,
          w: ENV_CARD.w,
          h: TYPE.card.lineH,
        };
  const going = b < unzoom;
  const k = going ? up : 1 - down;
  const from = rowRect(going ? src : dst);
  const r = lerpRect(from, Z, k);
  const sz = lerp(TYPE.card.size, size, k);
  const x = lerp(ENV_CARD.x + CARD_ART.inset, Z.x + ZOOM_ART.inset, k);
  const y = r.y + r.h / 2 + 0.36 * sz;
  const fade = going ? 1 : 1 - clamp((down - 0.7) / 0.3);
  // ART.md §9, form then rise: its backing forms round the card's row in
  // place over the first 1/8 beat (the move has barely begun), covering
  // the row's own glyphs under the copy, before it grows away.
  const formed = going ? span(b, B.zoom.at, DUR.flick, "arrive") : 1;
  // Coming back, its text folds away ahead of the box, so it never lies
  // over the fold's "1 more line" pill it lands on.
  const ink = going ? 1 : 1 - clamp((down - 0.35) / 0.35);
  // In flight it floats; landed over the terminal, it rests (raised).
  const air = going
    ? 1 - span(b, B.zoom.at + DUR.zoom, DUR.tick, "glide")
    : span(b, unzoom, DUR.tick, "glide");
  // The rewrite: the old line fades as the seat's gap opens, the new one drops in.
  const oldA = 1 - span(b, B.rewrite.at, DUR.seatGap, "glide");
  const drop = span(b, B.rewrite.at + DUR.seatGap, DUR.seat, "drop");
  const newA = clamp(
    span(b, B.rewrite.at + DUR.seatGap, DUR.seat, "arrive") * 2,
  );
  // Where `".env.deploy"` sits in it: its card flies out of it and back.
  const name = '".env.deploy"';
  const i = now.indexOf(name);
  const token: Rect | null =
    i >= 0
      ? {
          x: x + i * advance(sz),
          y: y - 0.8 * sz,
          w: name.length * advance(sz),
          h: sz,
        }
      : null;
  ctx.save();
  ctx.globalAlpha *= fade;
  const radius = lerp(RADIUS.seat, RADIUS.zoom, k);
  if (air > 0)
    raised(ctx, r, radius, {
      shadow: SHADOW.float,
      shadowAlpha: k * air,
      alpha: formed,
    });
  if (air < 1)
    raised(ctx, r, radius, {
      shadow: SHADOW.raised,
      shadowAlpha: k * (1 - air),
      alpha: formed,
    });
  const fadeR = [r.x + r.w - 72, r.x + r.w - 16] as const;
  ctx.save();
  ctx.beginPath();
  ctx.rect(r.x, r.y, r.w, r.h);
  ctx.clip();
  ctx.globalAlpha *= ink;
  if (oldA > 0) {
    ctx.save();
    ctx.globalAlpha *= oldA;
    gridRuns(ctx, tomlRuns(was), x, y, sz, { fade: fadeR });
    ctx.restore();
  }
  if (newA > 0) {
    ctx.save();
    ctx.globalAlpha *= newA;
    gridRuns(ctx, tomlRuns(now), x, y - MOTION.seatDrop * (1 - drop), sz, {
      fade: fadeR,
    });
    ctx.restore();
  }
  ctx.restore();
  ctx.restore();
  return { token, rect: r };
}

/**
 * The deploy task's file card: its one line (the script's last), set from
 * the column that keeps its end in view, the line's head faded off the
 * card's left edge (a viewport crop: the text itself is whole).
 */
function scriptCard(
  ctx: CanvasRenderingContext2D,
  script: string | null,
  token: string | null = null,
  mark = 0,
): void {
  const line = script?.trim().split("\n").at(-1) ?? null;
  if (line === null) {
    fileCard(ctx, DEPLOY_SCRIPT, {
      title: "mise-tasks/deploy",
      lines: null,
      from: "fixtures",
    });
    return;
  }
  // An empty card for its frame, then the line on it.
  const rows = fileCard(ctx, DEPLOY_SCRIPT, {
    title: "mise-tasks/deploy",
    lines: [""],
    from: "fixtures",
  });
  const size = TYPE.file.size;
  const adv = advance(size);
  const x0 = DEPLOY_SCRIPT.x + FILE_ART.inset;
  const cols = Math.floor((DEPLOY_SCRIPT.w - 2 * FILE_ART.inset) / adv);
  const n = Array.from(line).length;
  const skip = Math.max(0, n - cols);
  const y =
    rows[0]?.y ?? DEPLOY_SCRIPT.y + FILE_ART.tabH + FILE_ART.top + 0.8 * size;
  ctx.save();
  ctx.beginPath();
  ctx.rect(
    DEPLOY_SCRIPT.x + 1,
    DEPLOY_SCRIPT.y + FILE_ART.tabH,
    DEPLOY_SCRIPT.w - 2,
    FILE_H - FILE_ART.tabH - 1,
  );
  ctx.clip();
  gridRuns(ctx, shellLine(line), x0 - skip * adv, y, size, {
    // Faded in from the card's left edge: the head of the line is off it.
    fade: skip ? [x0 + 3 * adv, x0 - adv] : null,
  });
  ctx.restore();
  // `token` outlined where the line names it, when that part is in view.
  const i = token === null ? -1 : line.indexOf(token);
  if (i < 0 || mark <= 0 || token === null) return;
  const col = Array.from(line.slice(0, i)).length - skip;
  if (col < 3) return;
  // An outline only (the seat mark's paper stroke), over the text.
  ctx.save();
  ctx.globalAlpha *= mark;
  roundedRect(
    ctx,
    x0 + col * adv - 6,
    y - 0.8 * size - 6,
    Array.from(token).length * adv + 12,
    size + 12,
    RADIUS.seat,
  );
  ctx.strokeStyle = COLOR.mark;
  ctx.lineWidth = STROKE.seat;
  ctx.stroke();
  ctx.restore();
}

/** A shell line in a file card's colours: text1, as ART.md §8 sets shell files. */
const shellLine = (line: string) => [run(line, PALETTE.text1)];

/** The score's cues (score/cues.ts), where the picture has them. */
export const cues: SceneCues<"redact"> = (facts) => {
  const s = schedule(reelData(facts));
  if (!s) return {};
  const B = s.beats;
  return {
    zoom: B.zoom.at,
    // The new line lands in its seat.
    rewrite: B.rewrite.end,
    cards: [B.files.at, B.files.at + FILES_GAP],
    redacted: B.redacted,
    // The thunk on the stamp's landing, not its approach (kit STAMP_LAND).
    stamp: B.stamp.at + DUR.thread + STAMP_LAND,
    fold: B.back.at + BACK.fold,
  };
};
