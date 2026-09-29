// The ticket rail and its tickets (ART.md §10): the one kitchen device
// besides the chef. Each act opens on a thermal-paper order ticket that
// prints out of a steel rail across the top band, tears, swings on its
// clip, and lifts off with the rail as the act's first stage comes in
// under it. A ticket carries its act's number as meta ("ACT I"), a band in
// the act's pillar colour with the act's chip in mono ink (`[tools]`), and
// the act's name in Cormorant Garamond italic.
//
// There is no chapter marker that outlives the ticket: the rail lifts off
// with it and the stage it leaves is the next section's rest (kit/rest.ts),
// so nothing here crosses a bar line. The player's chapters track and the
// page's chapter list carry the act names from then on (timeline.ts).
//
// Pure functions of the ticket section's local beat; the numbers come from
// kit/style.ts (TICKET, COLOR, TYPE, EASE, DUR).

import {
  type ActId,
  actOf,
  BEAT,
  PALETTE,
  PILLAR,
  type Pillar,
  type SectionId,
  sec,
} from "../bible";
import { rgba } from "../color";
import { castShadow, roundedRect } from "../fx";
import { clamp, DEG, lerp, progress, smoothstep, TAU } from "../math";
import { drawText, font, layout, MONO, serifItalic } from "../type";
import type { Rect } from "./motion";
import {
  COLOR,
  DASH,
  DUR,
  EASE,
  MOTION,
  RADIUS,
  SHADOW,
  TICKET,
  TYPE,
} from "./style";

/** Each act's chip on its ticket's band (STORYBOARD.md); "" for a band with no chip. */
export const TICKET_CHIPS: Readonly<Record<ActId, string>> = {
  mise: "",
  "dev-tools": "[tools]",
  versions: "tool@version",
  environments: "[env]",
  tasks: "[tasks]",
  dotfiles: "[dotfiles]",
  everywhere: "mise.lock",
  "new-machine": "mise bootstrap",
  install: "",
};

/** Tickets that hang beside a terminal on the narrow rail, not across the stage. */
const NARROW: readonly SectionId[] = ["new"];

/** What a ticket says, and where it hangs. */
export interface TicketSpec {
  /** "ACT I" … "ACT VII". */
  meta: string;
  /** The band's chip in mono ("[tools]"), or "" for a bare band. */
  chip: string;
  /** The act's name: the title. */
  title: string;
  pillar: Pillar;
  /** Beside the terminal (TICKET.narrow on TICKET.railNarrow). */
  narrow: boolean;
}

/** Ticket section `id`'s ticket, from its act (timeline.ts ACTS): its numeral, name and pillar. */
export function ticketFor(id: SectionId): TicketSpec {
  const s = sec(id);
  const act = actOf(id);
  if (!s.ticket || !act.pillar)
    throw new Error(`section "${id}" is not a ticket`);
  return {
    meta: `ACT ${act.numeral}`,
    chip: TICKET_CHIPS[act.id],
    title: act.label,
    pillar: act.pillar,
    narrow: NARROW.includes(id),
  };
}

/** The rail and the ticket's hanging place for a spec. */
export function ticketPlace(narrow: boolean): { rail: Rect; paper: Rect } {
  return narrow
    ? { rail: TICKET.railNarrow, paper: TICKET.narrow }
    : { rail: TICKET.rail, paper: TICKET.ticket };
}

/** How far the tear drops the ticket, px (ART.md §10). */
const TEAR_DROP = 8;
/** The rail comes down from here, px: its top, above the frame. */
const RAIL_FROM = -40;
/** Past the frame's top a lifting ticket clears, px: its float shadow's reach and its 3° turn. */
const LIFT_CLEAR = 100;
/** The clip's block: this far above the rail's top, and short of the paper's top. */
const CLIP_OVER = 4;
const CLIP_SHORT = 2;

/** The printer's steps, section beats: one printer tick each (12 on the 1/16 grid, 0 to 11/16). */
export const PRINT_TICKS: readonly number[] = Array.from(
  { length: TICKET.feedSteps },
  (_, k) => (k * TICKET.cue.tear) / TICKET.feedSteps,
);

/** The ticket at a moment: what drawTicket draws, as numbers. */
export interface TicketPose {
  /** The rail's top, px (TICKET.rail.y at rest). */
  railY: number;
  /** The paper's offset from its hanging place, px: inside the rail while it feeds, 0 hanging, rising as it lifts. */
  dy: number;
  /** The swing and lift-off turn about the clip, degrees (positive clockwise). */
  rot: number;
  /** The bottom edge is torn to teeth. */
  torn: boolean;
  /** The tear's flash, 1 to 0 over 1/8 beat. */
  flash: number;
  /**
   * The whole assembly's opacity: 1 while any of the paper is on the frame
   * (it prints and lifts off as a solid object), fading only once its foot
   * has left the frame's top.
   */
  alpha: number;
  /** How far the feed has come, 0 to 1. */
  feed: number;
}

export interface TicketOptions {
  /** The rail is already up at beat 0 (a ticket straight after a ticket): it does not slide in. */
  railUp?: boolean;
  narrow?: boolean;
}

/**
 * The ticket at section beat `b` (ART.md §10's cues, TICKET.cue): the rail
 * slides down over 1/4 beat; the paper feeds out of it in 12 printer steps
 * to the tear at 3/4; the tear drops it 8 px and it swings on its clip,
 * still by 1.1; from 1.2 it dips (anticipation, 1/8 beat) and lifts off
 * with the rail, clear of the frame's top by 1.8, turning 3°. Null when
 * nothing shows: at 0 and from 1.8, so both bar lines are clear.
 */
export function ticketPose(
  b: number,
  o: TicketOptions = {},
): TicketPose | null {
  const C = TICKET.cue;
  const { rail, paper } = ticketPlace(o.narrow ?? false);
  if (b <= 0) return null;
  // The rail, and everything it carries: sliding down, then lifting off
  // after a 1/8-beat dip, turning the paper 3° and fading as it goes.
  const railIn = o.railUp ? 1 : EASE.arrive(progress(0, DUR.tick, b));
  const dip =
    MOTION.anticipate *
    EASE.wind(progress(C.liftAt, C.liftAt + DUR.anticipate, b));
  const u = progress(C.liftAt + DUR.anticipate, C.liftEnd, b);
  // TICKET.lift.dy (460) leaves a hanging ticket's foot on the frame (it
  // hangs to y 496), so it rises at least far enough to clear the top with
  // its shadow and its turn. It stays opaque while any of the paper is on
  // the frame, so it passes over the stage coming in under it as a solid
  // object (a fade while it still covered the incoming chrome read as a
  // translucent grey sheet), and fades only once its foot has left the
  // frame's top: the shadow and the turn's low corner go with it.
  const rise = Math.min(TICKET.lift.dy, -(paper.y + paper.h + LIFT_CLEAR));
  const lift = b >= C.liftAt ? lerp(dip, rise, EASE.leave(u)) : 0;
  const railY = lerp(RAIL_FROM, rail.y, railIn) + lift;
  const turn = TICKET.lift.deg * EASE.leave(u);
  // The paper's lowest point: its foot, and the corner the turn drops.
  const foot =
    paper.y +
    paper.h +
    (b >= C.liftAt ? railY - rail.y : 0) +
    (paper.w / 2) * Math.sin(turn * DEG);
  const alpha = u >= 1 ? 0 : 1 - smoothstep(0, LIFT_CLEAR / 2, -foot);
  if (alpha <= 0) return null;
  // The paper on the rail: its bottom edge at the rail's foot at 0, fed out
  // in 12 printer steps to TEAR_DROP short of its hanging place at the
  // tear, then dropped onto it.
  const feed = EASE.print(progress(0, C.tear, b));
  const torn = b >= C.tear;
  const inside = rail.y + rail.h - (paper.y + paper.h);
  const hang = torn
    ? -TEAR_DROP * (1 - EASE.settle(progress(C.tear, C.tear + DUR.flick, b)))
    : lerp(inside, -TEAR_DROP, feed);
  // The swing: a damped pendulum from the tear (hk's swingDeg at twice its
  // rate, a half-beat period, so it swings out and back before the lift),
  // eased to rest by 1.1 so the title reads still, and the lift's turn on top.
  const d = (b - C.tear) * BEAT;
  const swing =
    d > 0
      ? TICKET.swing.deg *
        Math.exp(-d / TICKET.swing.tau) *
        Math.sin((TAU * d) / (BEAT / 2)) *
        (1 - smoothstep(0.95, 1.1, b))
      : 0;
  return {
    railY,
    dy: railY - rail.y + hang,
    rot: swing + turn,
    torn,
    flash: torn ? 1 - progress(C.tear, C.tear + DUR.flick, b) : 0,
    alpha,
    feed,
  };
}

/** The steel rail at `r` (its top at `y`): a rounded bar lit from above, casting SHADOW.small. */
export function drawRail(
  ctx: CanvasRenderingContext2D,
  r: Rect,
  y: number = r.y,
): void {
  castShadow(ctx, SHADOW.small, () =>
    roundedRect(ctx, r.x, y, r.w, r.h, r.h / 2),
  );
  const g = ctx.createLinearGradient(0, y, 0, y + r.h);
  g.addColorStop(0, COLOR.steelHi);
  g.addColorStop(0.45, COLOR.steel);
  g.addColorStop(1, COLOR.steelLo);
  ctx.save();
  roundedRect(ctx, r.x, y, r.w, r.h, r.h / 2);
  ctx.fillStyle = g;
  ctx.fill();
  ctx.fillStyle = rgba(PALETTE.paper, 0.18);
  ctx.fillRect(r.x + r.h / 2, y + 1, r.w - r.h, 1);
  ctx.restore();
}

/** The paper's outline at `p`: round top corners, and a straight or torn bottom edge. */
function paperPath(
  ctx: CanvasRenderingContext2D,
  p: Rect,
  torn: boolean,
): void {
  const r = RADIUS.ticket;
  const T = TICKET.teeth;
  const bottom = p.y + p.h;
  ctx.beginPath();
  ctx.moveTo(p.x, bottom);
  ctx.lineTo(p.x, p.y + r);
  ctx.arcTo(p.x, p.y, p.x + r, p.y, r);
  ctx.lineTo(p.x + p.w - r, p.y);
  ctx.arcTo(p.x + p.w, p.y, p.x + p.w, p.y + r, r);
  ctx.lineTo(p.x + p.w, bottom);
  if (torn) tearPath(ctx, p, T, false);
  ctx.closePath();
}

/** The teeth along the bottom edge, right to left (continuing a path), or as a stroke of its own. */
function tearPath(
  ctx: CanvasRenderingContext2D,
  p: Rect,
  T: { w: number; h: number },
  fresh: boolean,
): void {
  const n = 2 * Math.max(1, Math.round(p.w / (2 * T.w)));
  const bottom = p.y + p.h;
  if (fresh) {
    ctx.beginPath();
    ctx.moveTo(p.x + p.w, bottom);
  }
  for (let i = n - 1; i >= 0; i--)
    ctx.lineTo(p.x + (i * p.w) / n, bottom - ((n - i) % 2 ? T.h : 0));
}

/**
 * How wide a title may set on a ticket `w` wide: the band's width less half
 * an inset of air at either end, so no title runs edge to edge.
 */
export const titleRoom = (w: number): number => w - 3 * TICKET.band.inset;

/** A title set in Cormorant italic at `size`, shrunk to fit `maxW` if it must. */
export function titleFont(
  ctx: CanvasRenderingContext2D,
  text: string,
  size: number,
  maxW: number,
): string {
  const f = serifItalic(size, TYPE.ticket.titleWeight);
  const w = layout(ctx, text, f).width;
  return w <= maxW
    ? f
    : serifItalic(Math.floor((size * maxW) / w), TYPE.ticket.titleWeight);
}

/**
 * Draw ticket `spec` at section beat `b` (ticketPose): the paper and its
 * print behind the rail, the rail over its top, and the clip that holds
 * it. Nothing at beat 0 or from 1.8.
 */
export function drawTicket(
  ctx: CanvasRenderingContext2D,
  b: number,
  spec: TicketSpec,
  o: TicketOptions = {},
): void {
  const pose = ticketPose(b, { ...o, narrow: spec.narrow });
  if (!pose) return;
  const { rail, paper } = ticketPlace(spec.narrow);
  const p: Rect = { ...paper, y: paper.y + pose.dy };
  const cx = p.x + p.w / 2;
  const pivot = { x: cx, y: pose.railY + rail.h / 2 };
  const B = TICKET.band;
  ctx.save();
  ctx.globalAlpha *= pose.alpha;
  // The paper, where it has come out of the rail (below its middle).
  ctx.save();
  ctx.beginPath();
  ctx.rect(p.x - 400, pivot.y, p.w + 800, 2000);
  ctx.clip();
  ctx.translate(pivot.x, pivot.y);
  ctx.rotate(pose.rot * DEG);
  ctx.translate(-pivot.x, -pivot.y);
  castShadow(ctx, SHADOW.float, () => paperPath(ctx, p, pose.torn));
  paperPath(ctx, p, pose.torn);
  const pg = ctx.createLinearGradient(0, p.y, 0, p.y + p.h);
  pg.addColorStop(0, COLOR.ticketTop);
  pg.addColorStop(1, COLOR.ticketBottom);
  ctx.fillStyle = pg;
  ctx.fill();
  ctx.save();
  ctx.clip();
  // Meta, band, title, perforation.
  drawText(ctx, spec.meta, p.x + B.inset, p.y + TICKET.metaY, {
    font: font(TYPE.meta.size, TYPE.meta.weight),
    fill: rgba(COLOR.ink, 0.62),
    tracking: TYPE.meta.tracking * TYPE.meta.size,
  });
  ctx.fillStyle = PILLAR[spec.pillar];
  ctx.fillRect(p.x + B.inset, p.y + B.y, p.w - 2 * B.inset, B.h);
  if (spec.chip)
    drawText(
      ctx,
      spec.chip,
      cx,
      p.y + B.y + B.h / 2 + 0.36 * TYPE.ticket.band,
      {
        font: font(TYPE.ticket.band, TYPE.ticket.bandWeight, MONO),
        fill: COLOR.ink,
        align: "center",
      },
    );
  const size = spec.narrow ? TYPE.ticket.titleNarrow : TYPE.ticket.title;
  drawText(ctx, spec.title, cx, p.y + TICKET.titleY, {
    font: titleFont(ctx, spec.title, size, titleRoom(p.w)),
    fill: COLOR.ink,
    align: "center",
  });
  ctx.setLineDash([...DASH.perf]);
  ctx.strokeStyle = rgba(COLOR.ink, 0.25);
  ctx.lineWidth = 1.5;
  ctx.beginPath();
  ctx.moveTo(p.x + B.inset, p.y + TICKET.perfY);
  ctx.lineTo(p.x + p.w - B.inset, p.y + TICKET.perfY);
  ctx.stroke();
  ctx.restore();
  // The tear's flash along the teeth.
  if (pose.flash > 0) {
    tearPath(ctx, p, TICKET.teeth, true);
    ctx.setLineDash([]);
    ctx.lineJoin = "round";
    ctx.strokeStyle = rgba(COLOR.glint, 0.9 * pose.flash);
    ctx.lineWidth = 2;
    ctx.stroke();
  }
  ctx.restore();
  // The rail over the paper's top, and the clip on it that holds the paper.
  drawRail(ctx, rail, pose.railY);
  ctx.save();
  const cl = TICKET.clip;
  const top = pose.railY - CLIP_OVER;
  const h = paper.y - CLIP_SHORT - (rail.y - CLIP_OVER);
  roundedRect(ctx, cx - cl.w / 2, top, cl.w, h, 4);
  ctx.fillStyle = COLOR.steelHi;
  ctx.fill();
  ctx.fillStyle = COLOR.steelLo;
  ctx.fillRect(cx - cl.w / 2 + 6, top + h - 8, cl.w - 12, 3);
  ctx.restore();
  ctx.restore();
}

/** The ticket for section `id` at its beat `b`: ticketFor and drawTicket in one. */
export function drawTicketFor(
  ctx: CanvasRenderingContext2D,
  id: SectionId,
  b: number,
  o: Omit<TicketOptions, "narrow"> = {},
): void {
  drawTicket(ctx, b, ticketFor(id), o);
}

/** 0 to 1: how far the ticket has lifted off at beat `b` (for a scene timing what comes in under it). */
export const ticketLift = (b: number): number =>
  clamp(
    EASE.leave(
      progress(TICKET.cue.liftAt + DUR.anticipate, TICKET.cue.liftEnd, b),
    ),
  );
