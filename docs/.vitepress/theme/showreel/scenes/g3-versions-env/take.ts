// What the Versions and Environments scenes (switch, packslip, vars,
// redact) share for their terminals: a take's commands laid on the
// section's beats so each one's result (the first frame after its Enter)
// lands where the scene says, typed at the take's own pace with the pauses
// between commands cut; and a take's screen drawn like kit/grey.ts shot()
// but with the pane's focus dim (PANE_DIM) as well as its alpha, so a scene
// can dim its terminal onto a bar line's rest (kit/rest.ts `dim`).
//
// Pure functions of the take and the beat, like everything a frame draws.

import { BEAT } from "../../bible";
import {
  type Capture,
  type CaptureId,
  capture,
  firstWith,
  frameAfter,
  type Screen,
  type ScreenOptions,
  screenAt,
  stepOf,
} from "../../captures";
import {
  badgePop,
  clipTime,
  cut,
  cwdHop,
  type G,
  keep,
  missing,
  type Play,
  play,
  rectOf,
} from "../../kit/grey";
import {
  BADGES,
  orderBadges,
  screenBadges,
  scrollOf,
  shownRows,
} from "../../kit/rest";
import {
  type ChromeBadge,
  drawTerm,
  lineText,
  type Pane,
  type TermLayout,
} from "../../kit/term";

/** How long before a command's first key its play starts: the prompt waiting, 20 ms. */
export const PRE = 0.02;

/** The first key step `name` types, take seconds. */
export function firstKeyOf(c: Capture, name: string): number {
  const s = stepOf(c, name);
  const k = c.keys.find(([t]) => t > s.start && t <= s.end);
  return k ? k[0] : s.type;
}

/**
 * Step `name`'s result on screen: the first frame after its last key with
 * a new row that says something (a new prompt, a printed line, a Tab's
 * list), past the frames that only echo the key.
 */
export function resultOf(c: Capture, name: string): number {
  const s = stepOf(c, name);
  return firstWith(c, /\S/, s.enter + 1e-6) ?? frameAfter(c, s.enter);
}

/**
 * Step `name` typed at the take's pace from `from` (default its first key
 * less PRE) so that its result lands on beat `land`, or as soon after as
 * the previous command allows: never starting before beat `after`. Plays on
 * to `to` (default the step's mark). Returns the play and the beat the
 * result really lands on.
 */
export function typed(
  c: Capture,
  name: string,
  land: number,
  after = -Infinity,
  o: { from?: number; to?: number; result?: number } = {},
): { play: Play; land: number } {
  const from = o.from ?? firstKeyOf(c, name) - PRE;
  const result = o.result ?? resultOf(c, name);
  const lead = (result - from) / BEAT;
  const at = Math.max(after, land - lead);
  return {
    play: play(at, from, o.to ?? stepOf(c, name).end),
    land: at + lead,
  };
}

/**
 * A take's steps one after another on the section's beats, each typed at
 * the take's pace (typed()) so its result lands on its beat in `lands`,
 * the take opening on `open` (take seconds) at beat 0. Returns the plays
 * and the beat each result really lands on; with no take, the beats as
 * given, which the score's cues then use.
 */
export function landSteps<L extends string>(
  c: Capture | null,
  open: (c: Capture) => number,
  steps: readonly (readonly [step: string, land: L])[],
  lands: Readonly<Record<L, number>>,
): { plays: Play[]; at: Record<L, number> } {
  const at = { ...lands } as Record<L, number>;
  if (!c) return { plays: [], at };
  const plays: Play[] = [cut(0, open(c))];
  let after = 0;
  for (const [step, land] of steps) {
    const t = typed(c, step, lands[land], after);
    plays.push(t.play);
    at[land] = after = t.land;
  }
  return { plays, at };
}

export interface TermOptions {
  /** The pane's alpha, 0 to 1. */
  alpha?: number;
  /** 0 to 1: the pane's focus dim (PANE_DIM): text and title. */
  dim?: number;
  /** Each line's alpha, from the lines shown. */
  lineAlpha?: (i: number, lines: readonly string[]) => number;
  screen?: ScreenOptions;
  /** Out from under the section's edge fade: the screen holds through a bar line. */
  keep?: boolean;
  /** Badges the chrome bar carries besides the ones the take calls for. */
  badges?: readonly (string | ChromeBadge)[];
}

/** A take's screen in a pane, as term() drew it. */
export interface Shown {
  screen: Screen;
  ct: number;
  c: Capture;
  layout: TermLayout;
  pane: Pane;
}

/**
 * Take `id`'s screen in pane `p` (text, title and badges; the window is the
 * scene's) at the take time `plays` map beat `g.b` to: the time-lapse badge
 * while a play runs faster than real time, the illustration badge while a
 * shown row names a placeholder, the cwd dot hopping on a `cd` (over a
 * quarter beat of the section from the beat the plays first show it), the
 * screen never scrolling back up, and the pane dimmed by `dim`. Null when the take is missing (its missing card is
 * drawn instead) or the pane is not up.
 */
export function term(
  g: G,
  id: CaptureId,
  p: Pane,
  plays: (c: Capture) => readonly Play[],
  o: TermOptions = {},
): Shown | null {
  const a = o.alpha ?? 1;
  if (a <= 0) return null;
  const c = capture(g.d, id);
  const draw = (): Shown | null => {
    if (!c) {
      missing(g.ctx, rectOf(p), id, a);
      return null;
    }
    const ps = plays(c);
    const { ct, lapse, since } = clipTime(ps, g.b);
    const screen = screenAt(c, ct, o.screen ?? {});
    const texts = screen.lines.map(lineText);
    const la = o.lineAlpha;
    // As a bar line's rest scrolls it (kit/rest.ts drawScreen): never back up.
    const scroll = scrollOf(c, ct, p, "monotonic", o.screen);
    const badges = orderBadges([
      ...(lapse ? [{ text: BADGES.lapse, ...badgePop(g.b - since) }] : []),
      ...screenBadges(c, ct, shownRows(texts, p, scroll)),
      ...(o.badges ?? []),
    ]);
    const layout = drawTerm(g.ctx, { ...p, window: false }, screen.lines, {
      t: g.t,
      alpha: a,
      dim: o.dim,
      scroll,
      cursor: screen.cursor ?? undefined,
      lineAlpha: la ? (i) => la(i, texts) : undefined,
      badges: badges.length ? badges : undefined,
      // The cwd hop on the section's beats, so a cut past a `cd` or a hold
      // just after it still shows its whole cross-fade (kit/grey.ts cwdHop).
      hop: cwdHop(c, ct, { plays: ps, b: g.b }) ?? undefined,
    });
    return { screen, ct, c, layout, pane: p };
  };
  return o.keep ? keep(g, draw) : draw();
}
