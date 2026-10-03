// The kit the scenes draw with: every scene is `greyScene(id, draw)`, its
// terminal shots are the takes' real screens at the moment a scene maps
// them to (captures.ts), and its diagrams are made from the takes' own lines
// and files: the ticket on its rail, the lanes, the tiles, the rivers of
// registry names and the ledger, each in the production look (ART.md §10,
// §12, the numbers in kit/style.ts). Badges, keycaps, footnotes, chips,
// threads and cards are kit/parts.ts's.
//
// What stays on the stage across a bar line (kit/rest.ts: the station card,
// the terminal's window, the chef) a scene draws inside `keep`, at rest on
// its first and last frames, so it holds through the bar line on the
// handoff's frame (handoff.ts). Everything else fades in over the section's
// first sixteenth and out over its last (but in a ticket's section, whose
// ticket is off the frame on both bar lines by its own pose, and whose
// stage is all kept), and whips across lock|new. Every
// frame stands on the stage (fx.ts drawStage: the background and the pass
// lamp), and says which screen is lit (Scene.lit) so the vignette spares it:
// on a bar line, the one kit/rest.ts restLit names.
//
// A ticket's section hands the stage on (ticketStage): the stage it
// inherits leaves under the printing ticket and the next section's comes
// in under the lifting one.
//
// (The name greyScene is the animatic's, kept so a rename does not touch
// all 27 scenes at once; it draws the production stage.)

import {
  BEAT,
  H,
  type Join,
  type Joins,
  type LitRect,
  type Scene,
  type Section,
  type SectionId,
  sec,
  W,
} from "../bible";
import {
  type Capture,
  type CaptureId,
  capture,
  CUT_ON,
  frameAt,
  lastBefore,
  type ReelData,
  reelData,
  rowText,
  type Screen,
  type ScreenOptions,
  screenAt,
  stepOf,
} from "../captures";
import { rgba } from "../color";
import { drawStage } from "../fx";
import { type BoundaryId, handoffIn, handoffOut } from "../handoff";
import { progress, smoothstep } from "../math";
import { captionsFor, type SceneEvents } from "../storyboard";
import { drawWhip, drawWhipIn, drawWhipOut, WHIP_AT } from "../whip";
import { enter, exit, type Presence } from "./motion";
import { badgePop, missing, rectOf, win } from "./parts";
import {
  BADGES,
  drawLayer,
  drawPaneScreen,
  drawPaneWindow,
  orderBadges,
  fittedPane,
  restLit,
  RESTS,
  screenBadges,
  scrollOf,
  shownRows,
  type TakeScroll,
} from "./rest";
import { drawTicketFor } from "./ticket";
import { DUR, EASE, PACE, STAGE_FX, TICKET } from "./style";
import {
  type ChromeBadge,
  drawTerm,
  lineText,
  type Pane,
  type Run,
  type TermLayout,
} from "./term";

export * from "./parts";

/**
 * The bar line section `id` starts from in the reel being drawn: a film's
 * join (bible.ts Joins, film.ts) when it plays the section after another
 * than the source's, else the timeline's own (handoff.ts handoffIn). Null
 * for the first section.
 */
export function boundaryIn(id: SectionId, joins?: Joins): BoundaryId | null {
  return joins?.[id]?.in ?? handoffIn(id)?.id ?? null;
}

/** The bar line section `id` ends on (boundaryIn's counterpart; handoffOut). Null for the last. */
export function boundaryOut(id: SectionId, joins?: Joins): BoundaryId | null {
  return joins?.[id]?.out ?? handoffOut(id)?.id ?? null;
}

/** What a scene's layers draw with on each frame. */
export interface G {
  ctx: CanvasRenderingContext2D;
  /** Local seconds, and local beats. */
  lt: number;
  b: number;
  /** Global seconds. */
  t: number;
  d: ReelData | null;
  s: Section;
  /**
   * The bar lines the section starts from and ends on in this reel
   * (boundaryIn, boundaryOut): the stage a ticket takes down and the one it
   * brings in. A film that plays the sections out of order sets them
   * (SceneEnv.joins); the source reel's are the timeline's.
   */
  prev: BoundaryId | null;
  next: BoundaryId | null;
  /** The section's film join, if a film gave it one (SceneEnv.joins): a ticket's meta line. */
  join: Join | null;
  /** The edge fade the transient layers are drawn under (never 0: `keep` divides by it). */
  edge: number;
  /** How deep in `keep` the scene is drawing. */
  kept: number;
}

/** The last frame before a bar line at 120 fps, the finest render. */
const LAST = 1 / 120;
/** Transient layers fade in over a section's first sixteenth and out over its last. */
const EDGE = BEAT / 4;
/** The smallest edge fade: invisible, and still a divisor. */
const FLOOR = 1e-6;

export interface SceneOptions {
  /**
   * The lit screen at local beat `b` (Scene.lit): the terminal's window
   * (or the chef) the vignette spares, following the pane's own alpha as
   * it enters and leaves. It must be the bar lines' (kit/rest.ts restLit)
   * on the section's first frame and its last. Without it, sectionLit. A
   * scene whose lit screen is a bar line's reads the bar line from `joins`
   * (boundaryIn, boundaryOut), as `G.prev` and `G.next` do.
   */
  lit?: (b: number, d: ReelData | null, joins?: Joins) => LitRect | null;
  /**
   * Whether the transient layers take the section's edge fade (in over
   * its first sixteenth, out over its last). Default: every section but a
   * ticket. A ticket's section draws everything but its ticket kept
   * (ticketStage), and the ticket is off the frame on both bar lines by
   * its own pose (kit/ticket.ts), so the fade would only have made the
   * printing paper a translucent grey slab over the stage it inherits.
   */
  edge?: boolean;
  /**
   * Layers drawn over the whip: after the section's layers, outside the
   * whip's move and smear (drawWhipIn/Out) and over its streaks, under the
   * same edge fade as the rest. For a device that is not part of the
   * whipping stage: new's narrow ticket comes down in place on its rail
   * while the terminal whips in under it (drawSectionTicket). Elsewhere
   * it is drawn straight after `draw`.
   */
  over?: (g: G) => void;
  /**
   * The scene's events on its beats, from the facts (kit/pace.ts
   * Pace.events): when each is on screen and still. A caption whose
   * storyboard entry names one (`after`) starts PACE.gap after it
   * (storyboard.ts captionTimes), so a caption never arrives before what
   * it explains, whatever the take's timing.
   */
  events?: (d: ReelData | null) => SceneEvents | null;
}

const sameLit = (a: LitRect | null, z: LitRect | null): boolean =>
  a === z ||
  (a !== null &&
    z !== null &&
    a.x === z.x &&
    a.y === z.y &&
    a.w === z.w &&
    a.h === z.h &&
    a.alpha === z.alpha);

const litAt = (r: LitRect | null, k: number): LitRect | null =>
  r && k > 0 ? { ...r, alpha: r.alpha * Math.min(1, k) } : null;

/**
 * A section's lit screen at local beat `b` when its scene names none: a
 * ticket's follows ticketStage (the inherited window leaving, the next one
 * arriving); any other holds its bar lines' when both are the same, else
 * lets the first go over the beat before its middle and brings the second
 * up over the beat after.
 */
export function sectionLit(
  id: SectionId,
  b: number,
  joins?: Joins,
): LitRect | null {
  const s = sec(id);
  const hin = boundaryIn(id, joins);
  const hout = boundaryOut(id, joins);
  const a = hin ? restLit(hin) : null;
  const z = hout ? restLit(hout) : null;
  if (s.ticket) {
    const inK = hin ? stagePresence(hin, "leave", b) : [];
    const outK = hout ? stagePresence(hout, "enter", b) : [];
    const lit = (id2: BoundaryId | null, ks: Presence[]) => {
      if (!id2) return 1;
      const i = RESTS[id2].findIndex(
        (l) => l.kind === "pane" || l.kind === "chef",
      );
      return i < 0 ? 0 : ks[i].alpha;
    };
    const zk = lit(hout, outK);
    if (z && zk > 0) return litAt(z, zk);
    return litAt(a, lit(hin, inK));
  }
  if (sameLit(a, z)) return a;
  const mid = s.beats / 2;
  return b < mid
    ? litAt(a, 1 - smoothstep(mid - 1, mid, b))
    : litAt(z, smoothstep(mid, mid + 1, b));
}

/**
 * A scene for section `id`: the stage (background and pass lamp), the
 * section's captions from the storyboard, and `draw`'s layers over it.
 * Transient layers are gone on the first frame and the last (where the bar
 * line holds); layers drawn inside `keep` are not faded, and meet the bar
 * line's persistent layers there.
 */
export function greyScene(
  id: SectionId,
  draw: (g: G) => void,
  o: SceneOptions = {},
): Scene {
  const s = sec(id);
  const edged = o.edge ?? !s.ticket;
  const fadeIn = edged && handoffIn(id) !== null;
  const fadeOut = edged && handoffOut(id) !== null;
  return {
    id,
    start: s.start,
    end: s.end,
    captions: (facts) => {
      const d = reelData(facts);
      return captionsFor(id, d, o.events?.(d));
    },
    lit: (lt, env) =>
      o.lit
        ? o.lit(lt / BEAT, reelData(env.facts), env.joins)
        : sectionLit(id, lt / BEAT, env.joins),
    draw(ctx, lt, env) {
      drawStage(ctx, env.W, env.H, env.t);
      const a =
        (fadeIn ? smoothstep(0, EDGE, lt) : 1) *
        (fadeOut ? smoothstep(0, EDGE, s.len - LAST - lt) : 1);
      const edge = Math.max(a, FLOOR);
      const g: G = {
        ctx,
        lt,
        b: lt / BEAT,
        t: env.t,
        d: reelData(env.facts),
        s,
        prev: boundaryIn(id, env.joins),
        next: boundaryOut(id, env.joins),
        join: env.joins?.[id] ?? null,
        edge,
        kept: 0,
      };
      const under = (fn: (g: G) => void) => () => {
        ctx.save();
        ctx.globalAlpha *= edge;
        fn(g);
        ctx.restore();
      };
      const layers = under(draw);
      if (s.end === WHIP_AT) drawWhipOut(ctx, env.t, layers);
      else if (s.start === WHIP_AT) drawWhipIn(ctx, env.t, layers);
      else layers();
      drawWhip(ctx, env.t);
      if (o.over) under(o.over)();
    },
  };
}

/**
 * Draw a persistent layer: out from under the section's edge fade, so it
 * holds through the bar line. Any other alpha (the whip's smear, a
 * scene's own fade) still applies.
 */
export function keep<T>(g: G, fn: () => T): T {
  const { ctx } = g;
  ctx.save();
  // Nested keeps divide the edge fade out once.
  if (!g.kept) ctx.globalAlpha = Math.min(1, ctx.globalAlpha / g.edge);
  g.kept++;
  try {
    return fn();
  } finally {
    g.kept--;
    ctx.restore();
  }
}

/**
 * Bar line `id`'s persistent layers (or only the ones at `only`, by index),
 * kept, at `alpha`: a scene drawing its stage exactly as the bar line holds
 * it, or a ticket bringing the next scene's stage in.
 */
export function rest(
  g: G,
  id: BoundaryId,
  alpha = 1,
  only?: readonly number[],
): void {
  if (alpha <= 0) return;
  keep(g, () => {
    g.ctx.save();
    g.ctx.globalAlpha *= alpha;
    RESTS[id].forEach((l, i) => {
      if (!only || only.includes(i)) drawLayer(g.ctx, l, g.d, g.t);
    });
    g.ctx.restore();
  });
}

/**
 * Bar line `id`'s terminal screen alone (its pane layer's take, with the
 * chrome's title and badges, not the window), kept, at `alpha`. For an
 * outgoing scene that has cleared its own screen and keeps the window up
 * itself (paneWindow): it brings the next take's opening prompt up in it
 * before the bar line, so the rest is a titled prompt, never an empty,
 * untitled window. Reach 1 by the section's last frame. Nothing for a bar
 * line whose pane has no take.
 */
export function restScreen(g: G, id: BoundaryId, alpha = 1): void {
  if (alpha <= 0) return;
  const l = RESTS[id].find((x) => x.kind === "pane");
  if (!l || l.kind !== "pane") return;
  keep(g, () => drawPaneScreen(g.ctx, l, g.d, g.t, Math.min(1, alpha)));
}

// The camera: a slow push-in on a payoff.

/**
 * A push-in (the vocabulary's `glide`, ART.md §13): the section's layers
 * scale about `focus` from 1 to `scale` over beats [`from`, `to`], hold,
 * and come back to 1 over [`out`, `out` + `outDur`] (default: never, for a
 * push that ends a section only if it is back by the bar line). Exactly 1
 * outside the span, so a bar line is never pushed in.
 */
export interface PushIn {
  from: number;
  to: number;
  /** 1.04 is MOTION.pushIn's slow breath; 1.25–1.35 makes a payoff read on a phone. */
  scale: number;
  /** The point that stays put, px: the payoff's centre. */
  focus: { x: number; y: number };
  out?: number;
  outDur?: number;
}

/** How far push `p` has scaled the frame at beat `b`: 1 outside it. */
export function pushAt(p: PushIn, b: number): number {
  const up = EASE.glide(progress(p.from, p.to, b));
  const down =
    p.out === undefined
      ? 0
      : EASE.glide(progress(p.out, p.out + (p.outDur ?? DUR.move), b));
  return 1 + (p.scale - 1) * up * (1 - down);
}

/**
 * Draw `fn` pushed in by `p` at the scene's beat: a camera over the layers
 * it draws (kept or not; captions are the compositor's and stay put). The
 * section's bar lines must fall outside the push (pushAt is 1 there), or
 * the rest is not the handoff's frame.
 */
export function pushed<T>(g: G, p: PushIn, fn: () => T): T {
  const k = pushAt(p, g.b);
  if (k === 1) return fn();
  const { ctx } = g;
  ctx.save();
  ctx.translate(p.focus.x, p.focus.y);
  ctx.scale(k, k);
  ctx.translate(-p.focus.x, -p.focus.y);
  try {
    return fn();
  } finally {
    ctx.restore();
  }
}

/** A lit rect (Scene.lit) under push `p` at beat `b`: where the pushed-in screen now stands, so the vignette still spares it. */
export function pushedLit(
  r: LitRect | null,
  p: PushIn,
  b: number,
): LitRect | null {
  const k = pushAt(p, b);
  if (!r || k === 1) return r;
  return {
    ...r,
    x: p.focus.x + (r.x - p.focus.x) * k,
    y: p.focus.y + (r.y - p.focus.y) * k,
    w: r.w * k,
    h: r.h * k,
  };
}

/** A pane's window, kept up across the section's bar lines at `alpha`. */
export function paneWindow(g: G, p: Pane, alpha = 1): void {
  if (alpha <= 0) return;
  keep(g, () => {
    g.ctx.save();
    g.ctx.globalAlpha *= alpha;
    drawPaneWindow(g.ctx, p);
    g.ctx.restore();
  });
}

// Tickets: each hands the stage on (STORYBOARD.md "Tickets hand the stage on").

/**
 * How each of bar line `id`'s layers stands at a ticket's beat `b` as it
 * leaves (from TICKET.cue.clear, fall and fade, DUR.exit each, 1/16 beat
 * apart, all gone by clearEnd) or enters (from bringAt, rise and fade, all
 * at rest by bringAt + bringDur), in RESTS order.
 */
export function stagePresence(
  id: BoundaryId,
  way: "leave" | "enter",
  b: number,
): Presence[] {
  const C = TICKET.cue;
  const n = RESTS[id].length;
  const each = n > 1 ? Math.min(DUR.stagger, DUR.staggerMax / (n - 1)) : 0;
  return RESTS[id].map((_, i) =>
    way === "leave"
      ? exit(b, C.clear + i * each, C.clearEnd - C.clear - (n - 1) * each)
      : enter(b, C.bringAt + i * each, C.bringDur - (n - 1) * each),
  );
}

/** Bar line `id`'s layers, kept, each as `ps` has it (stagePresence). */
function drawStageAs(g: G, id: BoundaryId, ps: readonly Presence[]): void {
  keep(g, () =>
    RESTS[id].forEach((l, i) => {
      const p = ps[i];
      if (p.alpha <= 0) return;
      g.ctx.save();
      g.ctx.globalAlpha *= p.alpha;
      g.ctx.translate(0, p.dy);
      drawLayer(g.ctx, l, g.d, g.t);
      g.ctx.restore();
    }),
  );
}

export interface TicketStageOptions {
  /** Take down the stage the ticket inherits (default true); false when the scene moves it off itself. */
  clear?: boolean;
  /** Bring the next section's stage in (default true); false when the scene brings it itself. */
  bring?: boolean;
  /**
   * Draw the act's ticket (default true, for `ticket`); false when the
   * scene draws it over the whip itself (SceneOptions.over:
   * drawSectionTicket).
   */
  ticket?: boolean;
}

/**
 * A ticket section's stage (STORYBOARD.md): its first bar line's layers,
 * at rest on beat 0, leave under the printing ticket (fall 16 px and fade,
 * gone by the tear); its last bar line's come in under the lifting one
 * (rise 24 px and fade, at rest by 1.85), so the stage is empty only while
 * the ticket hangs. A pure function of the beat, exactly at rest on both
 * bar lines.
 */
export function ticketStage(g: G, o: TicketStageOptions = {}): void {
  const { prev, next } = g;
  if (prev && o.clear !== false)
    drawStageAs(g, prev, stagePresence(prev, "leave", g.b));
  if (next && o.bring !== false)
    drawStageAs(g, next, stagePresence(next, "enter", g.b));
}

/**
 * The section's own ticket at its beat (kit/ticket.ts drawTicketFor),
 * kept: out from under any edge fade, so the paper and rail are opaque
 * from their first frame (the feed reveals the paper; nothing fades it
 * in). Off the frame on both bar lines by its own pose. As a
 * SceneOptions.over hook it comes down in place over the whip.
 */
export function drawSectionTicket(g: G): void {
  keep(g, () => drawTicketFor(g.ctx, g.s.id, g.b, { meta: g.join?.meta }));
}

/**
 * A ticket section: its stage handed on (ticketStage), and the act's
 * ticket printing on its rail over it (drawSectionTicket, ART.md §10),
 * drawn from the section's act alone.
 */
export function ticket(g: G, o: TicketStageOptions = {}): void {
  ticketStage(g, o);
  if (o.ticket !== false) drawSectionTicket(g);
}

// Terminal shots: a take's screen, mapped onto the section's beats.

/**
 * From beat `at`, the take plays from `from` toward `to` at `rate` × real
 * time, then holds. A typed play (`typed`, as step() makes them) plays its
 * typing, take time `from` to `typed.enter`, at `typed.rate` instead, then
 * holds the frame just before Enter for `typed.pause` beats (the key's
 * anticipation), then runs from Enter at `rate` (PACE rule 2).
 */
export interface Play {
  at: number;
  from: number;
  to: number;
  rate: number;
  typed?: Typed;
}

/** A play's typing: its Enter's take time, the typing's rate, and the beats held before Enter. */
export interface Typed {
  enter: number;
  rate: number;
  pause: number;
}

export const play = (
  at: number,
  from: number,
  to: number,
  rate = 1,
  typed?: Typed,
): Play => (typed ? { at, from, to, rate, typed } : { at, from, to, rate });

/**
 * Typing's rate: PACE.typeRate × real time (the rig's 45 ms a key shown at
 * about 11 a second). Its output after Enter plays at 1x.
 */
export const TYPE_RATE = PACE.typeRate;
/** Beats a typed command holds on screen before Enter lands. */
export const ENTER_PAUSE = PACE.enterPause;

/**
 * A play's pieces in order, each `[beat, from, to, rate]` of take time
 * (a rate of 0 holds `from`): one for a plain play; typing, the pause
 * before Enter and the output for a typed one.
 */
function pieces(p: Play): (readonly [number, number, number, number])[] {
  const T = p.typed;
  if (!T || T.enter <= p.from + 1e-9 || T.enter > p.to + 1e-9)
    return [[p.at, p.from, p.to, p.rate]];
  const typing = (T.enter - p.from) / (BEAT * T.rate);
  // The pause holds the last frame before Enter's (frameAt matches within 1e-9).
  const pre = Math.max(p.from, T.enter - 1e-4);
  return [
    [p.at, p.from, pre, T.rate],
    [p.at + typing, pre, pre, 0],
    [p.at + typing + T.pause, T.enter, p.to, p.rate],
  ];
}

/** Beats play `p` runs before it holds. */
export function playLen(p: Play): number {
  const ps = pieces(p);
  const [at, f, t, r] = ps[ps.length - 1];
  return at - p.at + (r > 0 ? (t - f) / (BEAT * r) : 0);
}

/** Plays end to end from beat `at`: each [from, to, rate] of take time, after the last. */
export function chain(
  at: number,
  parts: readonly (readonly [from: number, to: number, rate?: number])[],
): Play[] {
  const out: Play[] = [];
  let b = at;
  for (const [f, t, r = 1] of parts) {
    out.push(play(b, f, t, r));
    b += (t - f) / (BEAT * r);
  }
  return out;
}

/**
 * A typed command: take time `from` to `to`, typing up to Enter (`enter`)
 * at TYPE_RATE, holding ENTER_PAUSE beats on the typed command, and the
 * rest at `rate`. With no Enter inside the span, a plain play.
 */
export function typed(
  at: number,
  from: number,
  enter: number,
  to: number,
  o: { rate?: number; typeRate?: number; pause?: number } = {},
): Play {
  return play(at, from, to, o.rate ?? 1, {
    enter,
    rate: o.typeRate ?? TYPE_RATE,
    pause: o.pause ?? ENTER_PAUSE,
  });
}

/**
 * Step `name` of take `c` from its first key, at beat `at`: to its mark, or
 * to `until`. Its typing plays at TYPE_RATE and waits ENTER_PAUSE beats
 * before Enter (`typeRate: 1` and `pause: 0` for the take's own pace);
 * the output after Enter at `rate`.
 */
export function step(
  c: Capture,
  name: string,
  at: number,
  o: {
    rate?: number;
    until?: number;
    from?: number;
    typeRate?: number;
    pause?: number;
  } = {},
): Play {
  const s = stepOf(c, name);
  return typed(at, o.from ?? s.type, s.enter, o.until ?? s.end, o);
}

/** Beat `at` shows take time `t` and holds it: a cut. */
export const cut = (at: number, t: number): Play => play(at, t, t);

/**
 * How fast install rows play: three times real time, so no elapsed time a
 * row prints stays on screen unchanged for more than 0.1 s (plan v3
 * timing.test; mise ticks a row's elapsed time every 250-300 ms).
 */
export const LAPSE = 3;

/**
 * An install: `step`'s command typed from beat `at` (at TYPE_RATE, with
 * the pause before Enter), then its progress rows at LAPSE × real time up
 * to the last frame before the summary (or a transfer rate): CUT_ON, which
 * never reaches the screen. The shot must end where the plays do
 * (playsEnd): the rows are never held still. Rows shorter than
 * PACE.lapseUp at LAPSE end sooner: the tail (LapseTail) keeps the
 * command and the badge up for the rest.
 */
export function install(c: Capture, name: string, at: number): Play[] {
  const s = stepOf(c, name);
  const t = typed(at, s.type, s.enter, s.enter + 0.02);
  return [
    t,
    play(
      at + playLen(t),
      s.enter + 0.02,
      lastBefore(c, CUT_ON, s.enter, s.end),
      LAPSE,
    ),
  ];
}

/** The take time on screen at beat `b`, and whether it is playing faster than real time. */
export function clipTime(
  plays: readonly Play[],
  b: number,
): { ct: number; lapse: boolean; since: number } {
  let cur: Play | null = null;
  for (const p of plays) if (p.at <= b + 1e-9) cur = p;
  if (!cur) return { ct: plays[0].from, lapse: false, since: b };
  let piece = pieces(cur)[0];
  for (const x of pieces(cur)) if (x[0] <= b + 1e-9) piece = x;
  const [at, from, to, rate] = piece;
  const ct = Math.min(to, from + (b - at) * BEAT * rate);
  return { ct, lapse: rate > 1.001 && ct < to - 1e-6, since: cur.at };
}

/** When a list of plays has run out: the last beat anything on it moves. */
export function playsEnd(plays: readonly Play[]): number {
  const p = plays[plays.length - 1];
  return p.at + playLen(p);
}

export interface ShotOptions {
  /** Screen crops (captures.ts ScreenOptions), fixed or by take time. */
  screen?: ScreenOptions | ((ct: number) => ScreenOptions);
  /** The pane's alpha multiplier at beat b (a pane fading, or dimmed under a slam). */
  dim?: (b: number) => number;
  /**
   * The pane's focus dim at beat b, 0 to 1 (PANE_DIM: its text to 42 %
   * and its title to 60 %, as drawTerm's `dim`): a held screen waiting
   * dimmed while the card has the focus (an install's tail).
   */
  focus?: (b: number) => number;
  /** Shake the pane, px, at beat b. */
  shake?: (b: number) => readonly [number, number];
  /** Each line's alpha, from the lines shown (a dimmed run of log lines). */
  lineAlpha?: (i: number, lines: readonly string[]) => number;
  /** Fade over this many beats at each end (default 0.25; 0 is a hard cut). */
  fade?: number;
  /** Draw the text only: the scene keeps the window up itself (paneWindow). */
  bare?: boolean;
  /** Out from under the section's edge fade: the screen holds through a bar line. */
  keep?: boolean;
  /**
   * Badges the chrome bar carries at beat b besides the ones the shot
   * adds itself (time-lapse, illustration): the fresh machine's
   * (kit/rest.ts FRESH). Set in their ART.md §6 order (orderBadges).
   */
  badges?: (b: number) => readonly (string | ChromeBadge)[];
  /**
   * How the take's screen scrolls (kit/rest.ts scrollOf): "monotonic" by
   * default (a terminal never pans back up when a prompt widget collapses),
   * "plain" for the bottom rows always, or a fixed first line.
   */
  scroll?: TakeScroll;
  /**
   * An install's tail (LapseTail): the rows fade out while they still
   * move, the pane holds the frame the command was entered on, and the
   * time-lapse badge stays up through the hold.
   */
  tail?: LapseTail;
  /**
   * Size the pane to the take (kit/rest.ts fittedPane): its bottom edge
   * grows as output needs rows, from at least this many (true: FIT_MIN).
   * The fitted pane comes back as `pane`, for a scene that keeps its own
   * window (bare) to draw it with, or use fittedPane with the same take
   * time (the result's `ct`) before it draws the window.
   */
  fit?: boolean | number;
}

/**
 * How a shot ends on an install (its plays ending on a LAPSE play, as
 * `install` makes them), instead of cutting the whole pane away on the
 * frame before the summary (which left an empty, untitled window, and a
 * time-lapse badge up for 0.13 s: too short to read).
 *
 * - The progress rows (every line under the command) fade out over the
 *   plays' last `rowsOut` beats, while they are still moving, so no row is
 *   ever held and none finishes on screen.
 * - From the plays' end to the shot's `to`, the pane holds the take's own
 *   frame from just after Enter (`hold`: the command typed, no rows yet;
 *   default: the last frame at or before the fast play's start whose last
 *   line is the prompt's), so the window keeps its title and the command
 *   that was run.
 * - The time-lapse badge stays up from the fast play's first frame through
 *   the hold, fading over DUR.badge to `badgeUntil` (default the shot's
 *   `to`): at least a beat, long enough to read (ART.md §6, §9).
 */
export interface LapseTail {
  /** Beats the rows fade over before the plays end (default DUR.flick). */
  rowsOut?: number;
  /** The take time held once the plays end (default: enteredAt the fast play's start). */
  hold?: number | ((c: Capture) => number);
  /** The badge is gone by this beat (default: the shot's `to`). */
  badgeUntil?: number;
}

/**
 * The take's frame with a command just entered, at or before take time
 * `t`: the latest frame whose last non-blank line is a prompt with a
 * command on it (`~/work/api $ mise use jq`), before any output under it.
 * `t` itself when there is none.
 */
export function enteredAt(c: Capture, t: number): number {
  for (let i = frameAt(c, t); i >= 0; i--) {
    const rows = c.frames[i].rows;
    let last = rows.length - 1;
    while (last >= 0 && !rowText(c, rows[last]).trim()) last--;
    if (last >= 0 && /^\S* ?\$ \S/.test(rowText(c, rows[last])))
      return c.frames[i].t;
  }
  return t;
}

/** Where a take's cwd changes: each frame's time whose latest prompt names a new one, and what it was before. */
const cwdChanges = new WeakMap<
  Capture,
  { t: number; from: string | null; to: string }[]
>();

/**
 * A `cd` the take has just made at take time `ct`: how far through its
 * hop the chrome's cwd dot and title are (0 to 1), and the path it left;
 * null when no `cd` is under way.
 *
 * With `on` (the shot's plays and the scene's beat), the hop runs over
 * DUR.tick of the section's beats from the beat the plays first show the
 * change (beatOf), so a cut that jumps past the `cd`, or a hold just after
 * it, still gets its whole cross-fade on screen (in take time a cut
 * finished it in one frame, and a hold froze it half way). A `cd` at or
 * before the take time the plays open on happened before the shot: no
 * hop. Without `on`, it runs over DUR.tick of take time.
 */
export function cwdHop(
  c: Capture,
  ct: number,
  on?: { plays: readonly Play[]; b: number },
): { k: number; from: string | null } | null {
  let list = cwdChanges.get(c);
  if (!list) {
    list = [];
    let cur: string | null = null;
    for (const f of c.frames) {
      let cwd: string | null = null;
      for (let r = f.rows.length - 1; r >= 0 && cwd === null; r--) {
        const m = /^(~\S*|\/\S*) \$(?: |$)/.exec(rowText(c, f.rows[r]));
        if (m) cwd = m[1];
      }
      if (cwd !== null && cwd !== cur) {
        if (cur !== null) list.push({ t: f.t, from: cur, to: cwd });
        cur = cwd;
      }
    }
    cwdChanges.set(c, list);
  }
  for (let i = list.length - 1; i >= 0; i--) {
    const ch = list[i];
    // The same tolerance frameAt matches a frame with: a take time a play
    // lands on a hair before the change's frame shows that frame, so the
    // hop must have started (else the new title shows alone for a frame).
    if (ch.t > ct + 1e-9) continue;
    let k: number;
    if (on && on.plays.length) {
      if (ch.t <= on.plays[0].from + 1e-9) return null;
      k = (on.b - beatOf(on.plays, ch.t)) / DUR.tick;
    } else k = (ct - ch.t) / (DUR.tick * BEAT);
    return k < 1 ? { k: Math.max(1e-3, k), from: ch.from } : null;
  }
  return null;
}

/**
 * Take `id` in pane `p` from beat `from` to `to`, its screen at the take time
 * `plays` map the beat to. Badges ride the pane's chrome bar (ART.md §6): a
 * time-lapse badge while a play runs faster than real time, an
 * illustration badge while a shown row names a placeholder repository
 * (kit/rest.ts screenBadges), and any the scene adds (`badges`). Returns the
 * screen shown and the take time, or null when the pane is not up or the
 * take is missing.
 */
export function shot(
  g: G,
  id: CaptureId,
  p: Pane,
  from: number,
  to: number,
  plays: (c: Capture) => readonly Play[],
  o: ShotOptions = {},
): {
  screen: Screen;
  ct: number;
  c: Capture;
  layout: TermLayout;
  pane: Pane;
} | null {
  const a = win(g.b, from, to, o.fade ?? 0.25);
  if (a <= 0) return null;
  const c = capture(g.d, id);
  if (!c) {
    const box = () => missing(g.ctx, rectOf(p), id, a);
    if (o.keep) keep(g, box);
    else box();
    return null;
  }
  let out: ReturnType<typeof shot> = null;
  const drawIt = () => {
    const ps = plays(c);
    let { ct, lapse, since } = clipTime(ps, g.b);
    // An install's tail: the rows fade while they move, then the frame the
    // command was entered on holds, badged, to the shot's end.
    let rowsA = 1;
    let rowsFrom = Infinity;
    let lapseA = lapse ? 1 : 0;
    const fast = o.tail
      ? [...ps].reverse().find((q) => q.rate > 1.001)
      : undefined;
    if (o.tail && fast) {
      const T = o.tail;
      const end = playsEnd(ps);
      const hold =
        typeof T.hold === "function"
          ? T.hold(c)
          : (T.hold ?? enteredAt(c, fast.from));
      const held = screenAt(
        c,
        hold,
        typeof o.screen === "function" ? o.screen(hold) : (o.screen ?? {}),
      );
      let last = held.lines.length - 1;
      while (last >= 0 && !lineText(held.lines[last]).trim()) last--;
      rowsFrom = last + 1;
      if (g.b >= end) {
        ct = hold;
        lapse = false;
      } else
        rowsA =
          1 - EASE.glide(progress(end - (T.rowsOut ?? DUR.flick), end, g.b));
      const until = T.badgeUntil ?? to;
      lapseA = g.b >= fast.at ? 1 - progress(until - DUR.badge, until, g.b) : 0;
      since = fast.at;
    }
    const opts =
      typeof o.screen === "function" ? o.screen(ct) : (o.screen ?? {});
    const sc = screenAt(c, ct, opts);
    const [dx, dy] = o.shake?.(g.b) ?? [0, 0];
    const fp = o.fit
      ? fittedPane(c, ct, p, {
          min: typeof o.fit === "number" ? o.fit : undefined,
          screen: opts,
        })
      : p;
    const pane: Pane = {
      ...fp,
      window: p.window && !o.bare,
      ...(dx || dy
        ? {
            x: fp.x + dx,
            y: fp.y + dy,
            x0: fp.x0 + dx,
            baseline0: fp.baseline0 + dy,
          }
        : {}),
    };
    const texts = sc.lines.map(lineText);
    const la = o.lineAlpha;
    const scroll = scrollOf(c, ct, pane, o.scroll, opts);
    // The time-lapse badge rides the chrome bar while the take plays
    // faster than real time (and through an install's tail), popping in
    // as the fast play starts.
    const pop = badgePop(g.b - since);
    const badges = orderBadges([
      ...(lapseA > 0
        ? [{ text: BADGES.lapse, ...pop, alpha: pop.alpha * lapseA }]
        : []),
      ...screenBadges(c, ct, shownRows(texts, p, scroll)),
      ...(o.badges?.(g.b) ?? []),
    ]);
    const lineA =
      la || rowsA < 1
        ? (i: number) => (la ? la(i, texts) : 1) * (i >= rowsFrom ? rowsA : 1)
        : undefined;
    const layout = drawTerm(g.ctx, pane, sc.lines, {
      t: g.t,
      alpha: a * (o.dim?.(g.b) ?? 1),
      dim: o.focus?.(g.b),
      scroll,
      cursor: sc.cursor ?? undefined,
      lineAlpha: lineA,
      badges: badges.length ? badges : undefined,
      hop: cwdHop(c, ct, { plays: ps, b: g.b }) ?? undefined,
    });
    out = { screen: sc, ct, c, layout, pane };
  };
  if (o.keep) keep(g, drawIt);
  else drawIt();
  return out;
}

// Reading a take's rows.

/** The first text of a take's row matching `re` on the screen at `t`, or null. */
export function rowMatch(
  c: Capture | null,
  t: number,
  re: RegExp,
): RegExpExecArray | null {
  if (!c) return null;
  const sc = screenAt(c, t);
  for (const l of sc.lines) {
    const m = re.exec(l.map((x) => x.text).join(""));
    if (m) return m;
  }
  return null;
}

/** The runs of a take's row matching `re` on the screen at `t`, or null. */
export function rowRunsMatch(
  c: Capture | null,
  t: number,
  re: RegExp,
): Run[] | null {
  if (!c) return null;
  const sc = screenAt(c, t);
  return sc.lines.find((l) => re.test(l.map((x) => x.text).join(""))) ?? null;
}

/**
 * The beat a play list first shows take time `ct`: within a play, or on the
 * cut that jumps past it. Infinity if the list never gets there.
 */
export function beatOf(plays: readonly Play[], ct: number): number {
  for (const p of plays)
    if (ct <= p.to + 1e-9) {
      if (ct < p.from) return p.at;
      for (const [at, from, to, rate] of pieces(p))
        if (rate > 0 && ct <= to + 1e-9)
          return at + Math.max(0, ct - from) / (BEAT * rate);
      return p.at + playLen(p);
    }
  return Infinity;
}

/** Dim the whole frame by `k` (0 to 1): the lights going down (STAGE_FX.dim). */
export function dimLights(ctx: CanvasRenderingContext2D, k: number): void {
  if (k <= 0) return;
  ctx.save();
  ctx.fillStyle = rgba(STAGE_FX.dim.color, STAGE_FX.dim.alpha * k);
  ctx.fillRect(0, 0, W, H);
  ctx.restore();
}

/** Where the time goes in a section, for scenes: `progress` on beats. */
export const along = (b: number, a: number, z: number): number =>
  progress(a, z, b);

/** A row of a pane's screen, found by its text: its line index and the column `re` matches at. */
export function findRow(
  sc: Screen,
  re: RegExp,
): { line: number; col: number; text: string } | null {
  for (let i = sc.lines.length - 1; i >= 0; i--) {
    const text = sc.lines[i].map((r) => r.text).join("");
    const m = re.exec(text);
    if (m) return { line: i, col: m.index, text };
  }
  return null;
}
