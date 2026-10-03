// from jdx/hk@37937824 docs/.vitepress/theme/showreel/bible.ts
// The reel's shared contract: the frame, the clock (re-exported from
// timeline.ts), the palette, and the shape of a scene. Scenes own everything
// between two bar lines; handoff.ts owns the frame on each bar line. Nothing
// here draws, so any module can import it without pulling in drawing code.
//
// Adapted for mise: the palette is mise's brand (the dark site tokens, the
// social card and the hero terminal), and the facts are left open for the
// capture run's versions file.

import type { BoundaryId } from "./handoff";
import type { Pillar, SectionId } from "./timeline";
import type { Caption } from "./type";

/** Logical frame size. Scenes draw in these units at any output resolution. */
export const W = 1920;
export const H = 1080;

// The clock lives in timeline.ts, which imports nothing, so the landing page
// can read the chapters without pulling in the drawing code.
export {
  ACTS,
  type Act,
  type ActId,
  actOf,
  BAR,
  BEAT,
  BEATS,
  BPM,
  bar,
  beat,
  type Chapter,
  CHAPTERS,
  chaptersVtt,
  DURATION,
  type Pillar,
  type Section,
  type SectionId,
  SECTIONS,
  sec,
  TICKET_BEATS,
} from "./timeline";

/**
 * The stage every scene shares: side margins at x 160 and 1760, actors between
 * y 100 and FLOOR, and the captions' band below CAPTION_TOP (type.ts) kept
 * quiet while a caption is up.
 */
export const STAGE = { left: 160, right: 1760, top: 100, floor: 700 } as const;
/** Actors stand at or above this y. */
export const FLOOR = STAGE.floor;

/**
 * mise's dark brand, one step darker than the page so the video reads as an
 * object on it. Colour rules (plan §5): a pillar colour only ever means its
 * pillar; red means an error, in a terminal only; sage never means "passed"
 * (a pass is a paper tick); paper is the captions and the copy.
 */
export const PALETTE = {
  /** Deepest background: behind `open` and `end`, and the <video> letterbox. */
  night: "#110e11",
  /** Default stage: the social card's background. */
  bg: "#171417",
  /** The page's own dark background (--vp-c-bg dark). */
  page: "#191619",
  /** Raised surfaces: cards, panels, chrome bars (--vp-c-bg-alt dark). */
  surface: "#211d21",
  /** Chips and pills on a surface (--vp-c-bg-soft dark). */
  elevated: "#292329",
  /** Hairlines and window edges (--vp-c-divider dark). */
  divider: "#3b323a",
  /** The social card's rule. */
  rule: "#3d3540",
  /** Type (docs dark text tokens). */
  text1: "#f1e9ed",
  text2: "#c1b3bc",
  text3: "#a3949f",
  /** The social card's warm off-white: captions, tickets' titles, the end card. */
  paper: "#f4eee3",
  paperDim: "#c2b6a4",
  /** Dev tools: [tools], backends, packslip (--vp-c-brand-1 dark). */
  tools: "#ed9fbc",
  /** Environments: [env] (--mise-accent-gold). */
  env: "#e1bd87",
  /** Tasks: [tasks], daemons. */
  tasks: "#adce9c",
  /** The machine: dotfiles, everywhere, the new machine (the bootstrap pillar). */
  machine: "#eaa58e",
} as const;

/** Each pillar's colour (timeline.ts ACTS). */
export const PILLAR: Record<Pillar, string> = {
  tools: PALETTE.tools,
  env: PALETTE.env,
  tasks: PALETTE.tasks,
  machine: PALETTE.machine,
};

/**
 * A terminal: the landing hero's colours (landing.css .workbench-terminal),
 * with ANSI colours kept true to the capture inside panes only. ANSI yellow
 * is lemon, never the env pillar's gold. The prompt is a neutral mauve
 * (kit/style.ts COLOR.prompt), not the hero's pink: pink means tools, and a
 * prompt on every line would spend it (ART.md §15).
 */
export const TERM = {
  /** Window body. */
  window: "#251d26",
  /** Chrome bar. */
  chrome: "#2d242e",
  edge: "#3b323a",
  /** Window buttons: neutral, so no pillar colour appears outside its meaning. */
  dots: ["#5a4c58", "#5a4c58", "#5a4c58"] as const,
  /** Default foreground. */
  text: "#f3eaf0",
  /** The terminal's label and dim (SGR 2) text. */
  dim: "#a3949f",
  /** The shell's prompt, set from a plain string line (captured rows keep their own colours). */
  prompt: "#c5b2c3",
  /** The block cursor. */
  cursor: "#f3eaf0",
  /**
   * SGR 30–37 and 90–97: Catppuccin Frappé, as hk's reel, with green the
   * hero terminal's output colour and yellow lemon.
   */
  ansi: [
    "#51576d",
    "#e78284",
    "#b8d6a9",
    "#f0d27a",
    "#8caaee",
    "#f4b8e4",
    "#81c8be",
    "#b5bfe2",
    "#626880",
    "#ea999c",
    "#c6e2b9",
    "#f5dc94",
    "#a3bcf2",
    "#f6c6e9",
    "#99d1db",
    "#d4d9ee",
  ] as const,
} as const;

/**
 * What a scene may depend on besides time: the capture run's versions file
 * (the Node majors and the versions the terminals print), once the capture
 * harness writes it. Null draws a reel that depends on none. Nothing reads
 * a field yet; the capture PR types them here.
 */
export type ReelFacts = Readonly<Record<string, unknown>>;

/**
 * The bar lines a section meets in a film that plays the sections out of
 * the source order (film.ts): `in`, the bar line it starts from, whose
 * persistent layers it inherits; `out`, the one it ends on. A ticket takes
 * down the stage of the section the film plays before it, not the source's,
 * so a scene reads its bar lines from kit/grey.ts `G.prev` and `G.next`,
 * which take these over the timeline's own (handoff.ts handoffIn,
 * handoffOut). Absent, a section keeps its own bar lines.
 */
export interface Join {
  in?: BoundaryId;
  out?: BoundaryId;
  /** A ticket's meta line ("ACT II"): the act's number in the film's order. */
  meta?: string;
}
export type Joins = Partial<Record<SectionId, Join>>;

export interface SceneEnv {
  W: number;
  H: number;
  /** Global time in seconds. */
  t: number;
  facts: ReelFacts | null;
  /** A film's joins (Join); none in the source reel. */
  joins?: Joins;
}

export interface Scene {
  id: SectionId;
  /**
   * Global start and end, seconds: its section's, from `sec(id)`. The frame
   * at `end` belongs to the next scene.
   */
  start: number;
  end: number;
  /** Draw one frame. `lt` is local time, `t - start`. Paint the whole frame. */
  draw(ctx: CanvasRenderingContext2D, lt: number, env: SceneEnv): void;
  /**
   * The section's must-read captions, in its local beats. The reel draws
   * them over the scene in the lower third (type.ts), so a scene lists them
   * here and keeps that band quiet while they are up.
   */
  captions?(facts: ReelFacts | null): readonly Caption[];
  /**
   * The lit screen on the frame at `lt`, if there is one: a terminal's
   * window, which the reel's vignette leaves out. It is given the same
   * facts as `draw`, so a scene whose picture depends on them needs no
   * state carried over from the frame it last drew.
   */
  lit?(lt: number, env: Pick<SceneEnv, "facts" | "joins">): LitRect | null;
}

/**
 * A screen in the frame, logical px: a terminal's window, lit from within.
 * The vignette spares it, so the pane's colours read as the terminal's
 * wherever it stands.
 */
export interface LitRect {
  x: number;
  y: number;
  w: number;
  h: number;
  /** How far the vignette spares it, 0 to 1. */
  alpha: number;
}
