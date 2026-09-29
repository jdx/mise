// from jdx/hk@37937824 docs/.vitepress/theme/showreel/whip.ts
// The whip pan from `lock` into `new` (plan v2, Act VI into Act VII), the
// one bar line crossed in motion. The outgoing content winds up to the
// right, then whips out to the left, smeared along its motion; speed lines
// race left, peaking on the bar line, whose frame is the streaks alone on
// the stage; the next content arrives from the right while they die out.
// Every function here is a pure function of GLOBAL time, so both neighbours
// evaluate the same move and the cut is seamless.
//
// Adapted for mise: the bar line is lock|new, and the streaks are machine
// terracotta (the act the whip lands in) and the stage's neutrals, paperDim,
// text3 and paper: no other pillar's colour (ART.md §2, §12.11).

import { PALETTE, W } from "./bible";
import { rgba } from "./color";
import { makeCanvas, smear } from "./fx";
import { hash, outQuart, progress, swiftOut } from "./math";
import { beat, sec } from "./timeline";

// Every time is read off new's own clock, `sec("new").beat(n)`, one exact
// division, so the whip's marks are the frames on them.
const NEW = sec("new");
/** The bar line the whip crosses: lock|new. */
export const WHIP_AT = NEW.start;
/** The whip out: the content leaves left over the last half beat, and the streaks build. */
export const WHIP_OUT = beat(0.5);
/** The whip in: the streaks die out and the next content arrives over the first beat. */
export const WHIP_IN = beat(1);
/** When the outgoing scene starts to wind up: 1.5 beats before the bar line (lock b20.5). */
export const WHIP_START = NEW.beat(-1.5);
/** When the outgoing content starts to leave: half a beat before the bar line. */
export const WHIP_LEAVE = NEW.beat(-0.5);
/** When the whip has cleared: a beat after the bar line (new b1). */
export const WHIP_END = NEW.beat(1);

/**
 * The outgoing content's x offset at global `t`: a 26 px wind-up right,
 * then out to the left over WHIP_OUT, accelerating, still in shot on the
 * frame before the bar line.
 */
export function whipOut(t: number): number {
  const wind = 26 * swiftOut(progress(WHIP_START, WHIP_LEAVE, t));
  return wind - (wind + 1970) * progress(WHIP_LEAVE, WHIP_AT, t) ** 1.7;
}

/** The incoming content's x offset: from 1800 px right on the bar line to rest at WHIP_END. */
export function whipIn(t: number): number {
  return 1800 * (1 - outQuart(progress(WHIP_AT, WHIP_END, t)));
}

/** Draw content whipping out: smeared along its motion at `t` (the smear trails to its right). */
export function drawWhipOut(
  ctx: CanvasRenderingContext2D,
  t: number,
  draw: () => void,
): void {
  const x = whipOut(t);
  const v = x - whipOut(t - 1 / 120);
  smear(ctx, x, v, draw, 6);
}

/** Draw content whipping in from the right, smeared along its motion at `t`. */
export function drawWhipIn(
  ctx: CanvasRenderingContext2D,
  t: number,
  draw: () => void,
): void {
  const x = whipIn(t);
  const v = x - whipIn(t - 1 / 120);
  smear(ctx, x, v, draw, 6);
}

let streakSprites: Map<string, HTMLCanvasElement> | null = null;
/** A streak of `color`: hot at its left end, trailing off right, soft top and bottom. */
function streakSprite(color: string): HTMLCanvasElement {
  streakSprites ??= new Map();
  let c = streakSprites.get(color);
  if (!c) {
    c = makeCanvas(128, 16);
    const g = c.getContext("2d")!;
    const gx = g.createLinearGradient(0, 0, 128, 0);
    gx.addColorStop(0, rgba(color, 0));
    gx.addColorStop(0.04, rgba(color, 1));
    gx.addColorStop(0.3, rgba(color, 0.55));
    gx.addColorStop(1, rgba(color, 0));
    g.fillStyle = gx;
    g.fillRect(0, 0, 128, 16);
    g.globalCompositeOperation = "destination-in";
    const gy = g.createLinearGradient(0, 0, 0, 16);
    gy.addColorStop(0, "rgba(0,0,0,0)");
    gy.addColorStop(0.35, "rgba(0,0,0,1)");
    gy.addColorStop(0.65, "rgba(0,0,0,1)");
    gy.addColorStop(1, "rgba(0,0,0,0)");
    g.fillStyle = gy;
    g.fillRect(0, 0, 128, 16);
    streakSprites.set(color, c);
  }
  return c;
}

/** Broad bands: y, height, colour, alpha, parallax. The machine leads (the act the whip lands in), the stage's neutrals between. */
const WHIP_BANDS: readonly (readonly [
  number,
  number,
  string,
  number,
  number,
])[] = [
  [230, 56, PALETTE.machine, 0.22, 0.85],
  [372, 46, PALETTE.paperDim, 0.16, 1.15],
  [520, 60, PALETTE.machine, 0.34, 0.95],
  [650, 44, PALETTE.text3, 0.26, 1.25],
  [860, 54, PALETTE.machine, 0.2, 1.05],
];
/** The speed lines' colours: machine, and the neutrals ART.md §12.11 allows. */
const LINE_COLORS = [
  PALETTE.machine,
  PALETTE.text3,
  PALETTE.paperDim,
  PALETTE.machine,
  PALETTE.paper,
  PALETTE.text3,
];

/**
 * The whip's speed lines at global `t`: they build over the last WHIP_OUT,
 * peak on the bar line and die out over WHIP_IN, racing left the whole
 * time. Nothing is drawn outside that window, so a scene may call it on any
 * frame. Both neighbours draw it over their content.
 */
export function drawWhip(ctx: CanvasRenderingContext2D, t: number): void {
  const d = t - WHIP_AT;
  const q = d < 0 ? d / WHIP_OUT : d / WHIP_IN;
  if (q <= -1 || q >= 1) return;
  const env = (1 - Math.abs(q)) ** 0.7;
  // Travel: fastest on the bar line.
  const g = 0.5 + 0.5 * Math.sin((Math.PI / 2) * q);
  ctx.save();
  ctx.globalCompositeOperation = "lighter";
  for (const [i, [y, h, color, a, par]] of WHIP_BANDS.entries()) {
    const len = 1700 * env * par;
    const x = 1500 - 2600 * g * par + hash(i, 5) * 400;
    if (x > W || x + len < 0) continue;
    ctx.globalAlpha = a * env;
    ctx.drawImage(streakSprite(color), x, y - h / 2, len, h);
  }
  const span = W + 1900;
  for (let i = 0; i < 44; i++) {
    const fat = hash(i, 61) < 0.2;
    const par = 0.7 + hash(i, 53) * 0.7;
    const len =
      ((fat ? 480 : 260) + hash(i, 43) * 620) * par * (0.2 + 0.8 * env);
    const u = (hash(i, 47) * span - 3200 * g * par) % span;
    const x = (u < 0 ? u + span : u) - (span - W);
    if (x > W || x + len < 0) continue;
    const y = 110 + hash(i, 41) * 860;
    const w = fat ? 10 + hash(i, 59) * 12 : 1.5 + hash(i, 59) * 3;
    ctx.globalAlpha =
      (fat ? 0.22 + hash(i, 71) * 0.16 : 0.45 + hash(i, 71) * 0.25) * env;
    ctx.drawImage(
      streakSprite(LINE_COLORS[Math.floor(hash(i, 67) * LINE_COLORS.length)]),
      x,
      y - w / 2,
      len,
      w,
    );
  }
  ctx.restore();
}
