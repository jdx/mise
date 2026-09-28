// Rivers of registry names (registry, ART.md §12.1): three streams of the
// registry's real names flowing left along gentle curves over the left
// column, back to front (RIVERS_ART): small, dim and slow at the back, large,
// bright and quick in front, each name turned to its stream's tangent and
// spaced by arc length. Each lane is an alphabetical run of the names from
// its own starting place. The lit names (`node` in the middle stream,
// `terraform` in the front one) each appear exactly once, phased to cross
// the column's middle at `cross`; each rides its stream as one of its names
// until its cue (`light`), then lights in tools pink at weight 700 and
// 1.12× with a glow, its neighbours easing apart to give it the room, and
// an underline draws on as the caption names it.
// Each stream can wash in and ebb on its own (`streams`).
//
// No box and no count: the caption carries "1,000+". A pure function of the
// names and the section's beat; the curves' arc-length tables are built
// once, since they depend on nothing but RIVERS_ART.

import { BEAT, PILLAR } from "../../bible";
import { mix, rgba } from "../../color";
import { glow } from "../../fx";
import { clamp, hash, lerp, smoothstep } from "../../math";
import { font, MONO } from "../../type";
import { ink } from "../ink";
import { type Curve, curveAt, type Pt } from "../motion";
import { advance } from "../term";
import { DUR, GLOW, RIVERS_ART, STROKE, UNDERLINE } from "../style";
import { span } from "./common";

/** Where the lit names cross, and where the soft edges and the clip are, x px. */
const MID_X = 650;
const CLIP = { x0: 100, x1: 1100 } as const;

/**
 * Names the reel may never show (test/forbidden.test.ts) are left out of the
 * streams, should the registry ever gain one.
 */
const NEVER = /trust|asdf|tool-versions|experimental/i;

interface Table {
  curve: Curve;
  /** Points and cumulative arc length at N + 1 even steps of u. */
  pts: Pt[];
  len: number[];
  total: number;
}

const N = 240;
const tables = RIVERS_ART.rivers.map((rv): Table => {
  const curve: Curve = { a: rv.a, c: rv.c, b: rv.b };
  const pts = Array.from({ length: N + 1 }, (_, i) => curveAt(curve, i / N));
  const len = [0];
  for (let i = 1; i <= N; i++)
    len.push(
      len[i - 1] + Math.hypot(pts[i].x - pts[i - 1].x, pts[i].y - pts[i - 1].y),
    );
  return { curve, pts, len, total: len[N] };
});

/** The point `s` px along a stream, and its tangent's angle (radians). */
function at(t: Table, s: number): { p: Pt; th: number } {
  const ss = clamp(s, 0, t.total);
  let lo = 1;
  let hi = N;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (t.len[mid] < ss) lo = mid + 1;
    else hi = mid;
  }
  const i = lo;
  const d = t.len[i] - t.len[i - 1] || 1;
  const u = (ss - t.len[i - 1]) / d;
  const a = t.pts[i - 1];
  const b = t.pts[i];
  const th = Math.atan2(b.y - a.y, b.x - a.x);
  // Past either end the stream carries straight on along its end tangent.
  const over = s - ss;
  return {
    p: {
      x: a.x + (b.x - a.x) * u + Math.cos(th) * over,
      y: a.y + (b.y - a.y) * u + Math.sin(th) * over,
    },
    th,
  };
}

/** How far along a stream its point with x = `x` is (the stream is monotone in x). */
function sAtX(t: Table, x: number): number {
  for (let i = 1; i <= N; i++)
    if (t.pts[i].x >= x) {
      const a = t.pts[i - 1];
      const b = t.pts[i];
      const u = b.x === a.x ? 0 : (x - a.x) / (b.x - a.x);
      return t.len[i - 1] + (t.len[i] - t.len[i - 1]) * u;
    }
  return t.total;
}

/** A name set in a lane: where its centre is, which way it reads, how wide it is. */
export interface PlacedName {
  name: string;
  river: number;
  lane: number;
  x: number;
  y: number;
  th: number;
  w: number;
  size: number;
  lit: boolean;
}

export interface RiversOptions {
  /** The registry's names, sorted (ReelData.registry.names). */
  names: readonly string[];
  /** Local beats: the streams drift linearly with time (or with `drift`). */
  b: number;
  /**
   * The beats of drift the streams have run on beat `b`, when they do not
   * run at one speed (a scene slowing them once they have flowed in):
   * where every name stands follows it, and `cross` is on its clock.
   * Default `b`. The cues (`light`, `named`) stay on `b`.
   */
  drift?: number;
  /**
   * The unlit names' opacity multiplier, 0 to 1 (the streams dimmed to
   * texture while the lit names have the focus). Default 1.
   */
  dim?: number;
  /**
   * The lit names and the stream (0 back, 1 middle, 2 front) each rides.
   * Default RIVERS_ART.litIn: `node` in the middle, `terraform` in front.
   */
  lit?: Readonly<Record<string, number>>;
  /** The beat each lit name crosses x 650, the middle of its window (x 540–760). */
  cross: number | Readonly<Record<string, number>>;
  /** The beat each lit name's underline starts drawing on (the caption naming it); none drawn without. */
  named?: Readonly<Record<string, number>>;
  /**
   * The beat each lit name starts to light: until then it rides its stream
   * as one of its names (its colour, weight and size), then it turns pink,
   * bold and 1.12× with its glow over DUR.light (arrive). Without it the lit
   * names are lit throughout.
   */
  light?: Readonly<Record<string, number>>;
  alpha?: number;
  /** Each stream's own opacity, back to front (a stream washing in or ebbing), times `alpha`. */
  streams?: readonly number[];
}

/**
 * The names in view on beat `b`, back to front, each where it stands. A lit
 * name takes its scale's worth of extra room as it lights (none before its
 * cue, so it rides its stream spaced like every other name), so its
 * neighbours never touch it.
 */
export function riverNames(o: RiversOptions): PlacedName[] {
  const lit = o.lit ?? RIVERS_ART.litIn;
  const litNames = Object.keys(lit).filter((n) => o.names.includes(n));
  const pool = o.names.filter((n) => !litNames.includes(n) && !NEVER.test(n));
  const out: PlacedName[] = [];
  if (!pool.length) return out;
  const secs = (o.drift ?? o.b) * BEAT;
  RIVERS_ART.rivers.forEach((rv, ri) => {
    const t = tables[ri];
    const adv = advance(rv.size);
    const gap = RIVERS_ART.gapEm * rv.size;
    // A lit name grows with its light (DUR.light, arrive), and so does its room.
    const grow = (n: string): number => {
      const at0 = o.light?.[n];
      return at0 === undefined ? 1 : span(o.b, at0, DUR.light, "arrive");
    };
    const width = (n: string, isLit: boolean) =>
      Array.from(n).length *
      adv *
      (isLit ? lerp(1, RIVERS_ART.lit.scale, grow(n)) : 1);
    for (let lane = 0; lane < rv.lanes; lane++) {
      const off = (lane - (rv.lanes - 1) / 2) * RIVERS_ART.laneEm * rv.size;
      // The lane's lit name (only its first lane carries one), and where
      // its run of names goes on in the pool: after the lit name, in order,
      // past the names it begins (`terraform-docs` right after `terraform`
      // reads as the same name again), and before it, in order.
      const mine = lane === 0 ? litNames.find((n) => lit[n] === ri) : undefined;
      const start = mine
        ? pool.findIndex((n) => n > mine && !n.startsWith(mine))
        : Math.floor(hash(ri * 7 + lane, 3) * pool.length);
      const next = mine ? pool.findIndex((n) => n > mine) : -1;
      const before = mine ? (next < 0 ? pool.length : next) - 1 : start;
      const nameOf = (k: number): { name: string; lit: boolean } => {
        if (mine && k === 0) return { name: mine, lit: true };
        const j = mine ? (k > 0 ? start + k - 1 : before + k + 1) : start + k;
        const n = pool.length;
        return { name: pool[((j % n) + n) % n], lit: false };
      };
      // Name 0's centre, px along the stream: a lit name crosses MID_X on
      // its beat; any other lane starts at a place of its own.
      const cross =
        typeof o.cross === "number" ? o.cross : (o.cross[mine ?? ""] ?? 0);
      const s0 = mine
        ? sAtX(t, MID_X) + rv.speed * cross * BEAT
        : hash(ri * 13 + lane, 5) * 400 - 120;
      const c0 = s0 - rv.speed * secs;
      const visible = (c: number, w: number) =>
        c + w / 2 > -80 && c - w / 2 < t.total + 80;
      const place = (k: number, c: number) => {
        const { name, lit: isLit } = nameOf(k);
        const w = width(name, isLit);
        if (!visible(c, w)) return;
        const { p, th } = at(t, c);
        out.push({
          name,
          river: ri,
          lane,
          x: p.x - Math.sin(th) * off,
          y: p.y + Math.cos(th) * off,
          th,
          w,
          size: rv.size,
          lit: isLit,
        });
      };
      // Walk out from name 0 both ways until the stream is behind us.
      const first = nameOf(0);
      const w0 = width(first.name, first.lit);
      place(0, c0);
      let cc = c0;
      let wp = w0;
      for (let k = 1; k < 400; k++) {
        const nm = nameOf(k);
        const w = width(nm.name, nm.lit);
        cc += wp / 2 + gap + w / 2;
        wp = w;
        if (cc - w / 2 > t.total + 80) break;
        place(k, cc);
      }
      cc = c0;
      wp = w0;
      for (let k = -1; k > -400; k--) {
        const nm = nameOf(k);
        const w = width(nm.name, nm.lit);
        cc -= wp / 2 + gap + w / 2;
        wp = w;
        if (cc + w / 2 < -80) break;
        place(k, cc);
      }
    }
  });
  return out;
}

/** A name's opacity at its edge: the 140 px soft fade inside the clip. */
const edgeAlpha = (x: number): number =>
  smoothstep(CLIP.x0, CLIP.x0 + RIVERS_ART.edge, x) *
  (1 - smoothstep(CLIP.x1 - RIVERS_ART.edge, CLIP.x1, x));

/**
 * The rivers on beat `b`. Returns the lit names where they stand (for a
 * scene that points at one), or an empty list with no names to draw.
 */
export function rivers(
  ctx: CanvasRenderingContext2D,
  o: RiversOptions,
): PlacedName[] {
  const alpha = o.alpha ?? 1;
  const placed = riverNames(o);
  if (alpha <= 0 || !placed.length) return placed.filter((p) => p.lit);
  const region = RIVERS_ART.region;
  ctx.save();
  ctx.globalAlpha *= alpha;
  ctx.beginPath();
  ctx.rect(CLIP.x0, region.y - 60, CLIP.x1 - CLIP.x0, region.h + 120);
  ctx.clip();
  ctx.textAlign = "left";
  ctx.textBaseline = "alphabetic";
  const streamAlpha = (ri: number) => clamp(o.streams?.[ri] ?? 1);
  // How far each lit name has lit: all the way without a cue.
  const litK = (n: PlacedName): number => {
    if (!n.lit) return 0;
    const at0 = o.light?.[n.name];
    return at0 === undefined ? 1 : span(o.b, at0, DUR.light, "arrive");
  };
  // Glows first, under every name.
  for (const n of placed) {
    const k = litK(n);
    if (k > 0)
      glow(
        ctx,
        n.x + Math.sin(n.th) * n.size * 0.3,
        n.y - Math.cos(n.th) * n.size * 0.3,
        GLOW.name.radius,
        PILLAR.tools,
        GLOW.name.alpha * edgeAlpha(n.x) * k * streamAlpha(n.river),
      );
  }
  /** A name's characters at `size`, one a column from its centre, each faded by where it stands. */
  const setName = (
    n: PlacedName,
    size: number,
    weight: number,
    color: string,
    a: number,
  ) => {
    if (a <= 0) return;
    const adv = advance(size);
    const chars = Array.from(n.name);
    const x0 = -(chars.length * adv) / 2;
    const cos = Math.cos(n.th);
    ctx.font = font(size, weight, MONO);
    // One character a column, each faded by where it stands on the stage,
    // so a long name eases out at the edge rather than being cut.
    chars.forEach((ch, i) => {
      const lx = x0 + i * adv;
      const ea = edgeAlpha(n.x + cos * (lx + adv / 2));
      if (ea <= 0) return;
      ctx.fillStyle = rgba(color, a * ea);
      ctx.fillText(ch, lx, 0);
    });
  };
  for (const n of placed) {
    const rv = RIVERS_ART.rivers[n.river];
    const sa = streamAlpha(n.river);
    if (sa <= 0) continue;
    const k = litK(n);
    ink(n.name, "text");
    ctx.save();
    ctx.translate(n.x, n.y);
    ctx.rotate(n.th);
    const L = RIVERS_ART.lit;
    const dim = clamp(o.dim ?? 1);
    if (k < 1) setName(n, n.size, 400, rv.color, rv.alpha * sa * (1 - k) * dim);
    if (k > 0) {
      const size = n.size * lerp(1, L.scale, k);
      setName(
        n,
        size,
        L.weight,
        mix(rv.color, L.color, k),
        lerp(rv.alpha, 1, k) * sa * k,
      );
    }
    const x0 = -n.w / 2;
    const size = n.size * RIVERS_ART.lit.scale;
    const color = RIVERS_ART.lit.color;
    if (n.lit) {
      const at0 = o.named?.[n.name];
      const k = at0 === undefined ? 0 : span(o.b, at0, DUR.half, "arrive");
      if (k > 0) {
        ctx.fillStyle = rgba(color, edgeAlpha(n.x) * sa);
        // UNDERLINE.dy under the baseline, or less where the lane below would
        // meet it (lanes are 1.3 em apart).
        const dy = Math.min(UNDERLINE.dy, 0.3 * size);
        ctx.fillRect(x0, dy, n.w * k, STROKE.underline);
      }
    }
    ctx.restore();
  }
  ctx.restore();
  return placed.filter((p) => p.lit);
}
