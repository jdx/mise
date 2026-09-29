// Pacing: the rules that let a first-time viewer follow every beat
// without pausing (STORYBOARD.md "Pacing", the numbers in kit/style.ts
// PACE), as helpers a scene builds its schedule with.
//
// - Pace: a section's focal motions in order, one at a time, each starting
//   PACE.gap after the last stopped (rule 4); a terminal act waits out the
//   read hold of the output before it (rule 3); a caption lands once the
//   event it names is on screen and holds its reading time (rule 5,
//   storyboard.ts captionTimes); and the section's length follows, with its
//   rest (rule 6). The spans it names are the scene's events: pass
//   `events()` to greyScene's `events` so the captions anchor on them, and
//   read the score's cues off them.
// - readHold / lapseUp / restOut / slamHold: the holds, in beats.
// - focusOf / unfocus: everything but the current focus dims to
//   PACE.focusDim (rule 4); focusPlan / readsOf build the plan so that the
//   terminal is never dimmed while its output is still being read (rule 3
//   before rule 4), and nothing dims and relights inside FLICKER.
//
// Everything is a pure function of the take and the beat: a scene builds
// its Pace from the facts on every frame (or memoises it per take), and
// the same numbers come back.

import { BEAT, type SectionId, sec } from "../bible";
import type { Capture, CaptureId, ReelData } from "../captures";
import { lerp, progress } from "../math";
import { boardOf, type CaptionTimes, captionTimes } from "../storyboard";
import { type Play, playsEnd } from "./grey";
import { DUR, EASE, PACE } from "./style";

/**
 * A scene's schedule, built from the facts once per facts object (and
 * once for none): a Pace is cheap, but a frame should not rebuild it, its
 * captions' times and its plays every time it draws. The same facts
 * always give the same schedule, so frames stay pure functions of time.
 */
export function perFacts<T>(
  fn: (d: ReelData | null) => T,
): (d: ReelData | null) => T {
  const seen = new WeakMap<object, T>();
  let none: { v: T } | null = null;
  return (d) => {
    if (!d) {
      none ??= { v: fn(null) };
      return none.v;
    }
    if (!seen.has(d)) seen.set(d, fn(d));
    return seen.get(d) as T;
  };
}

/**
 * Take `id` in the facts, or null: as captures.ts capture(), but also for
 * facts that carry versions and no takes (the storyboard's version sets),
 * so a schedule built for captions falls back to the plan.
 */
export const takeIn = (d: ReelData | null, id: CaptureId): Capture | null =>
  d?.captures?.[id] ?? null;

/** Seconds as beats. */
const beats = (s: number): number => s / BEAT;

/** The gap between two focal motions, beats (rule 4). */
export const GAP = PACE.gap;
/** The alpha multiplier of a layer out of focus (rule 4). */
export const FOCUS_DIM = PACE.focusDim;

/**
 * How long a command's output holds still once it has printed, beats
 * (rule 3): 2 s, and half a second more for each line past the third the
 * viewer reads. Lines dimmed to texture are not counted.
 */
export const readHold = (lines: number): number =>
  beats(PACE.read + PACE.readPerLine * Math.max(0, lines - PACE.readFree));

/** Install rows under the time-lapse badge stay up at least this long, beats (rule 3). */
export const lapseUp = (): number => beats(PACE.lapseUp);
/** A section's closing rest, beats (rule 6). */
export const restOut = (): number => beats(PACE.restOut);
/** Kinetic type at full size before it shrinks, beats (rule 7). */
export const slamHold = (): number => beats(PACE.slamHold);

/** A named stretch of a section's beats: when it starts moving, and when it has stopped. */
export interface Span {
  at: number;
  end: number;
}

export interface PaceOptions {
  /** The first focal motion's earliest beat (default GAP: half a beat after the bar line). */
  from?: number;
  /** The facts the captions are filled from (their word counts). */
  d?: ReelData | null;
}

/**
 * A section's schedule, one focal motion at a time (rule 4). Every call
 * places its motion GAP after the last one stopped (or later, `after`),
 * names it, and returns its span; `events()` is the record the captions
 * and cues key off.
 *
 *   const p = new Pace("use");
 *   const nf = p.term("notFound", (at) => [step(c, "not-found", at)], { lines: 1 });
 *   const seat = p.move("seat", DUR.seatGap + DUR.seat);
 *   p.caption(0);            // the storyboard's caption 1, after its `after` event
 *   const beats = p.need();  // what the section needs, rest included
 *
 * A rule the schedule breaks (a caption over a moving thing, a hold cut
 * short) is listed in `issues`, which test/pace.test.ts and a scene's own
 * test can assert empty.
 */
export class Pace {
  readonly id: SectionId;
  readonly d: ReelData | null;
  /** Rules this schedule breaks, in words. */
  readonly issues: string[] = [];
  private last: number;
  private termFree: number;
  private readHeld = -Infinity;
  private readonly named = new Map<string, Span>();
  private readonly caps: CaptionTimes[] = [];

  constructor(id: SectionId, o: PaceOptions = {}) {
    this.id = id;
    this.d = o.d ?? null;
    const from = o.from ?? GAP;
    this.last = from - GAP;
    this.termFree = from;
  }

  /** The earliest beat the next focal motion may start: GAP after the last stopped, and not before `after`. */
  next(after = -Infinity): number {
    return Math.max(this.last + GAP, after);
  }

  /** The beat the terminal's last output has held long enough (rule 3), and may change. */
  get termReady(): number {
    return this.termFree;
  }

  /** Wait: no focal motion starts before beat `b`. */
  wait(b: number): this {
    this.last = Math.max(this.last, b - GAP);
    return this;
  }

  /** A focal motion of `dur` beats named `name`, from next(after). */
  move(name: string, dur: number, o: { after?: number } = {}): Span {
    const at = this.next(o.after);
    return this.put(name, { at, end: at + dur });
  }

  /**
   * A terminal act named `name`: `plays(at)` from GAP after the last
   * motion. Its typing may run while the output before it holds (rule 3
   * keeps that output on screen, unmoved, for its read hold; typing on the
   * prompt below it does not move it), but its Enter, the first thing that
   * can scroll or replace it, never lands before that hold is over: the
   * act starts later if it would. It stops moving where the plays run
   * out; its own output then holds readHold(`lines`) (termReady). `lines`
   * counts the output lines to read (default 1; 0 for a command that
   * prints nothing, like `cd`, whose prompt change still holds 2 s).
   */
  term(
    name: string,
    plays: (at: number) => readonly Play[],
    o: { lines?: number; after?: number } = {},
  ): Span & { plays: readonly Play[]; held: number; enter: number } {
    const lead = enterOf(plays(0));
    const at = Math.max(this.next(o.after), this.termFree - lead);
    const ps = plays(at);
    const end = playsEnd(ps);
    this.termFree = end + readHold(o.lines ?? 1);
    this.readHeld = Math.max(this.readHeld, this.termFree);
    return {
      ...this.put(name, { at, end }),
      plays: ps,
      held: this.termFree,
      enter: at + lead,
    };
  }

  /**
   * An install's rows under the time-lapse badge: the plays (install())
   * from next(), and the rows' unit kept up PACE.lapseUp from the first
   * row (the tail holds the command and the badge for what the rows lack).
   */
  lapse(
    name: string,
    plays: (at: number) => readonly Play[],
    rowsFrom: (ps: readonly Play[]) => number,
  ): Span & { plays: readonly Play[] } {
    const lead = enterOf(plays(0));
    const at = Math.max(this.next(), this.termFree - lead);
    const ps = plays(at);
    const rows = rowsFrom(ps);
    const end = Math.max(playsEnd(ps), rows + lapseUp());
    this.termFree = end;
    return { ...this.put(name, { at, end }), plays: ps };
  }

  /** Hold everything until the terminal's output has been read (before it clears or leaves). */
  readTerm(): this {
    return this.wait(this.termFree);
  }

  /**
   * The storyboard's caption `k` (0-based), as captionsFor will set it
   * given this schedule's events so far: anchored GAP after the event its
   * `after` names (or on its storyboard time), spaced by PACE.captionEvery
   * from the one before. Its writing-in is the focal motion; once landed
   * it holds its reading time without blocking the next motion.
   */
  caption(k: number): Span & { out: number } {
    const all = captionTimes(this.id, this.d, this.events());
    const c = all[k];
    if (!c) throw new Error(`${this.id} has no caption ${k + 1}`);
    this.caps[k] = c;
    const span = this.put(`caption${k + 1}`, { at: c.at, end: c.landed });
    return { ...span, out: c.minOut };
  }

  /** A span by name. */
  get(name: string): Span {
    const s = this.named.get(name);
    if (!s) throw new Error(`${this.id}: no event "${name}"`);
    return s;
  }

  /** Every named span's end (the beat it is on screen and still), for greyScene's `events` and the cues. */
  events(): Record<string, number> {
    const out: Record<string, number> = {};
    for (const [k, v] of this.named) out[k] = v.end;
    return out;
  }

  /** The last beat anything focal moves. */
  get settled(): number {
    return this.last;
  }

  /**
   * The beats the section needs: its last motion, then its rest (rule 6);
   * the last output's read hold (rule 3); and its last caption's reading
   * time with the beat it holds on to before the bar line (captionsFor).
   * Rounded up to whole 4/4 bars, or to a half bar with `half`.
   */
  need(o: { half?: boolean; rest?: boolean } = {}): number {
    const lastCap = this.caps.length ? this.caps[this.caps.length - 1] : null;
    const raw = Math.max(
      this.last + (o.rest === false ? 0 : restOut()),
      this.readHeld,
      lastCap ? lastCap.minOut + 1 : 0,
    );
    const grid = o.half ? 2 : 4;
    return Math.ceil(raw / grid - 1e-9) * grid;
  }

  private put(name: string, s: Span): Span {
    if (this.named.has(name))
      throw new Error(`${this.id}: event "${name}" twice`);
    if (s.at < this.last + GAP - 1e-6 && this.named.size)
      this.issues.push(
        `"${name}" starts at b${s.at} while the last motion stops at b${this.last}`,
      );
    this.named.set(name, s);
    this.last = Math.max(this.last, s.end);
    return s;
  }
}

/**
 * The beat plays starting on beat 0 land their first Enter (a typed play's
 * typing and pause), or 0 when none is typed: from there the output may
 * scroll or replace what is on screen.
 */
export function enterOf(plays: readonly Play[]): number {
  for (const p of plays)
    if (p.typed && p.typed.enter > p.from + 1e-9)
      return (
        p.at + (p.typed.enter - p.from) / (BEAT * p.typed.rate) + p.typed.pause
      );
  return 0;
}

/** The storyboard's `after` event of each of section `id`'s captions. */
export const captionEvents = (id: SectionId): (string | undefined)[] =>
  boardOf(id).captions.map((c) => c.after);

// Focus.

/**
 * A focus schedule: from each entry's beat, the layers it names are the
 * focus and every other layer dims to FOCUS_DIM; "*" brings everything
 * back. Entries in beat order.
 */
export type FocusPlan = readonly (readonly [
  at: number,
  on: "*" | string | readonly string[],
])[];

const inFocus = (on: FocusPlan[number][1], layer: string): boolean =>
  on === "*" || on === layer || (Array.isArray(on) && on.includes(layer));

/**
 * focusPlan's `reads` for terminal acts (Pace.term): each from its start
 * to the end of its output's read hold. An act without a hold (a plan
 * span standing in for a missing take) adds none.
 */
export const readsOf = (
  ...acts: readonly (Span & { held?: number })[]
): [number, number][] =>
  acts.flatMap((a) =>
    a.held === undefined ? [] : [[a.at, a.held] as [number, number]],
  );

/**
 * A dim shorter than this is a flicker, beats: the ease down (DUR.dim) and
 * straight back up.
 */
export const FLICKER = 2 * DUR.dim;

/**
 * A focus plan from the layers that take the focus in turn (`steps`, each
 * from its beat; "*" lights everything), keeping the terminal lit through
 * each window in `reads` (from an act's start to the end of its output's
 * read hold: Pace.term's `at` and `held`), so the output being read is
 * never dimmed under a motion elsewhere (rule 3 before rule 4). A layer
 * that would dim and light again (or light and dim again) inside FLICKER
 * holds as it was instead.
 */
export function focusPlan(
  steps: readonly (readonly [number, string | readonly string[]])[],
  reads: readonly (readonly [number, number])[],
): FocusPlan {
  const points = [
    ...new Set([...steps.map(([b]) => b), ...reads.map(([, z]) => z)]),
  ].sort((a, z) => a - z);
  const out: [number, "*" | string | readonly string[]][] = [];
  let last = "";
  for (const b of points) {
    let layer: string | readonly string[] | null = null;
    for (const [at, l] of steps) if (at <= b + 1e-9) layer = l;
    if (layer === null) continue;
    const reading = reads.some(([a, z]) => a <= b + 1e-9 && b < z - 1e-9);
    const layers = [layer].flat();
    const on: "*" | string | readonly string[] =
      layer === "*"
        ? "*"
        : reading && !layers.includes("term")
          ? [...layers, "term"]
          : layer;
    const key = JSON.stringify(on);
    if (key === last) continue;
    last = key;
    out.push([b, on]);
  }
  // A layer dimmed and lit again (or lit and dimmed again) inside FLICKER
  // is a flicker: the focus before holds on instead.
  const names = [
    ...new Set(
      out.flatMap(([, on]): string[] => (on === "*" ? [] : [on].flat())),
    ),
  ];
  const lit = (on: FocusPlan[number][1], l: string) =>
    on === "*" || on === l || (Array.isArray(on) && on.includes(l));
  return out.filter(([b, on], i) => {
    const before = out[i - 1];
    const after = out[i + 1];
    if (!before || !after || after[0] - b >= FLICKER - 1e-9) return true;
    return !names.some(
      (l) =>
        lit(on, l) !== lit(before[1], l) &&
        lit(after[1], l) === lit(before[1], l),
    );
  });
}

/**
 * Layer `layer`'s alpha multiplier at beat `b` under `plan`: 1 in focus,
 * FOCUS_DIM out of it, easing between over DUR.dim (glide) from each
 * entry's beat. 1 before the first entry, so a bar line with no entry on
 * it rests undimmed.
 */
export function focusOf(plan: FocusPlan, layer: string, b: number): number {
  const level = (i: number, t: number): number => {
    if (i < 0) return 1;
    const [at, on] = plan[i];
    if (t < at) return level(i - 1, t);
    const to = inFocus(on, layer) ? 1 : FOCUS_DIM;
    const k = EASE.glide(progress(at, at + DUR.dim, t));
    return lerp(level(i - 1, at), to, k);
  };
  return level(plan.length - 1, b);
}

/**
 * The same as a dim amount, 0 (in focus) to 1 (out of it): for the kit's
 * `dim`/`focus` options that take one (shot's `focus`, drawPaneWindow).
 */
export const unfocus = (plan: FocusPlan, layer: string, b: number): number =>
  (1 - focusOf(plan, layer, b)) / (1 - FOCUS_DIM);

/** Section `id`'s beat `b` in global seconds: for a plan printed in reel time. */
export const reelAt = (id: SectionId, b: number): number => sec(id).beat(b);
