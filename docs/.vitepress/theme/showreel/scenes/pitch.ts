// pitch (plan v3 §3, Act 0; ART.md §7, §9; STORYBOARD.md "Pacing"): the
// terminal waits at `~/work $` from the first frame, held from the open,
// while the station card rises in beside it and writes api's mise.toml as
// C2 left it, top to bottom (the terminal dimmed: one focus at a time).
// Its three tables light in their pillars' colours in turn, and the first
// caption names them ("tools," "env vars," "tasks."), so the pitch teaches
// the palette. Then C2 plays, typed at TYPE_RATE, each result held for its
// read with the card dimmed: `cd api` (the chrome's cwd dot hops), `node
// --version`, whose version lifts out of its row onto the card row that
// asked for it, with its thread; the env var, lifted the same way; and
// `mise run ci`, which holds on its last frame before `Finished in`. The
// chips and their threads leave before the bar line: the card, all three
// tables lit, and the terminal on that frame hold into the ticket.

import type { ReelFacts } from "../bible";
import {
  type Capture,
  capture,
  type CaptureId,
  fileOf,
  firstWith,
  type ReelData,
  reelData,
  stepOf,
} from "../captures";
import {
  beatOf,
  card,
  cut,
  greyScene,
  keep,
  LEFT,
  paneWindow,
  type Play,
  shot,
  step,
} from "../kit/grey";
import { bump, enter, span } from "../kit/motion";
import { type CardRows, tomlBlocks } from "../kit/parts";
import { focusOf, focusPlan, Pace, readsOf, unfocus } from "../kit/pace";
import { HOLDS, OPENS } from "../kit/rest";
import { CARD_ART, DUR, LAYOUT, TYPE } from "../kit/style";
import { advance } from "../kit/term";
import type { SceneCues } from "../score/cues";
import {
  drawLift,
  fileIn,
  type Lift,
  LIFT_LANDS,
  perFacts,
  routeInto,
  takeIn,
} from "./g2-tools/common";

const CARD = LAYOUT.card;
const TITLE = "~/work/api/mise.toml";
const OPEN = ["[tools]", "[env]", "[tasks.ci]"] as const;
/** The ci task's `run` line is wider than the card: it folds to "1 more line" (the output shows it). */
const HIDE = /^run = /;
/** The tables, by the header prefix card() lights them by. */
const TABLES = ["[tools]", "[env]", "[tasks"] as const;

/** The card rises in on the downbeat, and starts writing a sixteenth later. */
const CARD_IN = 0;
const WRITE_AT = 1 / 16;
/**
 * It writes on over a beat, a row at a time, each row its characters'
 * share: a file appearing, not a take being typed; it then holds all
 * section, its tables lit in turn and named by the caption.
 */
const WRITE = 1;
/** The tables light a quarter beat apart. */
const LIT_EACH = DUR.tick;

/** How many characters each of the card's rows types (card() writes rows on in turn, each over its share of `reveal`). */
function rowLengths(text: string): number[] {
  const cols = Math.floor(
    (CARD.w - 2 * CARD_ART.inset) / advance(TYPE.card.size) + 1e-9,
  );
  const len = (s: string) => Array.from(s).length;
  const blocks = tomlBlocks(text);
  const open = (h: string) => OPEN.some((p) => h.startsWith(p));
  const out: number[] = [];
  for (let i = 0; i < blocks.length; i++) {
    const blk = blocks[i];
    out.push(len(blk.header));
    if (open(blk.header)) {
      const hidden = blk.body.filter((l) => len(l) > cols || HIDE.test(l));
      for (const l of blk.body) if (!hidden.includes(l)) out.push(len(l));
      // The fold's annotation pill: set whole, a short beat.
      if (hidden.length) out.push(4);
      continue;
    }
    // Folded task tables fold together, into their first header.
    while (
      /^\[tasks\./.test(blk.header) &&
      i + 1 < blocks.length &&
      /^\[tasks\./.test(blocks[i + 1].header) &&
      !open(blocks[i + 1].header)
    )
      i++;
  }
  return out;
}

/**
 * When everything happens (kit/pace.ts): the section's focal motions one at
 * a time from the take, the captions anchored on them, and the focus.
 */
const schedule = perFacts((d: ReelData | null) => {
  const c = takeIn(d, "C2");
  const p = new Pace("pitch", { d, from: 0 });
  const none = { plays: [] as Play[] };
  const cardS = p.move("card", WRITE);
  const tables = p.move("tables", 2 * LIT_EACH + DUR.light);
  p.caption(0);
  const term = (
    name: string,
    step0: string,
    lines: number,
    dur: number,
    until?: (c: Capture) => number,
  ) =>
    c
      ? p.term(name, (at) => [step(c, step0, at, { until: until?.(c) })], {
          lines,
        })
      : { ...p.move(name, dur), ...none };
  const cd = term("cd", "cd", 0, 1.42);
  const node = term("node", "node", 1, 2.14);
  const nodeLift = p.move("nodeLift", LIFT_LANDS + DUR.thread);
  const env = term("env", "env", 1, 2.75);
  const envLift = p.move("envLift", LIFT_LANDS + DUR.thread);
  p.caption(1);
  const ci = term("ci", "ci", 4, 1.52, (c) => HOLDS.C2!(c));
  p.caption(2);
  const chipsOut = p.move("chipsOut", DUR.threadFade);
  const lit = [0, 1, 2].map((i) => tables.at + i * LIT_EACH);
  // The card writes on at one pace over its span.
  const text = fileIn(d, "C2", "work/api/mise.toml");
  const lens = text ? rowLengths(text) : [];
  const total = lens.reduce((a, n) => a + n, 0) || 1;
  const perChar = (cardS.end - WRITE_AT) / total;
  const starts: number[] = [];
  let at = WRITE_AT;
  for (const n of lens) {
    starts.push(at);
    at += n * perChar;
  }
  const reveal = (b: number): number => {
    if (!lens.length || b >= at) return 1;
    let i = 0;
    while (i + 1 < starts.length && starts[i + 1] <= b) i++;
    const f = (b - starts[i]) / (lens[i] * perChar);
    return Math.max(0, (i + Math.min(1, Math.max(0, f))) / lens.length);
  };
  let plays: Play[] = [];
  let cdAt = cd.end;
  let ciAt = ci.end;
  if (c) {
    plays = [
      cut(0, OPENS.C2!(c)),
      ...cd.plays,
      ...node.plays,
      ...env.plays,
      ...ci.plays,
    ];
    const cds = stepOf(c, "cd");
    cdAt = beatOf(plays, firstWith(c, /^~\/work\/api \$/, cds.type) ?? cds.end);
    const cis = stepOf(c, "ci");
    ciAt = beatOf(plays, firstWith(c, /^\[\w+\] /, cis.enter) ?? cis.end);
  }
  // The card has the focus as it writes and as each value lands on it;
  // the terminal while it acts, and until its output has been read
  // (rule 3); both on the bar lines.
  const focus = focusPlan(
    [
      [cardS.at, "card"],
      [cd.at, "term"],
      [nodeLift.at, "card"],
      [env.at, "term"],
      [envLift.at, "card"],
      [ci.at, "term"],
      [chipsOut.at, "*"],
    ],
    readsOf(cd, node, env, ci),
  );
  return {
    p,
    lit,
    reveal,
    plays,
    cd: cdAt,
    ci: ciAt,
    lifts: [nodeLift.at, envLift.at] as const,
    liftOut: chipsOut.at,
    focus,
  };
});

/** Each lift's value in the pane, and the card row that asked for it. */
const LIFTS: { value: RegExp; row: RegExp }[] = [
  { value: /^v\d+\.\d+\.\d+$/, row: /^node = / },
  { value: /^APP_ENV=\S+$/, row: /^APP_ENV = / },
];

export const scene = greyScene(
  "pitch",
  (g) => {
    const { ctx, b } = g;
    const c = capture(g.d, "C2");
    const S = schedule(g.d);
    const cardK = focusOf(S.focus, "card", b);
    // The card: rising in, writing on, its tables lighting as the caption names them.
    const up = enter(b, CARD_IN, DUR.enter);
    const lit: Record<string, number> = {};
    const ignite: Record<string, number> = {};
    TABLES.forEach((t, i) => {
      lit[t] = span(b, S.lit[i], DUR.light, "linear");
      ignite[t] = bump(b, S.lit[i], DUR.ignite);
    });
    const reveal = S.reveal(b);
    const rows: CardRows = keep(g, () =>
      card(
        ctx,
        { ...CARD, y: CARD.y + up.dy },
        {
          title: TITLE,
          text: fileOf(g.d, "C2", "work/api/mise.toml"),
          from: "C2",
          open: OPEN,
          hide: HIDE,
          lit,
          ignite,
          reveal: reveal < 1 ? reveal : undefined,
          alpha: up.alpha * cardK,
          // As tall as its rows, growing as they write on (kit/rest.ts pitch|tools).
          fit: true,
        },
      ),
    );
    // The terminal: held from the open, the take played on the section's
    // beats, its window as tall as the take needs (kit/rest.ts open|pitch):
    // it grows as `mise run ci` prints. Without the take, the window under
    // the missing card, as the bar lines draw it.
    if (!c) paneWindow(g, LEFT);
    const s = shot(g, "C2", LEFT, -1, g.s.beats + 1, () => S.plays, {
      keep: true,
      fit: true,
      focus: (b) => unfocus(S.focus, "term", b),
    });
    if (!s) return;
    // The lifts: the version and the env var out of their rows, onto the
    // card rows that asked for them, dimmed with the card once landed.
    LIFTS.forEach((l, i) => {
      const at = S.lifts[i];
      if (b < at) return;
      const line = s.screen.lines.findIndex((r) =>
        l.value.test(r.map((x) => x.text).join("")),
      );
      const row = rows.find((r) => r.kind === "body" && l.row.test(r.text));
      if (line < 0 || !row) return;
      const runs = s.screen.lines[line];
      const n = runs.reduce((k, r) => k + Array.from(r.text).length, 0);
      const cell = s.layout.cell(line, 0);
      const from = { x: cell.x, y: s.layout.baseline(line), size: s.pane.size };
      const rowMid = (row.top ?? row.y - 26) + (row.h ?? 38) / 2;
      const lift: Lift = {
        runs,
        from,
        to: { x: row.x + (row.w ?? 0) + 20, y: row.y },
        at,
        out: S.liftOut,
        route: routeInto(
          { x: cell.x + n * s.layout.advance, y: cell.y + cell.h / 2 },
          { x: CARD.x + CARD_ART.anchorX, y: rowMid },
        ),
      };
      ctx.save();
      ctx.globalAlpha *= b < at + LIFT_LANDS ? 1 : cardK;
      drawLift(ctx, lift, b);
      ctx.restore();
    });
  },
  { events: (d) => schedule(d).p.events() },
);

/** The score's cues (score/cues.ts), where the picture has them. */
export const cues: SceneCues<"pitch"> = (facts: ReelFacts | null) => {
  const S = schedule(reelData(facts));
  return {
    tables: S.lit,
    cd: S.cd,
    ci: S.ci,
    lands: S.lifts.map((a) => a + LIFT_LANDS),
  };
};

/** The section's schedule (kit/pace.ts Pace) and its takes' plays, for the pacing checks. */
export const pacing = (
  facts: ReelFacts | null,
): { pace: Pace; takes: Partial<Record<CaptureId, readonly Play[]>> } => {
  const S = schedule(reelData(facts));
  return { pace: S.p, takes: { C2: S.plays } };
};
