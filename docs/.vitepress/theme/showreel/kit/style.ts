// The reel's production look as tokens: colours beyond the palette, the
// stage's light, shadows and glows, radii and stroke widths, the type
// scale, where things stand, the chef's geometry, and the motion
// vocabulary (named easings and standard durations in beats). ART.md is
// the spec these numbers come from; the kit's builders import them, so a
// look changes here and nowhere else.
//
// Nothing here draws. It imports only the palette and the clock (bible.ts)
// and the pure curves (math.ts), so any module can import it, the tests
// included.

import { BEAT, PALETTE, type Pillar } from "../bible";
import {
  clamp,
  cubicBezier,
  type Ease,
  inCubic,
  inOutSine,
  linear,
  outBack,
  outCubic,
  outQuart,
  swiftIn,
  swiftInOut,
  swiftOut,
} from "../math";
import type { Rect } from "./motion";
import type { Pane } from "./term";

// Colour.

/**
 * Neutral roles the palette lacks. None of them is a pillar colour, and
 * none may stand in for one: pink, gold, sage and terracotta are PALETTE's
 * and mean only their pillars (ART.md §2).
 */
export const COLOR = {
  /** Print on a ticket: warm plum-black, 14.4 : 1 on paper, 8 : 1 or more on every pillar colour. */
  ink: "#231c22",
  /** A ticket's paper: a little lighter at the top edge, where the lamp falls. */
  ticketTop: "#f6f1e7",
  ticketBottom: "#ebe3d5",
  /** Specular light: the chef's glint, sparkles, a stamp's one-frame flash. */
  glint: "#fffaf2",
  /** Every cast shadow (alpha set per SHADOW preset). */
  shadow: "#070407",
  /** The ticket rail: brushed steel in the stage's mauve greys. */
  steelHi: "#7d7079",
  steel: "#5c5458",
  steelLo: "#3b323a",
  /** A raised surface's fill, lit from above: top and bottom of its gradient (PALETTE.surface between). */
  surfaceTop: "#252125",
  surfaceBottom: "#1e1b1e",
  /** A 1 px highlight inside a raised object's top edge: paper at 6 %. */
  topEdge: "rgba(244,238,227,0.06)",
  /** Keycaps: a physical key's face, lip and edge. */
  keyTop: "#3c343a",
  keyBottom: "#2e272d",
  keyLip: "#1a1519",
  keyEdge: "#5a4c58",
  /**
   * The shell's prompt, neutral (the hero terminal's label colour): the
   * prompt must not wear tools pink (ART.md §10, change for bible.ts TERM).
   */
  prompt: "#c5b2c3",
  /** Code in captions and footnotes: mono in dim paper (type.ts CODE). */
  code: PALETTE.paperDim,
  /** Threads, seat outlines, badges, ticks, stamps, pointers: paper. */
  mark: PALETTE.paper,
  /** The rollback arc on the checkpoint rail: paper at 70 %, so the dashed line holds at phone width. */
  arc: "rgba(244,238,227,0.7)",
  /** A ledger's ruled lines and margin: paper at 5 % and 9 %. */
  ledgerRule: "rgba(244,238,227,0.05)",
  ledgerMargin: "rgba(244,238,227,0.09)",
} as const;

/** A lit header's band: the pillar at 14 % over PALETTE.surface, as an opaque colour. */
export const PILLAR_BAND: Record<Pillar, string> = {
  tools: "#3e2f37",
  env: "#3c332f",
  tasks: "#353632",
  machine: "#3d3030",
};

/** A pillar's highlight: 35 % toward paper, the hot centre of its glow. */
export const PILLAR_BRIGHT: Record<Pillar, string> = {
  tools: "#efbbca",
  env: "#e8cea7",
  tasks: "#c6d9b5",
  machine: "#eebfac",
};

/**
 * File syntax. Values are what a viewer reads, so they are the brightest
 * ink on a card; keys step down to text2 and punctuation to text3. A
 * header is text2 folded, text1 open, and its pillar's colour lit.
 * Annotations (fold counts, "more") are Space Grotesk on a pill, never
 * mono, so they cannot be read as the file's own text.
 */
export const SYNTAX = {
  header: PALETTE.text2,
  headerOpen: PALETTE.text1,
  key: PALETTE.text2,
  punct: PALETTE.text3,
  value: PALETTE.paper,
  comment: PALETTE.text3,
  annotation: PALETTE.text3,
  annotationPill: PALETTE.elevated,
} as const;

// The stage.

/** The room: the stage's light, vignette, grain and the motes near the chef. */
export const STAGE_FX = {
  /**
   * The pass lamp: one warm light from above the stage, an ellipse of paper
   * at 5 % in the middle falling to nothing at its rim, painted straight
   * after the background and under everything. It is still: only breath
   * and the end card change it.
   */
  lamp: {
    x: 960,
    y: 250,
    rx: 1250,
    ry: 820,
    color: PALETTE.paper,
    alpha: 0.05,
  },
  /** The end card's lamp: moved over the chef, a little brighter. */
  lampEnd: {
    x: 1420,
    y: 420,
    rx: 980,
    ry: 780,
    color: PALETTE.paper,
    alpha: 0.07,
  },
  /** compose.ts: darker toward the corners, lit screens spared (fx.vignette). */
  vignette: { strength: 0.42, inner: 0.55 },
  /**
   * compose.ts: overlay grain (fx.grain), still: grain re-seeded every
   * frame is rounded away by the encoder, and the dark gradients band
   * without it.
   */
  grain: { amount: 0.07, fps: 0 },
  /** The lights going down (breath): night over the frame. */
  dim: { color: PALETTE.night, alpha: 0.7 },
  /**
   * Motes: a few specks of warm light rising round the chef, only in open
   * (after the chef resolves), morph and end, never over a pane or text.
   * Each is a pure function of global time and its index (math.hash).
   */
  motes: {
    count: 28,
    rMin: 1,
    rMax: 2.6,
    alphaMin: 0.04,
    alphaMax: 0.12,
    /** px per second, upward. */
    rise: 14,
    /** Sideways sway, px, and its rate, Hz. */
    sway: 10,
    swayHz: 0.15,
    color: PALETTE.paper,
    /** Where they live: round the chef's rect, grown by this much. */
    spread: 180,
  },
} as const;

/** A cast shadow, in canvas terms (shadowOffsetY, shadowBlur), from the lamp above. */
export interface ShadowSpec {
  color: string;
  alpha: number;
  dx: number;
  dy: number;
  blur: number;
}

const shade = (alpha: number, dy: number, blur: number): ShadowSpec => ({
  color: COLOR.shadow,
  alpha,
  dx: 0,
  dy,
  blur,
});

/**
 * Shadows. Every raised object (window, card, file card, ticket, slip,
 * tile, keycap, chip in flight) casts `raised` or `small` plus `contact`;
 * diagrams drawn on the stage (lanes' tracks, rivers, the rail, threads)
 * cast none. Shadow alpha multiplies by the object's own alpha.
 */
export const SHADOW = {
  /** Windows and cards at rest. */
  raised: shade(0.55, 18, 36),
  /** Anything lifted and in flight: file cards flying, the ticket, the lobes, a zoomed line. */
  float: shade(0.6, 30, 52),
  /** Chips, badges in flight, keycaps at rest, tiles. */
  small: shade(0.5, 8, 16),
  /** A keycap pressed down. */
  pressed: shade(0.4, 3, 6),
  /** A tight dark line under every raised object, so it sits on the stage. */
  contact: shade(0.35, 2, 4),
} as const;

/** Glows, for fx.glow (additive radial sprite): radius px and peak alpha. */
export const GLOW = {
  /** A header lighting: a pulse at its left bar (bump over DUR.ignite). */
  ignite: { radius: 110, alpha: 0.35 },
  /** A registry name lit in the rivers: steady. */
  name: { radius: 120, alpha: 0.32 },
  /** The [daemons] pilot light: steady once lit. */
  pilot: { radius: 40, alpha: 0.45 },
  /** A spark's head riding a thread (motion.drawSpark size 5). */
  spark: { size: 5, trail: 0.2 },
  /** The chef's backlight: blooms on its resolve and on the end card's "mise"... */
  chefBloom: { radius: 470, alpha: 0.14 },
  /** ...and settles to this halo. */
  chefHalo: { radius: 430, alpha: 0.06 },
  /** Behind the climax's big line. */
  climax: { radius: 560, alpha: 0.08 },
  /**
   * A stamp landing: one ring (fx.ring), px and width; kit/parts.ts stamp's
   * starts at the stamp's own edge and spreads `spread` px past it, so it
   * never crosses the words it rings or reaches far into its neighbours.
   */
  ring: { radius: 128, width: 4, spread: 44 },
  /** The slam's impact behind kinetic type. */
  slam: { radius: 300, alpha: 0.12 },
} as const;

// Shapes.

/** Corner radii, px. */
export const RADIUS = {
  window: 14,
  card: 18,
  file: 14,
  tab: 10,
  panel: 16,
  lane: 12,
  tile: 14,
  chip: 10,
  keycap: 16,
  ticket: 6,
  seat: 8,
  zoom: 16,
  /** Badges and annotation pills are fully round: height / 2. */
  pill: Infinity,
} as const;

/** Stroke widths, px. */
export const STROKE = {
  /** Window and card edges, tab-strip and chrome rules. */
  hairline: 1,
  /** A lit card edge (the active folder), a selected tile. */
  lit: 2,
  badge: 2,
  badgeMicro: 1.5,
  seat: 1.5,
  thread: 2,
  /** A thread's end anchors: dots of this radius. */
  anchor: 4,
  /** Tree lines in the folder diagram, the depends joins. */
  tree: 2,
  /** The checkpoint rail's track. */
  track: 4,
  /** The rollback arc: 3, not 2.5, so it holds at phone width. */
  arc: 3,
  tick: 5,
  ring: 3,
  stamp: 4,
  underline: 4,
  /** The ⌃L caret, and the tree's arrowheads. */
  caret: 5,
  /** A lit header's bar down its left edge (a width, not a stroke). */
  headerBar: 6,
} as const;

/** Dash patterns, px: [on, off]. */
export const DASH = {
  /** A lane waiting on its dependencies; a slot for kinetic type. */
  waiting: [10, 8],
  /** The rollback arc. */
  arc: [8, 8],
  /** The ticket's perforation. */
  perf: [6, 6],
} as const;

// Type.

/**
 * The type scale, logical px at 1920 × 1080. The reading floor is 40 px
 * for type (Space Grotesk) and 26 px for mono a viewer must read; 20–24 px
 * is texture that never has to be read (checksums, dimmed prose, micro
 * badges). Weights are the variable fonts' axes.
 */
export const TYPE = {
  caption: { size: 88, weight: 600, code: 80, codeWeight: 600 },
  labelL: { size: 64, weight: 600 },
  label: { size: 56, weight: 600 },
  detail: { size: 40, weight: 500 },
  footnote: { size: 40, weight: 500, code: 37 },
  badge: { size: 30, weight: 500 },
  badgeMicro: { size: 22, weight: 500 },
  /** Annotation pills on cards and ledgers ("4 lines", "3 more tables"). */
  annotation: { size: 22, weight: 500 },
  /** Uppercase meta ("ACT I", "CHECKPOINTS"): tracked out. */
  meta: { size: 22, weight: 600, tracking: 0.16 },
  /** Terminal text by pane preset (PANES). */
  term: {
    solo: { size: 32, lineH: 44 },
    /** Between: a side-width pane with few, short rows (PANES.envLarge). */
    mid: { size: 30, lineH: 42 },
    side: { size: 26, lineH: 38 },
    aside: { size: 20, lineH: 29 },
  },
  /** A window's chrome title (its cwd): mono. */
  chrome: { size: 22, weight: 500 },
  /** The station card. */
  card: { size: 26, lineH: 38, header: 700, row: 400, title: 24 },
  /** File cards (.env, ~/.zshrc, mise-tasks/deploy, ci.yml). */
  file: { size: 26, lineH: 38, title: 22 },
  /** The ledger: its rows, and its per-platform checksum texture. */
  ledger: { size: 24, lineH: 34, sum: 20 },
  /**
   * Chips: a captured value, a backend prefix, a file, a version on a
   * folder, and a value lifted out of a terminal onto the card (`lift`: at
   * 40, the one size a lifted value reads at on a phone).
   */
  chip: { value: 32, backend: 36, file: 26, version: 40, lift: 40 },
  /** Kinetic big type: mono 700, tracked in −2 %. */
  big: { slam: 120, kinetic: 120, climax: 88, weight: 700, tracking: -0.02 },
  /** Tickets: Cormorant Garamond italic title, the pillar band's chip in mono. */
  ticket: {
    title: 112,
    titleNarrow: 100,
    titleWeight: 600,
    band: 40,
    bandWeight: 600,
  },
  /** The name card (open and end). */
  name: { size: 160, weight: 500 },
  /** 50, not 52: at 52 "CLI" ended 40 px short of the hat's left lobe on the end card. */
  tagline: { size: 50, weight: 500 },
  /** 52: at 44 the install line read 9 px on a phone (hk's is about 56); its strip is 938 px, inside the left column. */
  install: { size: 52, weight: 500 },
  platform: { size: 40, weight: 500 },
  url: { size: 64, weight: 600 },
  /** Lanes: the task's prefix, its lines, and a hero line (skip). */
  lane: { label: 30, text: 24, hero: 40 },
  /** Rivers of registry names, back to front (RIVERS_ART's streams read these). */
  rivers: [24, 32, 44],
  /** 42: at 36 the tiles' labels were under the 40 px reading floor. */
  tile: { size: 42, weight: 600 },
  /** The checkpoint rail's ids and triggers: the rail's story, so above the floor for mono (36, 30). */
  rail: { id: 36, trigger: 30 },
  /**
   * The slip (packslip): mono at the reading floor on chips `chipH` tall,
   * rows `lineH` apart, so four rows fit its 220 px with the tab strip.
   */
  slip: { size: 26, lineH: 40, chipH: 34 },
  keycap: { size: 40, weight: 600 },
  stamp: { size: 34, weight: 700 },
} as const;

// Where things stand.

const rect = (x: number, y: number, w: number, h: number): Rect => ({
  x,
  y,
  w,
  h,
});

/**
 * The layout grid: a left column for the terminal (x 160–1100), a 40 px
 * gutter where threads run, and a right column for the station card and
 * whatever shares its place (x 1140–1760). Actors stand between y 120 and
 * 690; footnotes sit on 738; the captions' band starts at 740 (their
 * first cap height is near 770).
 */
export const LAYOUT = {
  left: rect(160, 120, 940, 570),
  gutter: rect(1100, 120, 40, 570),
  right: rect(1140, 120, 620, 570),
  full: rect(160, 120, 1600, 570),
  /** The station card. */
  card: rect(1140, 120, 620, 570),
  /** The card in the Environments act, over the `.env` file card. */
  envCard: rect(1140, 120, 620, 330),
  envFile: rect(1140, 480, 620, 210),
  /** The shell-env strip over the Environments act's terminal. */
  envStrip: rect(160, 120, 940, 112),
  /** The climax's card, folded to its three headers. */
  compact: rect(1140, 120, 620, 250),
  /** `~/.zshrc` and the checkpoint rail (Dotfiles). */
  zshrc: rect(1140, 120, 620, 250),
  rail: rect(1140, 400, 620, 290),
  /** Bootstrap: `~/.zshrc` flown in, and the tiles under it. */
  bootFile: rect(1140, 120, 620, 150),
  tiles: rect(1140, 300, 620, 390),
  /** Lock: the request (mise.toml's node line) over the ledger. */
  request: rect(1140, 120, 620, 110),
  ledger: rect(1140, 250, 620, 440),
  /** The CI panel, centred. */
  ci: rect(400, 120, 1120, 570),
  /** Packslip: the terminal shortened, the slip tucked under it. */
  slipPane: rect(160, 120, 940, 330),
  slip: rect(160, 470, 940, 220),
  /**
   * A line too wide for the card, zoomed to the stage's width: at the top
   * of the actor band, 28 px clear of the env pane (y 252) under it.
   */
  zoom: rect(160, 120, 1600, 104),
  /**
   * The footnote's baseline, x 160: its cap height about 20 px under the
   * actor band (690) and its descenders about 20 px over a two-line
   * caption's first cap height (~770), so it reads as neither the pane's
   * edge nor a third caption line.
   */
  footnoteY: 738,
  /** Kinetic type's centre over the left column. */
  bigX: 630,
  bigY: 420,
  /**
   * A slam's centre over a short pane (use: PANES.slip at y 120–450, the
   * slam under it): the art decision for ART.md §12.10's `mise use jq`.
   */
  slamY: 470,
} as const;

/** A pane on the layout: chrome 56 px, text inset 28 px, the first baseline 16 px under the chrome plus 0.8 em. */
const pane = (r: Rect, size: number, lineH: number, fade: number): Pane => {
  const baseline0 = Math.round(r.y + CHROME_H + 16 + 0.8 * size);
  const top = baseline0 - 0.8 * size - (lineH - size) / 2;
  return {
    ...r,
    chrome: true,
    window: true,
    size,
    lineH,
    x0: r.x + 28,
    baseline0,
    rows: Math.floor((r.y + r.h - 14 - top) / lineH + 1e-9),
    fade,
    anchor: "bottom",
  };
};

/** The window's chrome bar, px: tall enough for a 30 px badge inside it. */
export const CHROME_H = 56;

/** Terminal chrome: dots, title, rules and the badge slot. */
export const CHROME = {
  h: CHROME_H,
  /** Three neutral dots (TERM.dots): radius, first centre's x inset, spacing. */
  dot: { r: 7, x: 30, gap: 24 },
  /** The cwd, from the latest prompt on screen, centred in the bar: mono 22 text3. */
  title: { size: 22, color: PALETTE.text3 },
  /**
   * The cwd dot, 14 px left of the title: on a `cd` it hops (an arc 14 px
   * high over DUR.tick) as the title cross-fades to the new path. When
   * badges would reach the centred title, the title sets left instead,
   * 110 px in from the window's left edge.
   */
  cwdDot: { r: 5, gap: 14, hop: 14, color: PALETTE.paper },
  titleLeft: 110,
  /** Badges ride the bar's right end: this far in from the edge, 4 px down. */
  badgeInset: 12,
  badgeTop: 4,
} as const;

/** Terminal panes on the layout (kit/term.ts Pane). */
export const PANES = {
  /** Alone on the stage: 80 columns of 32 px, 11 rows. */
  solo: pane(LAYOUT.full, TYPE.term.solo.size, TYPE.term.solo.lineH, 0),
  /** Beside the card: 56 columns of 26 px, 12 rows, the rest faded off its right edge. */
  side: pane(LAYOUT.left, TYPE.term.side.size, TYPE.term.side.lineH, 56),
  /** Under the shell-env strip. */
  env: pane(
    rect(160, 252, 940, 438),
    TYPE.term.side.size,
    TYPE.term.side.lineH,
    56,
  ),
  /** Shortened over the slip (packslip). */
  slip: pane(LAYOUT.slipPane, TYPE.term.side.size, TYPE.term.side.lineH, 56),
  /**
   * Beside the card, nine rows (y 120–543): a take that shows a few rows
   * (args, the start of daemons), growing to `side` with lerpPane on
   * EASE.move when its output needs the room.
   */
  sideShort: pane(
    rect(160, 120, 940, 423),
    TYPE.term.side.size,
    TYPE.term.side.lineH,
    56,
  ),
  /**
   * Beside the card at the solo size, 32/44: 11 rows, 44 columns before
   * the fade. For a take whose rows are short (switch's longest is 33
   * columns), so a payoff reads at phone width.
   */
  sideLarge: pane(LAYOUT.left, TYPE.term.solo.size, TYPE.term.solo.lineH, 56),
  /** Under the shell-env strip at 30/42: 8 rows, 47 columns before the fade (redact's longest row). */
  envLarge: pane(
    rect(160, 252, 940, 438),
    TYPE.term.mid.size,
    TYPE.term.mid.lineH,
    56,
  ),
  /** Five rows of 32 px across the stage, over the chips (backends' gh pane). */
  wideShort: pane(
    rect(160, 120, 1600, 330),
    TYPE.term.solo.size,
    TYPE.term.solo.lineH,
    0,
  ),
  /** Dimmed and shrunk while prose prints: texture only, never read. */
  aside: pane(
    rect(160, 150, 700, 480),
    TYPE.term.aside.size,
    TYPE.term.aside.lineH,
    40,
  ),
} as const;

/** A dimmed pane (prose, tables, long paths printing): its text alpha and its body's mix toward the stage. */
export const PANE_DIM = { text: 0.42, body: 0.2, title: 0.6 } as const;

/**
 * Keycaps: sizes, and where one sits on a pane (its centre, from the
 * pane's bottom-right corner); a second one sits `pair` px left of it.
 */
export const KEYCAP = {
  ctrlL: { w: 108, h: 88 },
  tab: { w: 136, h: 88 },
  lip: 8,
  press: 6,
  dx: -104,
  dy: -70,
  pair: 170,
} as const;

/** Badges: height is 1.6 × size, padding 0.55 em each side; fill night at 85 %. */
export const BADGE = { heightEm: 1.6, padEm: 0.55, fillAlpha: 0.85 } as const;

/** The ticket rail and tickets. */
export const TICKET = {
  /** The rail: across the stage, or over the right column beside a terminal (new). */
  rail: rect(160, 64, 1600, 16),
  railNarrow: rect(1120, 64, 660, 16),
  /** A ticket hanging on the rail, centred, and beside the terminal (new). */
  ticket: rect(640, 96, 640, 400),
  narrow: rect(1150, 96, 600, 400),
  /** Its clip on the rail. */
  clip: { w: 40, h: 16 },
  /** Rows, from the ticket's top: meta baseline, the pillar band, the title's baseline, the perforation. */
  metaY: 44,
  band: { y: 68, h: 60, inset: 24 },
  titleY: 270,
  perfY: 318,
  /** The torn edge: teeth this wide and deep. */
  teeth: { w: 12, h: 7 },
  /** How far the ticket feeds, and in how many printer steps. */
  feedSteps: 12,
  /** It swings on its clip after the tear: degrees, decay (s). */
  swing: { deg: 2.2, tau: 0.35 },
  /** It lifts off the rail: px up, and its turn, degrees. */
  lift: { dy: -460, deg: 3 },
  /**
   * Its two beats, in section beats: the feed ends (the tear) at `tear`;
   * it lifts off from `liftAt` to `liftEnd`. The stage it inherits leaves
   * from `clear` and is gone by `clearEnd` (before the tear); the next
   * section's stage comes in under it from `bringAt` over `bringDur`
   * (kit/grey.ts ticketStage), at rest by 1.85, before the bar line.
   */
  cue: {
    tear: 0.75,
    liftAt: 1.2,
    liftEnd: 1.8,
    clear: 0,
    clearEnd: 0.5,
    bringAt: 1.3,
    bringDur: 0.55,
  },
} as const;

/** The name card, open and end: left-aligned at x 160, the chef on the right. */
export const NAME_CARD = {
  x: 160,
  nameY: 360,
  taglineY: 452,
  /** The install line's box: its top, height and padding. */
  install: { y: 506, h: 88, pad: 32 },
  platformY: 668,
  urlY: 790,
} as const;

// The chef.

/**
 * The chef, in logo-dark.svg's own units (viewBox 43 37 215 226), drawn in
 * paper. The toque's lobes and the hat's cut were measured off the SVG in
 * Chromium by the brand pass (test/chef.test.ts holds them to the file):
 * the art pass's first reading off a 10-unit grid put the hat's lower edge
 * through the top of the right ear.
 */
export const CHEF_ART = {
  viewBox: { x: 43, y: 37, w: 215, h: 226 },
  fill: PALETTE.paper,
  /** Where it stands on open and end (its SVG box, aspect 215 : 226). */
  place: rect(1180, 200, 500, 526),
  /**
   * The toque's three lobes, left to right: each the least-squares circle
   * through the middle of its dark ring (7 units wide), sampled every 2°
   * where the ring shows; each fits its ring to 0.1 units rms.
   */
  lobes: [
    { x: 88.5, y: 130.9, r: 34 },
    { x: 129.7, y: 81.2, r: 32.9 },
    { x: 191, y: 84.8, r: 36.6 },
  ],
  /**
   * Everything above the brim, for the morph's toque and the glint's clip:
   * the viewBox's top, then along the middle of the brim's dark line (a
   * gentle arc, not a straight edge) and out through the notches between
   * the hat and the ears, so the cut crosses paper only where the
   * silhouette's outer border meets those notches. The vertices from
   * `hatCutFrom` on run along the cut; the rest are the viewBox's edges.
   *
   * At each notch the outer border comes down off a lobe and bends into
   * the ear's border. The cut crosses it at that bend (from the notch's
   * inner dark corner to its outer one), so the whole strip coming down
   * off the lobe is hat and the bald head in the morph ends each ear on a
   * clean diagonal. (Cut higher, the strip stayed on the body as a paper
   * spike over the right ear and a flat tab on the left one.)
   */
  hat: [
    [43, 37],
    [258, 37],
    [258, 121],
    [226.5, 130.2],
    [219.5, 139.8],
    [215, 132.5],
    [200, 138.6],
    [180, 147.75],
    [160, 159.4],
    [140, 174],
    [130, 183.1],
    [122, 191],
    [122, 195.3],
    [111.3, 194.6],
    [43, 194.6],
  ] as readonly (readonly [number, number])[],
  hatCutFrom: 3,
  /** The band's angle, degrees (it rises to the right). */
  tilt: -30,
  /** Which pillar lands in which lobe: the card's reading order. */
  lobeOf: ["tools", "env", "tasks"] as const,
  /**
   * The glint: a specular stripe perpendicular to the band, swept along it
   * across the hat, clipped to the chef's fill. Width and soft falloff in
   * SVG units.
   */
  glint: { width: 7, soft: 12, alpha: 0.9, color: COLOR.glint },
  /** The sparkle the glint ends in (hk's four-point star), px. */
  sparkle: { r: 22 },
  /** The block chef in C1's help: its columns and rows on that screen. */
  block: { col: 66, cols: 14, row: 1, rows: 7 },
  /**
   * The mosaic the block chef's cells land in: each quadrant of a block
   * glyph is one cell, this size at the chef's place, the grid turned to
   * `tilt` about the place's centre (nudged down `dy`) so the pixel hat
   * lands on the vector hat.
   */
  mosaic: { cellW: 17, cellH: 34, dy: 10 },
  /** The mask reveal: each mosaic cell's square grows from this half-size to that, in cells. */
  reveal: { from: 0.5, to: 2.2 },
  /**
   * The morph's beats over which the chef glides from where it forms to
   * `place` (scenes/g1-brand/morph-card.ts morphPlace, on glide), and the
   * pass lamp with it to STAGE_FX.lampEnd (fx.ts lampAt), at rest a beat
   * and a half before the end card (PACE rule 6).
   */
  glide: [5, 13 / 2],
} as const;

// Diagrams.

/**
 * Lanes (depends, skip, clone): exploded terminal rows, one per task, over
 * the left column. A lane is a strip of terminal (TERM.window, the pane's
 * edge) holding that task's lines verbatim; its captured prefix (`[lint]`)
 * stands in the label column in its own ANSI colour. The dependencies'
 * lanes start together at trackX0; ci's starts `ciIndent` later, after a
 * depends bracket in the gutter between labels and tracks. Nothing is drawn
 * over a lane's text.
 */
export const LANES_ART = {
  top: 150,
  labelX: 160,
  /** The depends bracket's x, between the label column and the tracks. */
  bracketX: 300,
  trackX0: 330,
  trackX1: 1100,
  ciIndent: 120,
  laneH: 88,
  gap: 18,
  /** A lane's lines: first baseline from its top, then this pitch. */
  textY: 38,
  textPitch: 30,
  heroH: 150,
  /** The ci lane waits in a dashed outline until its dependencies finish. */
  waitingStroke: PALETTE.text3,
  /** A lane receiving a line brightens: its fill mixes toward paper this much. */
  flash: 0.08,
  /** The bracket: ticks into each dependency's lane, an elbow and arrowhead into ci's. */
  bracket: { color: PALETTE.paper, alpha: 0.6, tick: 14, arrow: 10 },
  /** The badge "Order from one real run. Not to scale.", right-aligned under the lanes. */
  badgeY: 610,
} as const;

/**
 * Rivers of registry names (registry): three streams of the registry's real
 * names flowing left along gentle curves over the left column, back to
 * front, each name turned to its stream's tangent. A stream is a quadratic
 * curve (a, c, b) with `lanes` parallel lines 1.3 em apart; names are
 * spaced 1.2 em along it and move `speed` px/s. `node` rides the middle
 * stream and `terraform` the front one, each once.
 */
export const RIVERS_ART = {
  region: LAYOUT.left,
  rivers: [
    {
      a: { x: 100, y: 330 },
      c: { x: 620, y: 200 },
      b: { x: 1160, y: 300 },
      lanes: 3,
      size: TYPE.rivers[0],
      color: PALETTE.text3,
      alpha: 0.45,
      speed: 30,
    },
    {
      a: { x: 100, y: 470 },
      c: { x: 620, y: 360 },
      b: { x: 1160, y: 450 },
      lanes: 2,
      size: TYPE.rivers[1],
      color: PALETTE.text2,
      alpha: 0.8,
      speed: 46,
    },
    {
      a: { x: 100, y: 640 },
      c: { x: 640, y: 520 },
      b: { x: 1160, y: 610 },
      lanes: 1,
      size: TYPE.rivers[2],
      color: PALETTE.text1,
      alpha: 1,
      speed: 64,
    },
  ],
  laneEm: 1.3,
  gapEm: 1.2,
  /** Soft edges at both ends of the region, px. */
  edge: 140,
  /** Which stream carries each lit name. */
  litIn: { node: 1, terraform: 2 } as Record<string, number>,
  /** A lit name: pink, weight 700, scaled up, with a glow and an underline drawn on. */
  lit: { color: PALETTE.tools, weight: 700, scale: 1.12 },
} as const;

/** The folder diagram (switch). */
export const FOLDERS_ART = {
  /** `~/work/` over the tree: mono 28 text2. */
  root: { x: 1140, y: 156, size: 28 },
  /** The trunk down the column's left, and a branch into each folder's tab. */
  trunkX: 1158,
  trunkTop: 172,
  folders: [rect(1190, 196, 570, 214), rect(1190, 454, 570, 214)],
  /** The folder's tab: a raised tab on the card's top-left, with the folder's name. */
  tab: { w: 190, h: 34, size: 26 },
  /** The node line inside: mono 30. */
  row: { size: 30 },
  /** The version a project printed, as a value chip: mono 40, 24 px in from the card's bottom-left. */
  chip: { size: 40, inset: 24 },
  /** The cwd marker on the tree: a paper dot. */
  cwd: { r: 9 },
} as const;

/** The checkpoint rail (track). */
export const RAIL_ART = {
  /** The track's y within the rail's rect, and its ends' insets. */
  y: 150,
  inset: 60,
  node: { r: 14, ring: 4 },
  /** The id under a node, and the trigger under that (baselines, for TYPE.rail's 36 and 30). */
  idY: 56,
  triggerY: 96,
  /** The pointer over the current checkpoint: a paper triangle. */
  pointer: { w: 24, h: 20, gap: 14 },
  /** The rollback arc's rise over the track. */
  arcRise: 80,
  /** "CHECKPOINTS" meta label: 28 px in from the rect's top-left. */
  labelInset: 28,
} as const;

/** Bootstrap's tiles: one per step, stacked. */
export const TILES_ART = {
  h: 84,
  gap: 16,
  labelX: 32,
  /** The machine pillar's bar down each tile's left edge. */
  bar: 6,
  /** The state mark's centre, from the tile's right edge, and its radius. */
  markX: 52,
  markR: 16,
} as const;

/** The ledger (mise.lock). */
export const LEDGER_ART = {
  /** The margin rule's x inset, and the rows' x inset. */
  margin: 48,
  rowX: 56,
  /** Platform names are padded to this many columns before their checksum. */
  platformCols: 17,
  /** A folded entry's annotation pill sits this far after its header. */
  annotationGap: 16,
  /** At most this many platform checksum rows, then "N more platforms" on a pill. */
  sums: 4,
} as const;

/** The slip (packslip): key and value columns. */
export const SLIP_ART = {
  keyX: 28,
  valueX: 220,
  /** The resources rows' values start past their kind (`completion`, `skill`), this far in from valueX. */
  kindW: 172,
  /** Value chips (shells, skills): mono 26 on elevated pills, this far apart. */
  chipGap: 12,
} as const;

/** The station card's anatomy (ART.md §7). */
export const CARD_ART = {
  /** The tab strip's height; the file tab's padding either side of its label. */
  tabH: 52,
  tabPad: 24,
  /** Column 0's inset; extra space before every header but the first. */
  inset: 28,
  headerGap: 14,
  /** Header bands and seat marks sit this far inside the card's sides. */
  bandInset: 12,
  seatInset: 14,
  /** The first baseline: under the tab strip by this much plus 0.8 em. */
  top: 18,
  /** Where a thread into the card ends: this far inside its left edge. */
  anchorX: 8,
  /** A folded table's annotation pill: gap after the header, height, padding. */
  annotation: { gap: 16, h: 30, pad: 12 },
  /** A title change slides the card out and the new one in, px. */
  swap: 60,
} as const;

/** File cards (ART.md §8). */
export const FILE_ART = {
  tabH: 46,
  tabPad: 20,
  inset: 24,
  top: 14,
} as const;

/** Threads (ART.md §9): out of the source, along the gutter, into the target. */
export const THREAD = {
  color: PALETTE.paper,
  alpha: 0.7,
  /** The gutter's x, between the left column and the right. */
  gutterX: 1120,
  /** Past the source run's right edge before it turns. */
  exit: 12,
  corner: 16,
} as const;

/** Chips (ART.md §9): height as a multiple of the text size, and padding. */
export const CHIP_ART = {
  heightEm: 1.5,
  /** Backend chips are pills this tall. */
  backendH: 56,
  /** The document glyph before a file chip's name. */
  docGlyph: { w: 18, h: 22, fold: 6 },
  /** A value copy rises this far before its chip forms round it. */
  liftRise: 6,
} as const;

/** Stamps (ART.md §9). */
export const STAMP = {
  rotate: -2,
  pad: 8,
  radius: 6,
  /** The glint-coloured fill on landing, decaying over DUR.flick. */
  flash: 0.25,
} as const;

/** Underlines (the climax, the rivers' lit names): offset under the baseline, px. */
export const UNDERLINE = { dy: 16 } as const;

/** Kinetic type's slot (`npm:`): outline radius and dash. */
export const SLOT = { radius: 12 } as const;

/** The climax's big line: centre x and the two baselines. */
export const CLIMAX = { x: 960, y: [470, 590] } as const;

/** The shell-env strip (vars). */
export const ENV_STRIP_ART = {
  bar: 6,
  labelX: 28,
  chipX: 200,
  chipGap: 16,
  chipSize: 30,
  /** Chips fold off by flipping up this far. */
  flip: 12,
} as const;

/** A card line zoomed to the stage's width (redact). */
export const ZOOM_ART = { size: 44, inset: 48, cardDim: 0.5 } as const;

/** The [daemons] pilot light: a dot at the header row's right end. */
export const PILOT = { r: 7, inset: 36, ring: 2 } as const;

/** The drawn link (packslip skills): row pitch, and the arrow's bulge and head. */
export const LINK_ART = { pitch: 64, lift: 0.12, head: 10 } as const;

// Motion.

/** Seconds of `n` beats. */
export const beats = (n: number): number => n * BEAT;

/**
 * An ease in `n` equal steps, each step itself eased by `each`: the
 * ticket's printer feed.
 */
export const stepped =
  (n: number, each: Ease = outCubic): Ease =>
  (u) => {
    const x = clamp(u) * n;
    const k = Math.min(n - 1, Math.floor(x));
    return (k + each(clamp(x - k))) / n;
  };

/**
 * outBack's `s` for a peak overshoot of `f` (0.1 gives 1.70158): the peak
 * is 4s³ / 27(s + 1)², solved by bisection.
 */
const overshoot = (f: number): number => {
  let lo = 0;
  let hi = 10;
  for (let i = 0; i < 60; i++) {
    const s = (lo + hi) / 2;
    if ((4 * s ** 3) / (27 * (s + 1) ** 2) < f) lo = s;
    else hi = s;
  }
  return (lo + hi) / 2;
};

/**
 * The named easings. Every move in the reel uses one of these, by name
 * (ART.md §9 lists each with its curve and when to use it).
 */
export const EASE = {
  /** Arriving and settling: fast out, long soft landing. cubic-bezier(0.16, 1, 0.3, 1). */
  arrive: swiftOut,
  /** Leaving: slow start, decisive finish. cubic-bezier(0.7, 0, 0.84, 0). */
  leave: swiftIn,
  /** Rest to rest (a card resizing, a pane shrinking, a zoom). cubic-bezier(0.83, 0, 0.17, 1). */
  move: swiftInOut,
  /** Slow continuous change: dims, push-ins, light. Sine in-out. */
  glide: inOutSine,
  /** A small thing coming to a stop: outCubic. */
  settle: outCubic,
  /** A seat, a chip landing, a keycap's return: 12 % overshoot. */
  snap: outBack(overshoot(0.12)),
  /** A heavy thing landing (the toque, the lock chip): 6 % overshoot. */
  drop: outBack(overshoot(0.06)),
  /** A stamp: hard in, no overshoot (scale 1.3 to 1). outQuart. */
  stamp: outQuart,
  /** Anticipation and slams: accelerating in. inCubic. */
  wind: inCubic,
  /** The ticket's feed: 12 printer steps. */
  print: stepped(TICKET.feedSteps),
  /** A spark or thread head along its path: cubic-bezier(0.3, 0, 0.2, 1). */
  travel: cubicBezier(0.3, 0, 0.2, 1),
  linear,
} as const;

export type EaseName = keyof typeof EASE;

/** Standard durations, in beats (one beat is 1 s at 60 BPM; `beats()` converts). */
export const DUR = {
  /** One 32nd: a word landing, a stamp's flash. */
  flick: 1 / 8,
  tick: 1 / 4,
  short: 3 / 8,
  half: 1 / 2,
  std: 3 / 4,
  beat: 1,
  /** Something entering (a window, a card, a chip). */
  enter: 1 / 2,
  /** Something leaving. */
  exit: 3 / 8,
  /** Rest to rest. */
  move: 3 / 4,
  /** A table folding or unfolding. */
  fold: 1 / 2,
  /** A header lighting, and its ignite pulse. */
  light: 1 / 2,
  ignite: 3 / 4,
  /** A line seating: the gap opening, the line landing, the mark held, the mark fading. */
  seatGap: 1 / 4,
  seat: 1 / 2,
  seatHold: 1,
  seatFade: 1,
  /** A thread drawing on, and fading. */
  thread: 1 / 2,
  threadFade: 1 / 2,
  /** A value lifting out of a terminal row onto a card. */
  lift: 3 / 4,
  /** Anticipation before a weighty move. */
  anticipate: 1 / 8,
  stamp: 1 / 4,
  keyDown: 1 / 8,
  keyUp: 1 / 4,
  /** A keycap is up this long either side of its press. */
  keycapLead: 1 / 2,
  keycapHold: 1,
  badge: 1 / 4,
  /** Stagger between items in a cascade, and the most a cascade may take. */
  stagger: 1 / 16,
  staggerMax: 1 / 2,
  /** A pane dimming or brightening. */
  dim: 1 / 2,
  zoom: 3 / 4,
  /** The ticket: feed, hang (from the tear), lift-off. */
  feed: 3 / 4,
  liftOff: 3 / 5,
  /**
   * Kinetic type: slam in, hold, shrink into the pane. The hold is at
   * least PACE.slamHold at full size (rule 7), counted from the landing.
   */
  slam: 3 / 16,
  bigHold: 5 / 4,
  bigShrink: 1 / 2,
  /** The chef: cells lifting, the mask reveal, the glint's sweep. */
  cellsFly: 3 / 4,
  reveal: 3 / 4,
  glint: 1 / 2,
} as const;

/**
 * The pacing rules (jdx on the 3:26 cut: "it goes WAY too fast, i can
 * hardly follow along"), so a first-time viewer can follow every beat
 * without pausing. STORYBOARD.md "Pacing" states them; kit/pace.ts turns
 * them into helpers (Pace, focusOf, readHold) and test/pace.test.ts holds
 * them. Seconds unless a name says beats; at 60 BPM a beat is 1 s.
 */
export const PACE = {
  /**
   * Typing plays at this fraction of real time (rule 2). The rig types a
   * key every 45 ms (22 a second); at 0.5 the screen shows about 11 a
   * second, under the rule's 12. The output after Enter plays at 1x.
   */
  typeRate: 0.5,
  /** Beats the typed command waits on screen before Enter lands: the key's anticipation. */
  enterPause: 1 / 4,
  /** Beats between one focal motion stopping and the next starting (rule 4). */
  gap: 1 / 2,
  /** A layer out of focus, as a multiple of its alpha (rule 4). */
  focusDim: 0.55,
  /**
   * A command's output, once printed, holds still this long (rule 3),
   * plus `readPerLine` for each line the viewer reads past `readFree`
   * (lines dimmed to texture do not count).
   */
  read: 2,
  readPerLine: 0.5,
  readFree: 3,
  /** Install rows under the time-lapse badge stay up at least this long (rule 3). */
  lapseUp: 1.5,
  /** Every section but a ticket ends with its result still for this long (rule 6). */
  restOut: 1.5,
  /** Kinetic type holds at full size at least this long before it shrinks (rule 7). */
  slamHold: 1,
  /** At most one caption change (a caption arriving, or leaving bare) in this long (rule 5). */
  captionEvery: 4,
} as const;

/** Distances and amounts the motion vocabulary shares, px unless noted. */
export const MOTION = {
  /** A window or card entering rises this far; leaving, it falls this far. */
  riseIn: 24,
  fallOut: 16,
  /** A card or file card entering from the right edge. */
  slideIn: 60,
  /** A seated line drops in from this far above its row. */
  seatDrop: 28,
  /** Anticipation: a weighty thing dips this far opposite its move first. */
  anticipate: 6,
  /** A file card in flight tilts this many degrees, settling to 0. */
  flightTilt: 1.5,
  /** An arc's bulge as a fraction of its length (motion.arc lift). */
  arcLift: 0.2,
  /** A stamp lands from this scale. */
  stampFrom: 1.3,
  /**
   * A slam lands from this scale: 1.2 (at 1.4 `mise use jq`'s approach ran
   * off the frame's left edge and over the card's rows).
   */
  slamFrom: 1.2,
  /** A pressed keycap sinks this far. */
  keyPress: 6,
  /** A pushed-in panel grows this much over its hold. */
  pushIn: 0.04,
  /** The one shake (`prod`): amplitude px and decay s (fx.shake). */
  shake: { amp: 8, decay: 0.12 },
} as const;
