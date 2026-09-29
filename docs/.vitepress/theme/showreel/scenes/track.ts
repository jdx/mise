// track (plan v3 §3, Act V; ART.md §12.9; STORYBOARD.md "Pacing"): C15.
// The ticket leaves the `~/.zshrc` card, the empty checkpoint rail and the
// terminal at `~ $`. One focal motion at a time, half a beat apart
// (kit/pace.ts Pace), whatever is not the focus dimmed, and never the
// terminal while its output is read:
//
// `mise dot track ~/.zshrc` types at TYPE_RATE and its confirm prompt is
// answered on camera; once `saved baseline checkpoint 1` has printed, the
// rail's first node snaps in and the pointer drops onto it, and caption 1
// follows it. Its footnote (the watcher) writes on under it. `echo "alias
// …" >> ~/.zshrc` types; once it has run, a copy of the quoted line lifts
// out of the command and flies into the card's next row (the pane keeps
// its own): the file has changed, and then the watcher saves it: the rail
// gains the second checkpoint (its id and trigger the take's) and the
// pointer steps to it, still under caption 1, which holds until the save
// has played. `mise dot history` lists the baseline and the watcher's
// save, confirming it, and dims. `mise dot rollback ~/.zshrc` prints its
// plan (dimmed) and its prompt, answered on camera; once `rolled back` has
// printed, the alias's row (its seat mark held, still, since it landed)
// folds back out of the card, and then the rail rewinds: the pointer goes
// back to the baseline, the dashed arc draws from it, the rollback's
// checkpoint snaps in at its end (the id and trigger mise recorded for it:
// C15's history after the rollback, off camera) and the pointer rides the
// arc to it. Caption 2 follows, and the footnote `mise dot origin set`
// under it. The tables brighten back and everything comes back into focus,
// and the card (the fixture again), the rail and the terminal hold at rest
// into the Everywhere ticket (kit/rest.ts track|machines).
//
// Soft wraps (`tracking ~/.zshrc (… declared i` / `g.toml)`) are joined to
// their line, so the side pane's fade crops the rest with it instead of
// leaving an orphan row (a viewport crop, ART.md §13). The bar lines never
// show a wrapped row, so they are the same either way.
//
// Every take time comes from the take (its keys, marks and printed rows);
// the schedule places each act on the section's beats, and `cues` gives
// the score the same beats the drawing uses.

import type { ReelFacts } from "../bible";
import {
  capture,
  type Capture,
  firstWith,
  type ReelData,
  reelData,
  rowRuns,
  rowText,
  screenAt,
  stepOf,
} from "../captures";
import {
  beatOf,
  clipTime,
  cut,
  findRow,
  footnote,
  type G,
  greyScene,
  keep,
  LEFT,
  type Play,
  rowMatch,
  splitRuns,
  step,
  win,
} from "../kit/grey";
import { span } from "../kit/motion";
import { Pace, unfocus } from "../kit/pace";
import {
  checkpoints,
  OPENS,
  RAIL,
  scrollOf,
  ZSHRC,
  zshrcLines,
} from "../kit/rest";
import { DUR, PANE_DIM } from "../kit/style";
import { termLayout } from "../kit/term";
import type { SceneCues } from "../score/cues";
import { captionsFor } from "../storyboard";
import { takePane } from "./g5-dotfiles-lock/pane";
import {
  focusPlan,
  inFocus,
  moveOr,
  perFacts,
  takeIn,
  termAct,
} from "./g5-dotfiles-lock/pacing";
import {
  checkpointRail,
  RAIL_DUR,
  rollbackBeats,
} from "./g5-dotfiles-lock/rail";
import {
  aliasLift,
  landsAt,
  LIFT,
  type LiftSource,
  zshrcCard,
} from "./g5-dotfiles-lock/zshrc";

/** Soft wraps joined to their line, cropped with it by the pane's fade. */
const SCREEN = { joinWraps: true } as const;

/** A history listing's rows, and a rollback plan's, which dim while the rail carries them. */
const HISTORY_ROW = /^(ID\s+When\s+Trigger|\d+\s+\d{4}-\d\d-\d\d \d\d:\d\d\s)/;
const PLAN_ROW = /^(Path\s+Action\s+From|\S+\s+(write|create|delete|remove)\s)/;
/** The command that appends the alias, and the text it quotes. */
const ECHO = /^~ \$ echo "(.*)" >> ~\/\.zshrc$/;

/**
 * How long each move on the card and the rail takes, beats: the first
 * node snapping in under the pointer's drop and its ring; a later node and
 * the pointer's step to it; the rollback's rewind, arc, node and ride; the
 * alias's copy lifting, flying and landing in its row; its row's seat mark
 * and fold out.
 */
const MOVES = {
  first: Math.max(
    RAIL_DUR.node,
    RAIL_DUR.pointerLag + RAIL_DUR.pointerIn,
    RAIL_DUR.ring,
  ),
  next: Math.max(RAIL_DUR.node, RAIL_DUR.step, RAIL_DUR.ring),
  rollback:
    RAIL_DUR.rewind -
    DUR.flick +
    RAIL_DUR.arc +
    Math.max(RAIL_DUR.ride, RAIL_DUR.ring, RAIL_DUR.node),
  alias: LIFT.rise + LIFT.form + LIFT.fly + LIFT.land,
  aliasOut: DUR.tick + DUR.fold,
  footnote: DUR.enter,
} as const;

/**
 * When everything happens (kit/pace.ts): the section's focal motions one
 * at a time, from the take; without it, the storyboard's plan.
 */
const schedule = perFacts((d: ReelData | null) => {
  const c = takeIn(d, "C15");
  const p = new Pace("track", { d });
  const act = (name: string, stepName: string, lines: number) =>
    termAct(p, name, c, (c, at) => [step(c, stepName, at)], { lines });
  const confirm = act("confirm", "confirm", 2);
  const answer = act("answer", "track", 4);
  const cp1 = moveOr(p, c, "cp1", MOVES.first);
  p.caption(0);
  const foot1 = moveOr(p, c, "foot1", MOVES.footnote);
  const edit = act("edit", "edit", 0);
  const alias = moveOr(p, c, "alias", MOVES.alias);
  const cp2 = moveOr(p, c, "cp2", MOVES.next);
  const history = act("history", "history", 1);
  const plan = act("rollbackPlan", "confirm-rollback", 1);
  const rollback = act("rollback", "rollback", 1);
  const aliasOut = moveOr(p, c, "aliasOut", MOVES.aliasOut);
  const cp3 = moveOr(p, c, "cp3", MOVES.rollback);
  p.caption(1);
  const foot2 = moveOr(p, c, "foot2", MOVES.footnote);
  // The dimmed tables brighten back and the focus lets go, for the rest.
  const undim = moveOr(p, c, "undim", DUR.dim);
  const acts = [confirm, answer, edit, history, plan, rollback];
  const plays: Play[] = c
    ? [cut(0, OPENS.C15!(c)), ...acts.flatMap((a) => a.plays)]
    : [];
  const at = (t: number | null) =>
    t === null || !plays.length ? Infinity : beatOf(plays, t);
  const rb = rollbackBeats(cp3.at);
  const outs = captionsFor("track", d, p.events()).map((x) => x.out);
  return {
    p,
    plays,
    /** The first node, the watcher's save, and the rollback's checkpoint. */
    made: [cp1.at, cp2.at, rb.made],
    rollback: rb,
    /** The copy lifts off the echo's line, and seats (its click). */
    lift: alias.at,
    seat: landsAt(alias.at),
    /** The history's table prints (it dims as the rail carries it). */
    listed: c
      ? at(firstWith(c, /^\d+\s+\d{4}-\d\d-\d\d/, stepOf(c, "history").enter))
      : history.end,
    /** The rollback's plan table prints. */
    plan: c
      ? at(firstWith(c, /^Path\s+Action/, stepOf(c, "confirm-rollback").enter))
      : plan.end,
    /** The alias's row takes a seat mark, and folds out a quarter beat later. */
    mark: aliasOut.at,
    fold: aliasOut.at + DUR.tick,
    // Each footnote leaves with its caption, the watcher's once the save
    // it qualifies has played (caption 1 holds to then: sections.json
    // `until`).
    notes: [
      [foot1.at, outs[0] + DUR.tick],
      [foot2.at, outs[1] + DUR.tick],
    ] as const,
    undim: undim.at,
    focus: focusPlan(
      [
        [confirm.at, "term"],
        [cp1.at, "rail"],
        [edit.at, "term"],
        [alias.at, "card"],
        [cp2.at, "rail"],
        [history.at, "term"],
        [aliasOut.at, "card"],
        [cp3.at, "rail"],
        [undim.at, "*"],
      ],
      acts.map((a) => [a.at, a.held] as const),
    ),
  };
});

/** The alias the echo quotes, as it was typed. */
function aliasOf(c: Capture | null): string | null {
  if (!c) return null;
  const end = c.frames[c.frames.length - 1].t;
  return rowMatch(c, end, ECHO)?.[1] ?? null;
}

/** Where the quoted text sits in the pane on the beat it lifts: its runs, column and baseline. */
function liftSource(c: Capture, ct: number, alias: string): LiftSource | null {
  const sc = screenAt(c, ct, SCREEN);
  const row = findRow(sc, ECHO);
  if (!row) return null;
  const col = row.text.indexOf(`"${alias}"`) + 1;
  const r = c.frames[sc.frame].rows.find((x) => rowText(c, x) === row.text);
  if (r === undefined || col < 1) return null;
  const [, tail] = splitRuns(rowRuns(c, r), col);
  const [mid] = splitRuns(tail, Array.from(alias).length);
  // The pane scrolls as drawScreen does (never back up): the same first line.
  const L = termLayout(
    LEFT,
    sc.lines.length,
    scrollOf(c, ct, LEFT, "monotonic", SCREEN),
  );
  return { runs: mid, x: L.col(col), y: L.baseline(row.line), size: LEFT.size };
}

/** A dimmed table row's alpha: dimmed from `from` (glide), bright again from `undim`. */
function tableAlpha(b: number, from: number, undim: number): number {
  const k =
    span(b, from, DUR.dim, "glide") * (1 - span(b, undim, DUR.dim, "glide"));
  return 1 - (1 - PANE_DIM.text) * k;
}

export const cues: SceneCues<"track"> = (facts: ReelFacts | null) => {
  const d = reelData(facts);
  if (!capture(d, "C15")) return {};
  const t = schedule(d);
  return {
    checkpoints: t.made,
    rollback: t.rollback.made,
    seat: t.seat,
  };
};

export const scene = greyScene(
  "track",
  (g: G) => {
    const { ctx, b } = g;
    const c = capture(g.d, "C15");
    const t = schedule(g.d);
    const alias = aliasOf(c);
    // The card, as the fixture, with the appended line while it is in the file.
    const row = keep(g, () =>
      inFocus(ctx, t.focus, "card", b, () =>
        zshrcCard(ctx, {
          rect: ZSHRC,
          lines: zshrcLines(g.d),
          alias,
          b,
          times: {
            lift: c ? t.lift : Infinity,
            mark: t.mark,
            fold: c ? t.fold : Infinity,
            // Its seat mark holds while the line is in the file (still,
            // never a motion of its own), and it folds out with it.
            saved: Infinity,
          },
        }),
      ),
    );
    // The rail: each checkpoint as the schedule makes it.
    keep(g, () =>
      inFocus(ctx, t.focus, "rail", b, () =>
        checkpointRail(ctx, RAIL, checkpoints(g.d), b, {
          made: c ? t.made : [],
          rewind: c ? t.rollback.rewind : undefined,
          arc: c ? t.rollback.arc : undefined,
        }),
      ),
    );
    // The terminal: the take at the schedule's times, its tables dimmed
    // while the rail carries them, and the pane dimmed out of focus.
    takePane(g, "C15", LEFT, () => t.plays, {
      screen: SCREEN,
      dim: unfocus(t.focus, "term", b),
      lineAlpha: c
        ? (i, lines) =>
            HISTORY_ROW.test(lines[i])
              ? tableAlpha(b, t.listed, t.undim)
              : PLAN_ROW.test(lines[i])
                ? tableAlpha(b, t.plan, t.undim)
                : 1
        : undefined,
    });
    // The alias's copy in flight, over everything.
    if (c && alias !== null) {
      const ct = clipTime(t.plays, t.lift).ct;
      aliasLift(ctx, liftSource(c, ct, alias), row, alias, b, t.lift);
    }
    footnote(
      ctx,
      "Autosave needs the history watcher, `mise dot watch`.",
      win(b, t.notes[0][0], t.notes[0][1]),
    );
    footnote(
      ctx,
      "Share the history with `mise dot origin set`.",
      win(b, t.notes[1][0], t.notes[1][1]),
    );
  },
  { events: (d) => schedule(d).p.events() },
);

/** The schedule, for the pacing checks. */
export const trackSchedule = schedule;
