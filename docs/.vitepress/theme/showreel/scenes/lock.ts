// lock (plan v3 §3, Act VI; ART.md §12.3–12.4; STORYBOARD.md "Pacing"):
// C17. The ticket leaves the api card (C17, `[tools]` open) and the
// terminal at `~/work/api $`. One focal motion at a time, half a beat
// apart (kit/pace.ts Pace), whatever is not the focus dimmed, and never the
// terminal while its output is read:
//
// `mise lock` types at TYPE_RATE and prints the platforms it targets and
// the tools it will process; the shot holds there, before its first
// progress row (those carry byte counts and elapsed times at real speed
// and end on checked rows and totals). The station card closes up round
// its `node = …` line into the request card (it keeps its title) at the
// foot of the right column and travels left, under the terminal, which
// shortens to the slip pane's height (ART.md §3 Focus), its three lines
// all it has to show. Half a beat after the request has left the right
// column the ledger, mise.lock as C17 wrote it, slides in to fill it, its
// entries folded to a line each under their provenance badges; half a
// beat after that node's entry opens (its version, specifiers and one
// clipped checksum row per platform). Once the terminal's lines have been
// read it dims. A thread runs from the
// request's line ("asks for") through the gutter to the version row, which
// takes a seat mark ("records"), and caption 1 follows it.
// Then the stage clears for "Commit both.": the thread lets go, the
// terminal leaves, and the request and the ledger shrink into their
// `mise.toml` and `mise.lock` chips. The CI panel (the fixture's workflow,
// as wide as it needs) rises where the terminal stood; once it is up,
// each chip flies an arc to dock by the row that reads it: `uses:
// jdx/mise-action@v4` (the action installs what mise.toml asks for) and
// `install_args: --locked` (pinned to what mise.lock records), each row
// lighting as its chip lands. No tick: no CI run was recorded. Then `run:
// mise run ci` lights, caption 2 follows, and only once it has landed the
// panel pushes in, still 1.5 beats before the reel's one whip (whip.ts),
// and caption 2 has gone before the whip winds up.
//
// `cues` gives the score the beats the drawing uses.

import { BEAT, type LitRect, sec } from "../bible";
import {
  type Capture,
  fileOf,
  fixture,
  lastBefore,
  type ReelData,
  reelData,
  stepOf,
} from "../captures";
import { ciDock, ciPanel, ciRect } from "../kit/diagrams/ci-panel";
import { CHIP_TIMES, fileChipSize, ledger } from "../kit/diagrams/ledger";
import {
  CARD,
  cut,
  type G,
  greyScene,
  keep,
  LEFT,
  type Play,
  rectOf,
  step,
} from "../kit/grey";
import type { Rect } from "../kit/motion";
import { exit, span } from "../kit/motion";
import { Pace, restOut, unfocus } from "../kit/pace";
import { OPENS } from "../kit/rest";
import { DUR, LAYOUT, PANES } from "../kit/style";
import { lerpPane, type Pane, termLit } from "../kit/term";
import type { SceneCues } from "../score/cues";
import { WHIP_AT, WHIP_START } from "../whip";
import { takePane } from "./g5-dotfiles-lock/pane";
import {
  focusPlan,
  inFocus,
  moveOr,
  perFacts,
  takeIn,
  termAct,
} from "./g5-dotfiles-lock/pacing";

/** The card closing up round its line into the request (DUR.fold) and travelling left (DUR.move). */
const REQUEST_DUR = DUR.fold + DUR.move;
/** The ledger sliding in (DUR.enter), its entries' headers a sixteenth apart inside it (ledger.ts). */
const LEDGER_DUR = DUR.enter;
/** node's entry opening: its rows part (DUR.seatGap), write in staggered, and its checksums write on (ledger.ts rowStates). */
const ENTRY_DUR = DUR.seatGap + DUR.staggerMax + DUR.flick + DUR.half;
/** The CI panel rising (ci-panel.ts, DUR.enter): the chips fly once it is up. */
const RISE_DUR = DUR.enter;
/** The thread draws on (DUR.thread) and the version row takes its seat mark (DUR.tick). */
const THREAD_DUR = DUR.thread + DUR.tick;
/** The mise.lock chip flies this long after mise.toml's, so it passes under it. */
const CHIP_LAG = DUR.tick;
/** The stage clears: the thread lets go, the terminal leaves, and both files shrink into their chips. */
const CLEAR_DUR = Math.max(
  DUR.threadFade,
  DUR.exit,
  CHIP_TIMES.anticipate + CHIP_TIMES.shrink,
);
/** The chips fly and land (mise.lock CHIP_LAG after mise.toml), and their rows light (DUR.seat). */
const DROP_DUR = CHIP_LAG + CHIP_TIMES.fly + DUR.seat;
/** The whip winds up this long before the bar line (whip.ts), after the section's last still frame. */
const WHIP_LEAD = (WHIP_AT - WHIP_START) / BEAT;

/** The rows the chips dock by, and the step that runs after them. */
const USES = /uses: jdx\/mise-action@/;
const LOCKED = /install_args: --locked/;
const RUN = /run: mise run ci/;

/**
 * The stage while the files stand side by side: the terminal shortened to
 * the slip pane (y 120–450), the request under it where packslip's slip
 * tucks (LAYOUT.slip's top, the left column's width), the ledger the whole
 * right column. The request closes up at the right column's foot first
 * (VIA), clear of the shortening pane, then travels left.
 */
const SHORT: Pane = PANES.slip;
const REQUEST: Rect = {
  x: LAYOUT.left.x,
  y: LAYOUT.slip.y,
  w: LAYOUT.left.w,
  h: LAYOUT.request.h,
};
const VIA: Rect = { ...REQUEST, x: CARD.x, w: CARD.w };
const LEDGER: Rect = LAYOUT.right;

/**
 * The ledger's excerpt note (honesty): one entry opens, the others fold to
 * annotations, and a platform's table is condensed to a row.
 */
const LEDGER_TAG = "Excerpt · rows condensed";

/** The last frame before `mise lock`'s first progress row or provenance download. */
const hold = (c: Capture): number => {
  const s = stepOf(c, "lock");
  return lastBefore(c, /^lock\b|^mise downloading/, s.enter, s.end);
};

/**
 * When everything happens (kit/pace.ts): the section's focal motions one
 * at a time, from the take; without it, the storyboard's plan.
 */
const schedule = perFacts((d: ReelData | null) => {
  const c = takeIn(d, "C17");
  const p = new Pace("lock", { d });
  const lock = termAct(
    p,
    "lock",
    c,
    (c, at) => [step(c, "lock", at, { until: hold(c) })],
    { lines: 2 },
  );
  const req = moveOr(p, c, "request", REQUEST_DUR);
  const led = moveOr(p, c, "ledger", LEDGER_DUR);
  const entry = moveOr(p, c, "entry", ENTRY_DUR);
  const thread = moveOr(p, c, "thread", THREAD_DUR);
  p.caption(0);
  const clear = moveOr(p, c, "panel", CLEAR_DUR);
  // The CI panel rises where the terminal stood; the chips fly once it is
  // up (never while it moves).
  const drop = moveOr(p, c, "drop", RISE_DUR + DROP_DUR);
  const fly = drop.at + RISE_DUR;
  const requestDock = fly + CHIP_TIMES.fly;
  const lockDock = requestDock + CHIP_LAG;
  // Caption 1 holds into caption 2, anchored half a beat after `run` (a
  // swap: one caption change), which then has its read before the whip
  // winds up.
  const run = moveOr(p, c, "run", DUR.seat);
  const cap = p.caption(1);
  // The slow push-in, once caption 2 has landed, still for the rest before the whip.
  const still = sec("lock").beats - WHIP_LEAD - restOut();
  const pushAt = p.next(cap.end);
  const push = still > pushAt ? moveOr(p, c, "push", still - pushAt) : null;
  const plays: Play[] = c ? [cut(0, OPENS.C17!(c)), ...lock.plays] : [];
  return {
    p,
    plays,
    /** The card closes into the request (and the pane shortens); the ledger slides in; node's entry opens. */
    morph: req.at,
    ledger: led.at,
    unfold: entry.at,
    thread: thread.at,
    /** The thread lets go and the terminal leaves; both files shrink into chips. */
    unthread: clear.at,
    leave: clear.at,
    chip: clear.at,
    /** The CI panel rises; once it is up the chips fly to their docks. */
    ci: drop.at,
    fly,
    requestDock,
    lockDock,
    run: run.at,
    push: push?.at,
    pushFor: push ? push.end - push.at : undefined,
    focus: focusPlan(
      [
        [lock.at, "term"],
        [req.at, "ledger"],
        [clear.at, "*"],
      ],
      [[lock.at, lock.held]],
    ),
  };
});

const plays = (d: ReelData | null) => (): readonly Play[] => schedule(d).plays;

/**
 * The platforms line fills all 80 columns and wraps (`…linux-arm64-mus` /
 * `l, linux-x64, …`): joined to its line, the side pane's fade crops the
 * rest with it (a viewport crop, ART.md §13) instead of leaving an orphan
 * row. Only that line: `→ Processing …` is 80 columns too.
 */
const SCREEN = { joinWraps: /^→ Targeting/ } as const;

/** The terminal's presence: up from the ticket, leaving as the stage clears. */
const paneAt = (d: ReelData | null, b: number) =>
  exit(b, schedule(d).leave, DUR.exit);
/** The terminal's window: shortening to SHORT as the card closes into the request. */
const paneOf = (d: ReelData | null, b: number): Pane =>
  lerpPane(LEFT, SHORT, span(b, schedule(d).morph, DUR.move, "move"));

export const cues: SceneCues<"lock"> = (facts) => {
  const t = schedule(reelData(facts));
  return {
    ledger: t.ledger,
    thread: t.thread + DUR.thread,
    dock: [t.requestDock, t.lockDock],
    run: t.run,
  };
};

/** The lit screen: the terminal while it is up. */
const lit = (d: ReelData | null, b: number): LitRect | null => {
  const a = paneAt(d, b).alpha;
  return a > 0 ? termLit(rectOf(paneOf(d, b)), a) : null;
};

export const scene = greyScene(
  "lock",
  (g: G) => {
    const { ctx, b } = g;
    const t = schedule(g.d);
    // The CI panel: where its rows stand on this frame, for the chips' docks.
    const yml = fixture(g.d, "api/.github/workflows/ci.yml");
    const chip = fileChipSize();
    const rect =
      yml === null
        ? LAYOUT.ci
        : ciRect(yml, [
            { match: USES, w: chip.w },
            { match: LOCKED, w: chip.w },
          ]);
    const panel = {
      yml,
      b,
      enter: t.ci,
      push: t.push,
      pushFor: t.pushFor,
      rect,
      lights: [
        { match: USES, at: t.requestDock },
        { match: LOCKED, at: t.lockDock },
        { match: RUN, at: t.run },
      ],
    };
    const rows = ciPanel(ctx, { ...panel, alpha: 0 });
    const dock = ciDock(rows, LOCKED);
    const requestDock = ciDock(rows, USES);
    // The card closing into the request, the ledger, the thread, the chips.
    const toml = fileOf(g.d, "C17", "work/api/mise.toml");
    const drawLedger = () =>
      keep(g, () =>
        inFocus(ctx, t.focus, "ledger", b, () =>
          ledger(ctx, {
            lock: fileOf(g.d, "C17", "work/api/mise.lock"),
            toml,
            b,
            rect: LEDGER,
            request: REQUEST,
            tag: LEDGER_TAG,
            enter: t.ledger,
            unfold: t.unfold,
            thread: t.thread,
            unthread: t.unthread,
            chip: dock
              ? {
                  at: t.chip,
                  fly: t.fly + CHIP_LAG,
                  dock,
                  handles: (a, z) => {
                    const out = chip.w + 4 * chip.h;
                    return [
                      { x: a.x + out, y: z.y },
                      { x: z.x + out, y: z.y },
                    ];
                  },
                }
              : undefined,
            // The chips are routed round the workflow's lines, never over
            // them, and never across each other (checked frame by frame at
            // 60 fps): mise.toml, from the panel's far side, drops under the
            // panel's foot, runs along under it and rises into its dock past
            // the lines' ends; mise.lock, landing after it on the row below,
            // drops early, swings out past the panel's right and comes in
            // level with its row, under the docked mise.toml.
            requestChip: requestDock
              ? {
                  at: t.chip,
                  fly: t.fly,
                  dock: requestDock,
                  handles: (a, z) => {
                    const under = rect.y + rect.h + 2 * chip.h;
                    return [
                      { x: a.x, y: under },
                      { x: z.x + 3 * chip.h, y: under },
                    ];
                  },
                }
              : undefined,
            fit: true,
            morph: {
              at: t.morph,
              via: VIA,
              card: {
                rect: CARD,
                title: "~/work/api/mise.toml",
                text: toml,
                from: "C17",
                open: ["[tools]"],
              },
            },
          }),
        ),
      );
    // The terminal: shortened as the card closes into the request, and
    // dimmed out of focus once `mise lock`'s lines have been read.
    const p = paneAt(g.d, b);
    const pane = () =>
      takePane(g, "C17", paneOf(g.d, b), plays(g.d), {
        alpha: p.alpha,
        dy: p.dy,
        dim: unfocus(t.focus, "term", b),
        screen: SCREEN,
      });
    // The bar line's order (card, then terminal) until the panel rises;
    // from then the ledger, the request and their chips stand over it.
    if (b < t.ci) {
      drawLedger();
      pane();
    } else {
      pane();
      ciPanel(ctx, panel);
      drawLedger();
    }
  },
  {
    lit: (b, d) => lit(d, b),
    events: (d) => schedule(d).p.events(),
  },
);

/** The schedule, for the pacing checks. */
export const lockSchedule = schedule;
