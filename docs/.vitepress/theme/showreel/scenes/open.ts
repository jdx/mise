// open (STORYBOARD.md Act 0, ART.md §11 Open, "Pacing"): the reel starts
// from night. As the lamp comes up a terminal rises in at `~ $`, centred in
// the frame and sized to the eight rows it shows, `mise` is typed (at
// TYPE_RATE, a beat of anticipation before Enter) and the top of its real
// help screen prints (C1, rows 1-7, a viewport crop): the tagline, the
// usage line, the first command, and the green block chef beside them. A
// quarter beat later the pane dims round the tagline and the block chef,
// so the hold has a subject, and it pushes in slowly (1 to 1.05, glide)
// while the screen is read. Then one motion at a time, half a beat apart
// (kit/pace.ts): the block chef's cells dip, lift off the screen and fly
// into a mosaic at the chef's place, green turning to paper, while the
// block chef's own glyphs fade; the vector chef shows through them and
// resolves (M1's F), blooming. The tagline lifts out of the pane into the
// name card, crossing from mono to Space Grotesk glyph on glyph, as the
// pane falls away; then "mise-en-place" writes on above it, a syllable a
// quarter beat. The card and the chef hold two seconds, then leave (rising
// 32 px, 3/8 beat), and the pitch's terminal rises in at `~/work $` and
// holds through the bar line.
//
// Copies lift, originals stay: the block chef's cells and the tagline that
// fly are copies (kit/chef.ts, scenes/g1-brand/open-card.ts); the pane
// keeps its own, dimmed, until it leaves. The chef's own beats are the
// kit's (kit/chef.ts OPEN_CUE), drawn on this schedule's clock: shifted so
// its dip falls on the `lift` event.

import { H, type LitRect, PALETTE, type ReelFacts, TERM, W } from "../bible";
import {
  type Capture,
  capture,
  type CaptureId,
  type ReelData,
  reelData,
  screenAt,
  stepOf,
} from "../captures";
import { mix, rgba } from "../color";
import { drawOpenChef, OPEN_CUE } from "../kit/chef";
import {
  clipTime,
  cut,
  greyScene,
  keep,
  missing,
  type Play,
  pushAt,
  pushed,
  pushedLit,
  type PushIn,
  rectOf,
  rest,
  splitRuns,
  step,
} from "../kit/grey";
import { enter, lerpRect, type Rect } from "../kit/motion";
import { OPEN_CARD_CUE, openCardRects } from "../kit/namecard";
import { GAP, Pace } from "../kit/pace";
import { restLit } from "../kit/rest";
import {
  CHEF_ART,
  DUR,
  EASE,
  MOTION,
  NAME_CARD,
  PANE_DIM,
  PANES,
} from "../kit/style";
import {
  drawTerm,
  drawTermLine,
  fitPane,
  lineText,
  type Pane,
  placePane,
  run,
  type Run,
  type TermLayout,
  termLayout,
  termLit,
} from "../kit/term";
import { lerp, progress } from "../math";
import type { SceneCues } from "../score/cues";
import {
  drawOpenNameCard,
  LEAVE_DUR,
  leaveOf,
  type TaglineFrom,
} from "./g1-brand/open-card";
import { perFacts, takeIn } from "./g2-tools/common";

/** The rows of the help screen the reel shows: the command line and rows 1-7. */
const ROWS = 8;
/**
 * The help screen: the solo terminal's top rows (C1's is 200 rows tall),
 * the window cut down to the eight it shows and centred in the frame (ART.md
 * §11 Open). The open has no caption, so the grid's actor band does not
 * bind it; at the full solo height it was a 570 px window holding 8 rows,
 * over an empty lower third, on the reel's first frames (the director
 * review's M7).
 */
const HELP: Pane = (() => {
  const p = fitPane({ ...PANES.solo, anchor: "top" }, ROWS);
  return placePane(p, { y: Math.round((H - p.h) / 2) });
})();
/** The tagline's row on the screen (`mise`'s first line of output). */
const TAG_ROW = CHEF_ART.block.row;
/** The block chef's rows and first column on the screen. */
const BLOCK_ROWS = [
  CHEF_ART.block.row,
  CHEF_ART.block.row + CHEF_ART.block.rows,
];
const BLOCK_COL = CHEF_ART.block.col;

// The open's beats (ART.md §11 Open), from its schedule; the chef's and
// the card's own motions are kit/chef.ts OPEN_CUE's and kit/namecard.ts's.

/** The lights come up from night, and the help pane rises in. */
const LIGHTS = DUR.half;
/** The chef's lift, from its dip to its resolve (kit/chef.ts OPEN_CUE). */
const LIFT = OPEN_CUE.resolved - OPEN_CUE.dip;
/** The name's three syllables, a quarter beat apart, each landing over a flick. */
const NAME_GAP = DUR.tick;
/** The name card and the chef hold this long once the name is whole (the opening is brisker: not a caption). */
const CARD_HOLD = 2;
/** The pane falls away over half a beat (glide, not leave: leave's late rush read as a two-frame pop at 30 fps). */
const PANE_OUT_DUR = DUR.half;
/**
 * The block chef's own glyphs stay bright through the dim and fade as their
 * copies fly, to this much of the dimmed text, so the eye follows the copies.
 */
const BLOCK_DIM = 0.45;
const PITCH_DUR = DUR.enter;

/**
 * When everything happens (kit/pace.ts): `mise` typed and its help held
 * for its read, then the chef, the tagline, the name, the leave and the
 * pitch's terminal, one at a time.
 */
const schedule = perFacts((d: ReelData | null) => {
  const c = takeIn(d, "C1");
  const p = new Pace("open", { d });
  const help = c
    ? p.term("help", (at) => [step(c, "help", at)], { lines: 2 })
    : { ...p.move("help", 1.24), plays: [] as Play[] };
  p.readTerm();
  const lift = p.move("lift", LIFT);
  const tagline = p.move("tagline", OPEN_CARD_CUE.liftDur);
  const name = p.move("name", 2 * NAME_GAP + DUR.flick);
  p.wait(name.end + CARD_HOLD + GAP);
  const leave = p.move("leave", DUR.flick + LEAVE_DUR);
  const pitchTerm = p.move("pitchTerm", PITCH_DUR);
  const plays: Play[] = c
    ? [cut(0, stepOf(c, "help").type), ...help.plays]
    : [];
  return {
    p,
    plays,
    printed: help.end,
    /** The chef's clock: its dip on the lift. */
    shift: lift.at - OPEN_CUE.dip,
    dip: lift.at,
    resolved: lift.end,
    card: {
      lift: tagline.at,
      name: [0, 1, 2].map(
        (i) => name.at + i * NAME_GAP,
      ) as unknown as readonly [number, number, number],
      leave: leave.at,
    },
    /** The pane falls away as the tagline's copy leaves its row. */
    paneOut: tagline.at - DUR.flick,
    /** The chef leaves 1/8 beat after the card, on the card's curve, so the stage empties right to left. */
    chefOut: leave.at + DUR.flick,
    /** The pitch's terminal rises in once the name card has gone, and holds through the bar line. */
    pitchIn: pitchTerm.at,
  };
});
type Schedule = ReturnType<typeof schedule>;

/**
 * The slow push-in over the help's hold (ART.md §13 `glide`), about the
 * pane's middle: from the print to the cells' dip, then held until the
 * pane has gone. The cells lift from the pushed-in glyphs (pushedCell), so
 * nothing jumps when they go.
 */
const pushOf = (S: Schedule): PushIn => ({
  from: S.printed,
  to: S.dip,
  scale: 1.05,
  focus: { x: HELP.x + HELP.w / 2, y: HELP.y + HELP.h / 2 },
});

/**
 * The lit screen while the name card is up: the card's lines and the chef,
 * one rect (x from the card's left edge, over the chef's place).
 */
const CARD_LIT: Rect = {
  x: NAME_CARD.x,
  y: CHEF_ART.place.y,
  w: CHEF_ART.place.x + CHEF_ART.place.w - NAME_CARD.x,
  h: CHEF_ART.place.h,
};

/** The help pane's presence at beat `b`: rising in with the lights, falling away as the tagline lifts. */
function paneAt(b: number, S: Schedule): { alpha: number; dy: number } {
  const i = enter(b, 0, LIGHTS);
  const o = EASE.glide(progress(S.paneOut, S.paneOut + PANE_OUT_DUR, b));
  return { alpha: i.alpha * (1 - o), dy: i.dy + MOTION.fallOut * o };
}

/** How far the pane has dimmed (PANE_DIM) round its tagline: a quarter beat after the help has printed. */
const dimAt = (b: number, S: Schedule): number =>
  EASE.glide(progress(S.printed + DUR.tick, S.printed + DUR.tick + DUR.dim, b));

/** The block chef's own glyphs: bright, then falling to the dimmed text's BLOCK_DIM as their copies fly. */
const blockAt = (b: number, S: Schedule): number =>
  lerp(
    1,
    PANE_DIM.text * BLOCK_DIM,
    EASE.glide(
      progress(OPEN_CUE.fly + S.shift, OPEN_CUE.resolved + S.shift, b),
    ),
  );

/** A rect of the help pane where the push-in has it at beat `b`. */
function pushRect(r: Rect, b: number, push: PushIn): Rect {
  const k = pushAt(push, b);
  const f = push.focus;
  return {
    x: f.x + (r.x - f.x) * k,
    y: f.y + (r.y - f.y) * k,
    w: r.w * k,
    h: r.h * k,
  };
}

/** The help pane's lit screen at beat `b`, pushed in with it. */
const helpLit = (b: number, alpha: number, push: PushIn): LitRect | null =>
  pushedLit(termLit(rectOf(HELP), alpha), push, b);

/** Where the tagline's text ends on its row: the first run of two spaces. */
const tagCols = (line: readonly Run[]): number =>
  Math.max(0, lineText(line).search(/\s{2,}|$/));

/**
 * The help screen at beat `b` in HELP (moved `dy`): every row dimmed with
 * the pane but the tagline's text, which stays bright until its copy
 * lifts; the block chef's own glyphs drawn apart, fading further as their
 * copies fly. Returns the pane's layout (for the cells' flight).
 */
function drawHelp(
  ctx: CanvasRenderingContext2D,
  c: Capture,
  S: Schedule,
  b: number,
  t: number,
  alpha: number,
): TermLayout {
  const { ct } = clipTime(S.plays, b);
  const sc = screenAt(c, ct, { top: ROWS });
  const dim = dimAt(b, S);
  const inBlock = (i: number) => i >= BLOCK_ROWS[0] && i < BLOCK_ROWS[1];
  const parts = sc.lines.map((l, i) => {
    if (!inBlock(i)) return { text: l, tag: null, block: null };
    const [left, block] = splitRuns(l, BLOCK_COL);
    if (i !== TAG_ROW) return { text: left, tag: null, block };
    const n = tagCols(left);
    const [tag, tail] = splitRuns(left, n);
    return { text: [run(" ".repeat(n)), ...tail], tag, block };
  });
  const L = drawTerm(
    ctx,
    HELP,
    parts.map((p) => p.text),
    { t, alpha, dim, cursor: sc.cursor ?? undefined },
  );
  const tagK = progress(S.card.lift, S.card.lift + DUR.flick, b);
  const blockA = blockAt(b, S);
  ctx.save();
  ctx.beginPath();
  ctx.rect(L.body.x, L.body.y, L.body.w, L.body.h);
  ctx.clip();
  parts.forEach((p, i) => {
    const line = (runs: Run[], col: number, a: number) => {
      if (a <= 0 || !runs.length) return;
      ctx.save();
      ctx.globalAlpha *= alpha * a;
      drawTermLine(ctx, runs, L.col(col), L.baseline(i), HELP.size, {
        t,
        lineH: HELP.lineH,
      });
      ctx.restore();
    };
    // Bright while the pane dims round it; dimmed with the rest as its
    // copy lifts out, so the line never reads twice.
    if (p.tag) line(p.tag, 0, lerp(1, PANE_DIM.text, tagK));
    if (p.block) line(p.block, BLOCK_COL, blockA);
  });
  ctx.restore();
  return L;
}

export const scene = greyScene(
  "open",
  (g) => {
    const { ctx, b } = g;
    // Night over the stage, lifting as the lamp comes up.
    const night = 1 - EASE.glide(progress(0, LIGHTS, b));
    if (night > 0) {
      ctx.save();
      ctx.globalAlpha *= night;
      ctx.fillStyle = PALETTE.night;
      ctx.fillRect(0, 0, W, H);
      ctx.restore();
    }
    const c = capture(g.d, "C1");
    const S = schedule(g.d);
    const PUSH = pushOf(S);
    const pane = paneAt(b, S);
    let layout = termLayout(HELP, ROWS);
    // The tagline as the help screen printed it, for the card to lift:
    // from its row where the pane stands (pushed in) as the lift starts.
    let from: TaglineFrom | null = null;
    if (c) {
      const help = screenAt(c, stepOf(c, "help").end, { top: ROWS });
      const row = help.lines[TAG_ROW];
      if (row) {
        const n = tagCols(row);
        const lift = S.card.lift;
        const k = pushAt(PUSH, lift);
        const f = PUSH.focus;
        const y0 = layout.baseline(TAG_ROW) + paneAt(lift, S).dy;
        from = {
          text: lineText(row).slice(0, n),
          x: f.x + (HELP.x0 - f.x) * k,
          y: f.y + (y0 - f.y) * k,
          size: HELP.size * k,
          color: row[0]?.color ?? TERM.text,
        };
      }
    }
    if (pane.alpha > 0)
      pushed(g, PUSH, () => {
        ctx.save();
        ctx.translate(0, pane.dy);
        if (c) layout = drawHelp(ctx, c, S, b, g.t, pane.alpha);
        else missing(ctx, rectOf(HELP), "C1", pane.alpha);
        ctx.restore();
      });
    // The block chef's cells lift from where the push-in has the glyphs.
    const cells = layout;
    const pushedCell = (i: number, col: number): Rect =>
      pushRect(cells.cell(i, col), b, PUSH);
    // The chef: the block chef's cells lifting into the vector chef, which
    // blooms and holds, then leaves 1/8 beat after the card. Its dark
    // lines are backed by the pane's body while the pane is behind it.
    // The kit's own leave is replaced by the card's (drawn at the beat it
    // starts from, which is the chef at rest, halo settled, motes on
    // global time).
    const out = leaveOf(b, S.chefOut);
    ctx.save();
    ctx.globalAlpha *= out.alpha;
    ctx.translate(0, out.dy);
    if (out.alpha > 0)
      drawOpenChef(ctx, Math.min(b - S.shift, OPEN_CUE.leave), {
        cell: pushedCell,
        t: g.t,
        avoid: [
          ...openCardRects(ctx),
          ...(pane.alpha > 0 ? [pushRect(rectOf(HELP), b, PUSH)] : []),
        ],
        backing:
          pane.alpha > 0
            ? rgba(
                mix(TERM.window, PALETTE.bg, PANE_DIM.body * dimAt(b, S)),
                pane.alpha,
              )
            : null,
      });
    ctx.restore();
    // The name card: the tagline lifting into it, the name writing on.
    drawOpenNameCard(ctx, b, from, S.card);
    // The pitch's terminal rises in at `~/work $` and holds through the bar line.
    const pitch = enter(b, S.pitchIn, PITCH_DUR);
    if (pitch.alpha > 0)
      keep(g, () => {
        ctx.save();
        ctx.translate(0, pitch.dy);
        rest(g, "open|pitch", pitch.alpha);
        ctx.restore();
      });
  },
  {
    // The help pane while it is up; then the name card and the chef; then
    // the pitch's terminal, exactly open|pitch's once it has risen in.
    lit: (b, d) => {
      const S = schedule(d);
      const PUSH = pushOf(S);
      if (b < S.paneOut) {
        const a = paneAt(b, S).alpha;
        return a > 0 ? helpLit(b, a, PUSH) : null;
      }
      const z = restLit("open|pitch");
      const k2 = EASE.move(progress(S.card.leave, S.pitchIn + PITCH_DUR, b));
      if (!z || k2 >= 1) return z;
      const k1 = EASE.move(progress(S.paneOut, S.paneOut + PANE_OUT_DUR, b));
      const help = helpLit(S.paneOut, 1, PUSH) ?? termLit(rectOf(HELP));
      const r = lerpRect(lerpRect(help, CARD_LIT, k1), z, k2);
      return { ...r, alpha: 1 };
    },
    events: (d) => schedule(d).p.events(),
  },
);

/** The score's cues (score/cues.ts LISTEN.open), from the schedule. */
export const cues: SceneCues<"open"> = (facts: ReelFacts | null) => {
  const S = schedule(reelData(facts));
  return {
    lift: S.dip,
    resolve: S.resolved,
    name: S.card.name,
    leave: S.card.leave,
  };
};

/** The section's schedule (kit/pace.ts Pace) and its takes' plays, for the pacing checks. */
export const pacing = (
  facts: ReelFacts | null,
): { pace: Pace; takes: Partial<Record<CaptureId, readonly Play[]>> } => {
  const S = schedule(reelData(facts));
  return { pace: S.p, takes: { C1: S.plays } };
};
