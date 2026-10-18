// packslip (plan v3 §3, Act II; ART.md §12.6, §12.7; STORYBOARD.md
// "Pacing"): the station card switch's api/ folder became stands beside
// the terminal, holding api's mise.toml as C7 left it. One focal motion at
// a time, whatever is not the focus dimmed: switch's last screen dissolves
// into C8's prompt; hk's row seats in [tools] (C8 started with hk pinned at
// the old version) as [tools] lights. The terminal shortens and a slip
// comes down out from under it, tab strip first: an excerpt of hk's real
// packslip, the statement mise saved when it installed that version (C8's
// off-camera files), its identity outlined as the caption says "signed",
// then its completion shells and skills as it names them. C8 plays typed
// at TYPE_RATE, each result held for its read: `hk check --j` and Tab (a
// keycap, rising in the beat before the key) list two flags; Ctrl-C (the
// take's idle gap cut), `mise use hk@…` (its progress header cut) prints
// its tools line; the card's value turns to the new version, then the slip
// re-stamps from the new version's statement; Tab again lists three, and
// `--junit-xml` is outlined, then the footnote on where Tab completes.
// `mise skills sync` prints its `linked` rows and the pane dims; the slip
// hands its chips on to the drawn link in its place: the skill names fly
// into the link's project paths, and the new version's chip into the
// install path's version segment, which lights on its landing; the rest of
// the slip fades where it stands. Each arrow then draws on the celesta's G
// G G F as hk's row lights again on the card. Once the caption is read the
// link folds away and [tools] unlights, and the card and the dimmed,
// shortened terminal hold into the Environments ticket (kit/rest.ts
// packslip|env).

import { BEAT, PALETTE, PILLAR, type ReelFacts } from "../bible";
import {
  type Capture,
  capture,
  type CaptureId,
  fileOf,
  firstWith,
  frameAfter,
  keyAt,
  type ReelData,
  reelData,
  screenAt,
  stepOf,
} from "../captures";
import { bumpOver, mono, pill } from "../kit/diagrams/common";
import {
  linkedRows,
  linkLayout,
  skillsLink,
} from "../kit/diagrams/skills-link";
import { type SlipChip, slip, slipChips } from "../kit/diagrams/slip";
import {
  beatOf,
  card,
  cut,
  findRow,
  footnote,
  greyScene,
  keep,
  keycap,
  keycapOn,
  on,
  type Play,
  play,
  playLen,
  rectOf,
  restScreen,
  rowMatch,
  step,
  TYPE_RATE,
  typed,
  win,
} from "../kit/grey";
import {
  focusOf,
  focusPlan,
  GAP,
  Pace,
  readHold,
  readsOf,
  unfocus,
} from "../kit/pace";
import { arc, curveAt, lerpRect, span } from "../kit/motion";
import { drawPaneWindow, HOLDS } from "../kit/rest";
import {
  CHROME_H,
  COLOR,
  DUR,
  PANES,
  PILLAR_BAND,
  RADIUS,
  STAMP,
  STROKE,
  TYPE,
} from "../kit/style";
import { lerpPane, type Pane, run, termLit } from "../kit/term";
import type { SceneCues } from "../score/cues";
import { captionsFor } from "../storyboard";
import { roundedRect } from "../fx";
import { clamp, lerp } from "../math";
import { flipCard } from "./g3-versions-env/card";
import {
  cardOptions,
  restCard,
  restPane,
  STATION,
  STATION_KEPT,
} from "./g3-versions-env/rests";
import { perFacts, takeIn } from "./g2-tools/common";
import { resultOf, term } from "./g3-versions-env/take";

/** How the drawn link builds on the slip's chips (ART.md §12.7), from its start, beats. */
const LINK = {
  /** The chips fly from the slip from here (the slip hands them on)... */
  fly: DUR.flick,
  /** ...and land in it: the link forms round them... */
  form: 5 / 8,
  /** ...and its arrows draw on the celesta's G G G F, half a beat each. */
  draw: 1,
} as const;
/** hk's row seats (the rows part, the line drops) over half a beat, then [tools] lights. */
const SEAT = DUR.half;

/** The Tab keycap's top, this far under the chrome bar. */
const TAB_TOP = 30;
/** The bar line it holds into: its card and its pane (kit/rest.ts packslip|env). */
const OUT = "packslip|env";
/** The card: as the bar line keeps it (C8's file), and before hk seats, the station card switch's folder became. */
const CARD_L = restCard(OUT)!;
/** The terminal: the pane switch holds it in (versions|switch), shortening to the one packslip|env keeps. */
const FROM = restPane("switch|packslip").pane;
const TO = restPane(OUT);

/** The shell the takes ran in, as C3's `command not found` names it, or null. */
function shellOf(d: ReelData | null): string | null {
  const c = capture(d, "C3");
  if (!c) return null;
  const t = firstWith(c, /: command not found/);
  return t === null
    ? null
    : (rowMatch(c, t, /^(\w+): command not found/)?.[1] ?? null);
}

/** The terminal on beat `b`: switch's pane, shortened as the slip comes. */
function paneAt(b: number, shorten: number): Pane {
  const k = span(b, shorten, DUR.move, "move");
  return k <= 0 ? FROM : k >= 1 ? TO.pane : lerpPane(FROM, TO.pane, k);
}

/** The first key typed after the Ctrl-C that closes a Tab list, less a tenth: the next command's typing, its idle gap cut. */
const afterCtrlC = (c: Capture, from: number): number =>
  keyAt(c, "m", keyAt(c, "\x03", from)) - 0.1;

/**
 * `hk check --j` and Tab, from beat `at` (from take time `from`, default
 * the step's first frame): the keys at TYPE_RATE, then the typed command
 * held while the Tab keycap rises (DUR.keycapLead), Tab, and its list.
 */
function tabPlays(c: Capture, name: string, at: number): Play[] {
  const s = stepOf(c, name);
  const keys = c.keys.filter(([t]) => t > s.start && t < s.enter);
  const last = keys.length ? frameAfter(c, keys[keys.length - 1][0]) : s.type;
  const from = Math.min(s.type, last);
  const typing = (last - from) / (BEAT * TYPE_RATE);
  return [
    play(at, from, last, TYPE_RATE),
    typed(
      at + typing,
      Math.max(last, s.enter - 0.02),
      s.enter,
      Math.min(s.end, resultOf(c, name) + 0.1),
      { pause: DUR.keycapLead },
    ),
  ];
}

/**
 * `mise use hk@…` from beat `at`, typed from the frame after the Ctrl-C,
 * at TYPE_RATE, a beat of anticipation, Enter; the progress header it
 * flashes (a version installed beforehand) is cut: the entered command
 * holds for its real time and the tools line prints with the prompt after
 * it.
 */
function usePlays(c: Capture, at: number): Play[] {
  const u = stepOf(c, "use");
  const tools = firstWith(c, /tools: hk@/, u.enter) ?? u.end;
  // The prompt comes back a frame after the tools line: they land as one.
  const prompt = firstWith(c, /^~\/work\/\w+ \$/, tools + 1e-6) ?? tools;
  const t = typed(at, afterCtrlC(c, u.start), u.enter, u.enter + 0.005);
  return [t, cut(at + playLen(t) + (tools - u.enter - 0.005) / BEAT, prompt)];
}

/**
 * When everything happens (kit/pace.ts): C8's commands and the card's,
 * slip's and link's answers one at a time, the captions anchored on them,
 * and the focus. Without the take, the plan's own beats (sections.json).
 */
const schedule = perFacts((d: ReelData | null) => {
  const c = takeIn(d, "C8");
  const p = new Pace("packslip", { d, from: 0 });
  const none = { plays: [] as Play[] };
  // Without the take, a command's span from the plan, and its read hold.
  const held = (s: { at: number; end: number }, lines: number) => ({
    ...s,
    ...none,
    held: s.end + readHold(lines),
  });
  const dissolve = p.move("dissolve", DUR.tick, { after: 0 });
  const pin = p.move("pin", SEAT + DUR.light);
  const slipS = p.move("slip", DUR.tick + DUR.enter + DUR.tick);
  p.caption(0);
  const tabOld = c
    ? p.term("tabOld", (at) => tabPlays(c, "tab-old", at), { lines: 2 })
    : held(p.move("tabOld", 1.85), 2);
  // The Ctrl-C that closes a list clears it: the list holds its read first.
  p.wait(tabOld.held);
  const use = c
    ? p.term("use", (at) => usePlays(c, at), { lines: 1 })
    : { ...p.move("use", 2.13), ...none };
  const flip = p.move("flip", DUR.std);
  const restamp = p.move("restamp", DUR.std);
  const tabNew = c
    ? p.term("tabNew", (at) => tabPlays(c, "tab-new", at), { lines: 3 })
    : held(p.move("tabNew", 1.84), 3);
  const junit = p.move("junit", DUR.light);
  p.caption(1);
  const foot = p.move("foot", DUR.enter);
  // The new list, its `--junit-xml` and the footnote hold a beat longer:
  // the list is the act's payoff, read against the old one.
  p.wait(p.settled + GAP + 1);
  p.wait(tabNew.held);
  const sync = c
    ? p.term(
        "sync",
        (at) => [
          step(c, "sync", at, {
            from: afterCtrlC(c, stepOf(c, "sync").start),
            until: HOLDS.C8!(c),
          }),
        ],
        // Its `linked` rows are texture: the drawn link reads them.
        { lines: 0 },
      )
    : { ...p.move("sync", 2.36), ...none };
  const n = 2;
  const link = p.move("link", LINK.draw + n * DUR.half + DUR.light);
  const c3 = p.caption(2);
  // The caption leaves once read (sections.json `until`); the link folds
  // half a beat after its wipe, and the section rests on the card.
  p.wait(c3.out + DUR.tick + DUR.half);
  const out = p.move("linkOut", DUR.fold);
  const plays: Play[] = c
    ? [
        cut(0, stepOf(c, "tab-old").type),
        ...tabOld.plays,
        ...use.plays,
        ...tabNew.plays,
        ...sync.plays,
      ]
    : [];
  // The beats the takes' keys and results land on.
  const beat = (t: number | null, fallback: number) =>
    c && t !== null ? beatOf(plays, t) : fallback;
  const tab = beat(c && keyAt(c, "\t", stepOf(c, "tab-old").start), tabOld.end);
  const tab2 = beat(
    c && keyAt(c, "\t", stepOf(c, "tab-new").start),
    tabNew.end,
  );
  const listed = beat(c && resultOf(c, "tab-old"), tabOld.end);
  const listed2 = beat(c && resultOf(c, "tab-new"), tabNew.end);
  const linked = beat(
    c && (firstWith(c, /^linked /, stepOf(c, "sync").enter) ?? null),
    sync.end,
  );
  const tools = beat(
    c && (firstWith(c, /tools: hk@/, stepOf(c, "use").enter) ?? null),
    use.end,
  );
  // What has the focus: the card as hk seats and flips, the slip as it
  // comes and re-stamps, the terminal while it acts, the drawn link (and
  // hk's row lighting on the card) at the end; on packslip|env the card
  // is lit and the terminal dimmed, as the bar line keeps them.
  // The terminal stays lit until each output has been read (rule 3).
  const focus = focusPlan(
    [
      [pin.at, "card"],
      [slipS.at, "slip"],
      [tabOld.at, "term"],
      [flip.at, "card"],
      [restamp.at, "slip"],
      [tabNew.at, "term"],
      [link.at, ["link", "card"]],
      [out.at, "card"],
    ],
    readsOf(tabOld, use, tabNew, sync),
  );
  return {
    p,
    plays,
    focus,
    seatFrom: pin.at,
    light: pin.at + SEAT,
    shorten: slipS.at,
    slip: slipS.at + DUR.tick,
    tab,
    tab2,
    listed,
    listed2,
    ctrlNew: sync.at,
    tools,
    flip: flip.at,
    restamp: restamp.at,
    junit: junit.at,
    foot: foot.at,
    linked,
    hand: link.at,
    linkIn: link.at,
    form: link.at + LINK.form,
    draw: link.at + LINK.draw,
    out: out.at,
    dissolved: dissolve.end,
  };
});

/** How far a flying chip has come: its flight's eased progress, and its path's point. */
function flight(
  b: number,
  from: number,
  to: number,
  a: { x: number; y: number },
  z: { x: number; y: number },
  bulge: number,
): { k: number; p: { x: number; y: number } } {
  const k = span(b, from, to - from, "move");
  return { k, p: curveAt(arc(a, z, bulge), k) };
}

/**
 * The slip's chips flying into the drawn link (ART.md §12.7): its version
 * chip into the first install path's version segment (drawn after the
 * link, over its band, fading into it as it lands), and each skill name
 * into its project path's name (drawn before the link, so the path's pill
 * forms over its own). `over` picks which.
 */
function handOff(
  ctx: CanvasRenderingContext2D,
  b: number,
  chips: readonly SlipChip[],
  rows: ReturnType<typeof linkLayout>,
  linked: number,
  form: number,
  over: boolean,
): void {
  const size = TYPE.slip.size;
  const chipH = TYPE.slip.chipH;
  if (over) {
    const v = chips.find((c) => c.version);
    const band = rows[0]?.band;
    if (!v || !band) return;
    const f = flight(
      b,
      linked + LINK.fly,
      form,
      { x: v.x, y: v.base },
      { x: rows[0].segX, y: rows[0].base },
      0.06,
    );
    const a = 1 - span(b, form, DUR.flick, "glide");
    if (b < linked || a <= 0) return;
    // Its pill turns into the segment's band as it goes, riding the arc
    // with its text.
    const r = lerpRect(v.rect, band, f.k);
    const dx = f.p.x - lerp(v.x, rows[0].segX, f.k);
    const dy = f.p.y - lerp(v.base, rows[0].base, f.k);
    ctx.save();
    ctx.globalAlpha *= a;
    roundedRect(
      ctx,
      r.x + dx,
      r.y + dy,
      r.w,
      r.h,
      lerp(RADIUS.seat, RADIUS.seat - 2, f.k),
    );
    ctx.fillStyle = PILLAR_BAND.tools;
    ctx.fill();
    mono(ctx, [run(v.value, PILLAR.tools)], f.p.x, f.p.y, size);
    ctx.restore();
    return;
  }
  chips
    .filter((c) => c.id === "skill")
    .forEach((c, i) => {
      const row = rows.find((r) => r.name.text === c.value);
      if (!row || b < linked) return;
      const f = flight(
        b,
        linked + LINK.fly + i * DUR.stagger,
        form,
        { x: c.x, y: c.base },
        { x: row.name.x, y: row.base },
        -0.08,
      );
      // Its pill, under the path's forming over it, until that is whole.
      const a = 1 - span(b, form + DUR.tick, DUR.flick, "glide");
      if (a <= 0) return;
      const h = lerp(chipH, row.from.h, f.k);
      ctx.save();
      ctx.globalAlpha *= a;
      pill(ctx, [run(c.value, PALETTE.text1)], f.p.x - 12, f.p.y, size, {
        h,
        radius: lerp(RADIUS.seat, RADIUS.chip, f.k),
        pad: 12,
      });
      ctx.restore();
    });
}

export const scene = greyScene(
  "packslip",
  (g) => {
    const { ctx, b } = g;
    const c = capture(g.d, "C8");
    const S = schedule(g.d);
    const { plays } = S;
    const tools = g.d?.versions.tools ?? {};
    const oldV = tools.hk_old ?? "";
    const newV = tools.hk_new ?? "";
    const hk = /^hk\s*=/;
    // The card: the station card switch's folder became (C7's file) on the
    // bar line; hk's row seats (the rows part, the line drops) as C8
    // started it, and its value turns to the new one on the tools line.
    // [tools] lights as hk lands, hk's row lights again as the drawn link
    // lights the new version, and both are out by the bar line.
    const drop = clamp((b - S.seatFrom) / (S.light - S.seatFrom));
    const seatA =
      b < S.light + DUR.seatHold
        ? 1
        : 1 - span(b, S.light + DUR.seatHold, DUR.seatFade, "glide");
    const seat = Math.max(
      seatA,
      win(b, S.flip, S.flip + 2, 0.5),
      0.7 * win(b, S.draw + DUR.half, S.out, 0.5),
    );
    const cardK = focusOf(S.focus, "card", b);
    keep(g, () => {
      // Where switch's bar line keeps no card, it comes back in in place.
      const alpha = (STATION_KEPT ? 1 : span(b, 0, DUR.flick, "glide")) * cardK;
      if (b <= S.seatFrom) {
        card(ctx, STATION.rect, { ...cardOptions(g.d, STATION), alpha });
        return;
      }
      const lit =
        span(b, S.light, DUR.light, "arrive") *
        (1 - span(b, S.out, DUR.exit, "leave"));
      flipCard(ctx, CARD_L.rect, {
        ...cardOptions(g.d, CARD_L),
        alpha,
        before: fileOf(g.d, "C8", "start/work/api/mise.toml"),
        after: fileOf(g.d, CARD_L.from, CARD_L.path),
        lit: { "[tools]": lit },
        ignite: { "[tools]": bumpOver(b, S.light, DUR.ignite) },
        seat: [{ re: hk, a: clamp(seat), drop }],
        line: hk,
        b,
        at: S.flip,
      });
    });
    // The slip, under the terminal: the old version's packslip, re-stamped
    // with the new one's after the card flips; its identity outlined as the
    // first caption says "signed" (its first line landing), then its shells
    // and skills as its second names them, all gone before the first Tab's
    // keycap comes up; it hands its chips to the link once the `linked`
    // rows have printed. Dimmed while something else has the focus.
    const events = S.p.events();
    const cap = captionsFor("packslip", g.d, events)[0];
    const signed = cap?.lines[0]?.in ?? S.slip + 1;
    const named = cap?.lines[1]?.in ?? S.slip + 1.25;
    const keyUp = S.tab - DUR.keycapLead;
    // Each Tab completes in the take's shell: its completion lights.
    const shell = shellOf(g.d);
    const tabbed = Math.max(
      win(b, S.listed, S.listed + 1.5, DUR.tick),
      win(b, S.listed2, S.listed2 + 1.5, DUR.tick),
    );
    const slipO = {
      d: g.d,
      before: oldV,
      after: newV,
      flip: S.restamp,
      b,
      enter: S.slip,
      hand: S.hand,
      alpha: focusOf(S.focus, "slip", b),
      under: TO.pane.y + TO.pane.h,
      hl: {
        identity: win(b, signed - DUR.flick, named + DUR.short, DUR.tick),
        completion: win(b, named + DUR.flick, keyUp, DUR.tick),
        skill: win(b, named + DUR.flick + DUR.stagger * 2, keyUp, DUR.tick),
        ...(shell ? { [`completion:${shell}`]: tabbed } : {}),
      },
    };
    slip(ctx, slipO);
    // The drawn link, in the slip's place: from the take's own `linked`
    // rows, built from the slip's chips.
    if (c && b >= S.hand) {
      const sc = screenAt(c, HOLDS.C8!(c));
      const linkO = {
        links: linkedRows(sc.lines),
        active: newV,
        b,
        enter: S.linkIn,
        draw: S.draw,
        leave: S.out,
        form: S.form,
      };
      const chips = slipChips({ ...slipO, b: S.hand });
      const rows = linkLayout(linkO);
      handOff(ctx, b, chips, rows, S.hand, S.form, false);
      skillsLink(ctx, linkO);
      handOff(ctx, b, chips, rows, S.hand, S.form, true);
    }
    // The terminal: its window, shortening, and dimmed once the `linked`
    // rows print (the rest keeps it dimmed); switch's last screen, held on
    // the bar line, dissolves into C8's prompt.
    const pane = paneAt(b, S.shorten);
    const dim = unfocus(S.focus, "term", b) * (TO.dim ?? 1);
    keep(g, () => drawPaneWindow(ctx, pane, dim));
    restScreen(g, "switch|packslip", 1 - on(b, 0, DUR.tick));
    const s = term(g, "C8", pane, () => plays, {
      alpha: on(b, 0, DUR.tick),
      dim,
      screen: TO.screen,
      keep: true,
    });
    // `--junit-xml`, outlined once the new version's list shows it.
    const ja = win(b, S.junit, S.ctrlNew, DUR.flick);
    const row = s && ja > 0 ? findRow(s.screen, /^--junit-xml\b/) : null;
    if (s && row) {
      const pop = span(b, S.junit, DUR.tick, "arrive");
      const cell = s.layout.cell(row.line, 0);
      const w = "--junit-xml".length * s.layout.advance;
      const cx = cell.x + w / 2;
      const cy = cell.y + cell.h / 2;
      const sc = 0.9 + 0.1 * pop;
      ctx.save();
      ctx.globalAlpha *= ja * pop;
      ctx.translate(cx, cy);
      ctx.scale(sc, sc);
      ctx.translate(-cx, -cy);
      roundedRect(
        ctx,
        cell.x - STAMP.pad,
        cell.y + 3,
        w + 2 * STAMP.pad,
        cell.h - 6,
        STAMP.radius,
      );
      ctx.strokeStyle = COLOR.mark;
      ctx.lineWidth = STROKE.lit;
      ctx.stroke();
      ctx.restore();
    }
    // The Tab keycaps, on the keys' own beats: at the pane's right end just
    // under the chrome, beside rows that end short of it (the corner ART.md
    // §6 names holds the new list's last rows here, which run full width).
    const corner = keycapOn(PANES.slip, "tab");
    const tab = { x: corner.x, y: PANES.slip.y + CHROME_H + TAB_TOP };
    keycap(ctx, tab.x, tab.y, "tab", b, S.tab);
    keycap(ctx, tab.x, tab.y, "tab", b, S.tab2);
    footnote(
      ctx,
      "In a shell with `mise activate`.",
      win(b, S.foot, S.ctrlNew, 0.25),
    );
  },
  {
    // The lit screen: the terminal's window as it shortens.
    lit: (b, d) => termLit(rectOf(paneAt(b, schedule(d).shorten))),
    events: (d) => schedule(d).p.events(),
  },
);

/** The score's cues (score/cues.ts), where the picture has them. */
export const cues: SceneCues<"packslip"> = (facts) => {
  const S = schedule(reelData(facts));
  return {
    light: S.light,
    slip: S.slip,
    tab: S.listed,
    flip: S.flip,
    junit: S.junit,
    linked: S.linked,
    link: S.draw,
  };
};

/** The section's schedule (kit/pace.ts Pace) and its takes' plays, for the pacing checks. */
export const pacing = (
  facts: ReelFacts | null,
): { pace: Pace; takes: Partial<Record<CaptureId, readonly Play[]>> } => {
  const S = schedule(reelData(facts));
  return { pace: S.p, takes: { C8: S.plays } };
};
