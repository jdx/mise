// The chef, mise's mascot and the reel's bookends (ART.md §11): one drawing
// in two forms. The block chef is the logo exactly as `mise` prints it at
// the top right of its help screen (src/assets/logo.txt, in the terminal's
// ANSI green); the vector chef is docs/public/logo-dark.svg's filled
// outlines, in paper. The open lifts the block chef's cells out of the help
// screen into a mosaic turned to the logo's tilt and reveals the vector
// chef through it; the morph flies the station card's three lit headers
// into the toque's three lobes and drops the toque on the chef; the end
// card sweeps a glint across the hat. The eyes never move: they are the
// logo's own closed arcs, and nothing here draws them apart from the rest.
//
// Both drawings are copied into this module as constants, so no frame
// reads a file or parses markup: test/chef.test.ts fails when logo.txt or
// logo-dark.svg changes without them, and checks the settled vector chef
// against the SVG as Chromium renders it.
//
// Every function is a pure function of its arguments (section beats, or
// global seconds for the motes), draws with the caller's transform
// multiplied in, never set, and imports its numbers from kit/style.ts.
// Offscreen layers are avoided: effects that must stay inside the chef's
// fill (the glint, the lobes' tint, the mask reveal) clip to its own path.
// In Node, where there is no Path2D, the drawing calls are no-ops.

import { type LitRect, type Pillar, PILLAR, TERM } from "../bible";
import { mix, rgba } from "../color";
import { castShadow, glow, motes, roundedRect, sparkle } from "../fx";
import { clamp, DEG, lerp, progress, smoothstep, TAU } from "../math";
import { drawText, font, MONO } from "../type";
import { arc, curveAt, type Pt, type Rect } from "./motion";
import { blockRects, type CellRect } from "./term";
import {
  CARD_ART,
  CHEF_ART,
  COLOR,
  DUR,
  EASE,
  GLOW,
  MOTION,
  PILLAR_BAND,
  PILLAR_BRIGHT,
  SHADOW,
  STROKE,
  TYPE,
  beats,
} from "./style";

// The sources.

/**
 * src/assets/logo.txt, line by line, as `mise` prints it (usage's `logo`,
 * styled green) at columns 66–79 of rows 1–7 of its help screen. Trailing
 * spaces are not part of the art: the file has none.
 */
export const LOGO_TXT: readonly string[] = [
  "  ▄▄  ▄▄▄  ▄▄",
  " ▟  ▙▟   ▙▟  ▙",
  " ▜▄▄▄▄▄▄▄▄▄▄▄▛",
  "  ▀▀▀▀▀▀▀▀▀▀▀",
  "   ▗▄▄▄▄▄▄▄▖",
  "   ▌ ▀   ▔ ▐",
  "   ▝▄ ▀▄▀ ▄▘",
];

/**
 * docs/public/logo-dark.svg's five `d` attributes, in document order, as
 * the file has them: filled outlines (the dark lines are the holes between
 * them), nonzero fill, in the viewBox `CHEF_ART.viewBox`. The third path's
 * first subpath is the chef's outer silhouette.
 */
export const CHEF_PATHS: readonly string[] = [
  "M219.33,161.64c-.19.04-.48.35-.49.89-.02.6.28,1.18.9,1.73.21-.99.17-" +
    "1.75-.1-2.28-.1-.19-.23-.34-.31-.34Z",
  "M111.22,112.48c1.3-.2,2.14-.96,2.63-2.37.38-1.09.3-2.06-.24-2.95-1.2" +
    "9-2.15-5.31-4.02-11.05-5.14.9,3.02,1.91,5.39,3.17,7.45,1.16,1.88,3.5" +
    "6,3.22,5.49,3.02Z",
  "M240.15,132.19c-4.92-1.61-10.04-1.87-15.82-.78l-4.57-10.95c14.88-12." +
    "45,20.94-33.82,14.08-51.85-7.82-20.54-29.73-33.19-50.99-29.42-8.84,1" +
    ".58-17.22,5.74-23.83,11.74-6.13-5.86-13.82-9.93-21.9-11.46-7.43-1.41" +
    "-14.96-.94-21.8,1.36-11.75,3.95-31.57,18.58-29.25,46.83-14.19.96-29." +
    "57,8.1-37.1,23.49-5.29,10.82-5.92,23.57-1.71,34.99,4.12,11.18,12.26," +
    "19.89,22.93,24.53,9.32,4.05,20.08,4.83,29.78,2.29,2.25,4.15,5.17,9.6" +
    "2,7.67,14.29l.61,1.14c1.15,2.15,2.23,4.18,3.16,5.91-.24.23-.48.47-.7" +
    "2.7-7.98,8.07-10.91,20.35-7.29,30.54,3.75,10.57,14.5,17.93,25.73,17." +
    "58,4.58-.19,8.93-1.65,13.01-4.37,10.21,12.65,25.75,21.09,42.08,22.53" +
    ",1.57.14,3.17.21,4.74.21,2.51,0,5.05-.17,7.55-.52,16.04-2.22,30.7-11" +
    ".39,40.21-25.16,6.84-9.9,10.51-21.25,10.62-32.81,0,0,.09-15.52-4.7-2" +
    "6.96,7.62-4.07,12.85-11.45,13.72-19.82,1.12-10.83-5.39-20.49-16.22-2" +
    "4.02ZM250.77,155.57c-.77,7.46-6.21,13.92-13.84,16.45-3.31,1.09-8.61," +
    "1.95-13.6,1.1-4.17,7.07-10.88,11.95-18.92,13.74-8.17,1.82-16.83.12-2" +
    "3.76-4.66-.82-.56-1.31-1.34-1.41-2.26-.09-.8.12-1.63.61-2.33.55-.8,1" +
    ".4-1.36,2.32-1.53.88-.16,1.76.04,2.53.58,5.33,3.67,11.98,4.96,18.26," +
    "3.51,5.74-1.3,10.65-4.67,13.82-9.49-.99-.65-1.86-1.41-2.58-2.25-2.44" +
    "-2.85-2.65-7.69-.48-11.01,1.73-2.65,4.5-3.63,7.59-2.7,1.99.6,3.44,1." +
    "75,4.32,3.43,1.38,2.62.98,5.97.39,8.37,4.36.49,9.09-.56,12.4-2.76,3." +
    "92-2.61,7.01-8.17,5-13.33-2.11-5.44-8.83-7.15-14.56-6.43-5.86.73-11." +
    "52,3.31-17,5.81l-2.14.97c-6.17,2.79-12.27,5.82-18.12,9-11.79,6.42-23" +
    ".42,13.95-34.54,22.39-.82.63-1.76.85-2.69.66-.83-.17-1.58-.66-2.12-1" +
    ".37-1.02-1.35-1.14-3.4.6-4.72,18.3-13.89,38.08-25.43,58.79-34.32l1.3" +
    "-.56c.79-.34,1.65-.71,2.39-1.02l-1.6-4.08c-9.53,3.23-56.72,20.53-88." +
    "07,55.46l.47.83c5.12-1.67,10.92-1.95,16.37-.78,2.16.46,3.07,2.54,2.6" +
    "9,4.29-.19.9-.68,1.64-1.38,2.09-.69.44-1.55.59-2.42.4-7.82-1.67-15.1" +
    "6-.02-20.15,4.51-4.94,4.49-6.99,11.63-5,17.37,2.07,5.95,8.48,9.99,14" +
    ".27,9.01,6.76-1.11,11.28-7.95,14.91-13.44l.17-.26c.51-.77,1.23-1.28," +
    "2.05-1.44.81-.16,1.69.03,2.46.54.74.49,1.3,1.25,1.54,2.08.19.67.26,1" +
    ".71-.47,2.81-1.01,1.53-1.82,2.71-2.62,3.82,9.66,17.9,31.66,27.89,51." +
    "21,23.23,19.83-4.71,34.94-24.03,35.14-44.93.02-1.79,1.33-3.09,3.13-3" +
    ".09,1.84.02,3.72,1.39,3.7,3.66-.1,10.32-3.39,20.46-9.52,29.34-8.53,1" +
    "2.34-21.63,20.55-35.95,22.54-2.21.31-4.47.46-6.7.46-1.4,0-2.81-.06-4" +
    ".21-.18-16.8-1.47-32.66-11.2-41.45-25.4-4.52,4.66-9.24,7.02-14.43,7." +
    "23-.2,0-.41.01-.61.01-8.16,0-16.26-5.69-19.01-13.46-2.84-8-.49-17.69" +
    ",5.85-24.1,1.24-1.26,2.67-2.4,4.25-3.41-.91-1.65-3.04-5.64-5.5-10.25" +
    "l-.36-.67c-3.73-6.98-8.37-15.65-10.57-19.59-9.4,3.86-20.7,3.69-30.26" +
    "-.47-9.1-3.96-16.05-11.41-19.58-20.98-3.62-9.84-3.09-20.82,1.46-30.1" +
    "2,7.59-15.51,25.34-21.2,38.7-19.94-5.97-28.95,13.1-43.54,24.05-47.22" +
    ",5.83-1.96,12.28-2.35,18.66-1.14,8.88,1.68,17.23,6.97,22.93,14.52,6." +
    "14-7.66,15.26-13.04,25.05-14.78,18.36-3.26,37.31,7.7,44.09,25.51,6.4" +
    "6,16.98-.37,37.4-15.87,47.56l8.49,20.36c6.84-2.12,12.42-2.32,17.56-." +
    "64,7.98,2.61,12.79,9.63,11.97,17.48ZM167.49,236.06l5.45-10.51c.47-.9" +
    "1,1.26-1.54,2.23-1.78.87-.22,1.84-.11,2.64.31,1.68.87,2.25,2.64,1.38" +
    ",4.31l-1.4,2.7c14.05,6.41,30.1,2.11,39.07-10.47.7-.99,1.77-1.5,2.94-" +
    "1.4,1.25.12,2.41.95,2.94,2.12.51,1.11.38,2.33-.35,3.36-6.13,8.61-15." +
    "64,14.37-26.08,15.81-1.75.24-3.52.36-5.27.36-5.7,0-11.22-1.24-16.41-" +
    "3.69l-.89,1.71c-.44.85-1.14,1.45-2.03,1.73-.93.29-1.96.2-2.84-.26-1." +
    "68-.87-2.25-2.64-1.38-4.31ZM165.08,213.23c.56-5.42,3.55-10.04,8.2-12" +
    ".68,4.66-2.64,10.16-2.83,15.1-.51,1.15.54,1.87,1.55,1.97,2.79.11,1.2" +
    "9-.48,2.58-1.5,3.29-.94.65-2.11.73-3.21.22-3.02-1.41-6.31-1.35-9.01." +
    "18-2.67,1.51-4.4,4.26-4.73,7.53-.2,1.9-1.58,2.76-2.81,2.93-.17.02-.3" +
    "3.03-.5.03-.13,0-.25,0-.38-.02-1.69-.17-3.36-1.55-3.13-3.77ZM204.13," +
    "210.35c-1.15-.49-1.89-1.44-2.04-2.63-.16-1.26.39-2.58,1.4-3.34.95-.7" +
    "2,2.14-.85,3.28-.37,4.78,2.01,7.4,7.31,5.95,12.08-1.08,3.58-4.41,6.3" +
    "4-8.29,6.88-.46.06-.92.1-1.37.1-.67,0-1.33-.07-1.99-.21-1.26-.26-2.2" +
    "1-1.1-2.6-2.31-.41-1.24-.14-2.63.68-3.54.75-.83,1.85-1.17,3.02-.93h0" +
    "c2.04.43,3.42-.79,3.83-2.11.38-1.21.08-2.8-1.87-3.62ZM222.48,198.34h" +
    "-.11c-1.16.18-2.26-.27-3.03-1.2-1.33-1.61-3.23-2.42-5.2-2.21-1.91.19" +
    "-3.54,1.37-4.48,3.23-.44.86-1.12,1.47-1.99,1.75-.36.12-.73.17-1.11.1" +
    "7-.59,0-1.19-.14-1.72-.42-1.7-.86-2.29-2.64-1.44-4.32,2.04-4.03,5.82" +
    "-6.73,10.11-7.21,4.26-.48,8.35,1.24,11.2,4.69.78.95,1,2.12.59,3.21-." +
    "44,1.18-1.54,2.08-2.81,2.29ZM125.47,213.14c-1.24-.2-2.2-.97-2.63-2.1" +
    "2-.46-1.2-.24-2.6.56-3.58.73-.9,1.84-1.3,3.06-1.11,1.39.23,3.21.46,4" +
    ".81.67l2.78.36c1.22.16,2.26.88,2.83,1.97.58,1.09.59,2.35.03,3.45-1.5" +
    "9,3.15-3.98,7.88-4.91,9.72-.53,1.04-1.48,1.73-2.61,1.88-.1.01-.2.02-" +
    ".3.03-.07,0-.14,0-.21,0-1.24,0-2.4-.61-3.04-1.58-.61-.94-.65-2.08-.1" +
    "1-3.15l3.06-6.02-3.31-.54Z",
  "M160.79,83.01c1.12-.09,1.79-.47,2.1-1.2.76-1.78-.37-5.38-2.83-9.02-." +
    "59,1.48-.97,2.69-1.13,3.6l-.02.12c-.61,3.62.03,5.22.67,5.93.47.52,1," +
    ".56,1.2.55Z",
  "M204.78,106.63c1.55-.56,3.51-.1,4.25,1.89l1.21,3.23c7.39-5.48,12.3-1" +
    "3.84,13.48-22.97,1.24-9.62-1.6-18.89-8.01-26.1-6.9-7.77-18.14-11.77-" +
    "29.35-10.44-10.01,1.19-18.34,6.39-22.85,14.27.26.38.55.79.86,1.22l.0" +
    "9.13c3.22,4.52,8.62,12.07,3.4,19.04-1.14,1.52-3.14,2.58-5.48,2.91-2." +
    "54.35-5.12-.2-6.73-1.43-6.48-4.93-4.33-14.54-.15-21.92-7.02-11-19.84" +
    "-16.67-31.92-14.12-7.91,1.67-14.36,6.32-18.68,13.45-4.97,8.23-6.53,1" +
    "9.29-4.07,28.89,4.33.58,10.5,2.04,15.47,5.7,4.91,3.62,6.04,9.94,2.64" +
    ",14.71-1.59,2.24-4.12,3.72-7.11,4.18-3.18.42-6.39-.48-8.8-2.46-4.57-" +
    "3.76-6.56-10.13-7.9-15.88-7.03-1.53-14.52.07-19.2,2.09-7.25,3.13-12." +
    "83,9.07-15.71,16.74-3.05,8.11-2.69,17.2.99,24.93,3.32,6.98,9.46,12.2" +
    "7,17.3,14.9,8.38,2.81,17.49,2.18,24.99-1.72.33-.17.67-.28,1.01-.33.1" +
    "3-.02.27-.03.42-.03,1.48,0,2.6,1.07,3.11,1.98,1.19,2.13,3.97,7.38,6." +
    "65,12.45l.14.26c2.85,5.39,5.79,10.96,7.5,14.06,32.14-34.76,79.25-52." +
    "15,88.81-55.42l-8.46-19.72c-.8-2.12.49-3.91,2.1-4.52Z",
];

// The block chef's pieces.

/** A piece of a character cell, in cell units (0 to 1), with its ink's coverage. */
export interface CellPiece {
  x: number;
  y: number;
  w: number;
  h: number;
  alpha: number;
}

/** Whole quadrants of a cell rect (kit/term.ts blockRects), one piece each; any other rect whole. */
function quadrants(r: CellRect, alpha: number): CellPiece[] {
  const [x, y, w, h] = r;
  const half = (v: number) => Math.abs(v * 2 - Math.round(v * 2)) < 1e-9;
  if (![x, y, w, h].every(half)) return [{ x, y, w, h, alpha }];
  const out: CellPiece[] = [];
  for (let qy = y; qy < y + h - 1e-9; qy += 0.5)
    for (let qx = x; qx < x + w - 1e-9; qx += 0.5)
      out.push({ x: qx, y: qy, w: 0.5, h: 0.5, alpha });
  return out;
}

/** One piece of the block chef: which character cell of the logo, and which part of it. */
export interface ChefCell {
  /** Row and column in logo.txt. */
  row: number;
  col: number;
  /** The piece, in cell units. */
  piece: CellPiece;
  /** Its column on the half-cell grid, 0 to 2 × width − 1: the open's stagger order, left to right. */
  qcol: number;
}

/**
 * Every piece of the block chef, row by row: the rects the terminal fills
 * for each glyph (kit/term.ts blockRects, so a lifted copy starts exactly on
 * the pane's own), cut into quadrants where they are whole quadrants, so
 * each quadrant flies on its own. A character that is neither a block
 * element nor a space is an error: the logo is block art.
 */
export function chefCells(lines: readonly string[] = LOGO_TXT): ChefCell[] {
  const out: ChefCell[] = [];
  lines.forEach((line, row) =>
    Array.from(line).forEach((ch, col) => {
      if (ch === " ") return;
      const blk = blockRects(ch);
      if (!blk)
        throw new Error(`logo row ${row} col ${col}: ${ch} is not a block`);
      for (const r of blk.rects)
        for (const p of quadrants(r, blk.alpha))
          out.push({
            row,
            col,
            piece: p,
            qcol: 2 * col + Math.floor(p.x * 2 + 1e-9),
          });
    }),
  );
  return out;
}

/** The block chef's pieces, parsed once. */
export const CHEF_CELLS: readonly ChefCell[] = chefCells();

/** The logo's size in characters: 14 columns (CHEF_ART.block.cols), 7 rows. */
export const LOGO_COLS = Math.max(...LOGO_TXT.map((l) => Array.from(l).length));
export const LOGO_ROWS = LOGO_TXT.length;

/**
 * A character cell on screen: the terminal's cell for screen line `line`,
 * column `col` (kit/term.ts TermLayout.cell, a line's height and one
 * advance wide).
 */
export type CellAt = (line: number, col: number) => Rect;

/** A piece placed on the frame: its centre, size and turn (radians). */
export interface Placed {
  cx: number;
  cy: number;
  w: number;
  h: number;
  rot: number;
}

/**
 * Where piece `c` sits in the help screen: logo row 0 is screen line
 * `line0` (CHEF_ART.block.row), column 0 is `col0` (CHEF_ART.block.col).
 */
export function cellOnScreen(
  c: ChefCell,
  cell: CellAt,
  line0: number = CHEF_ART.block.row,
  col0: number = CHEF_ART.block.col,
): Placed {
  const r = cell(line0 + c.row, col0 + c.col);
  return {
    cx: r.x + (c.piece.x + c.piece.w / 2) * r.w,
    cy: r.y + (c.piece.y + c.piece.h / 2) * r.h,
    w: c.piece.w * r.w,
    h: c.piece.h * r.h,
    rot: 0,
  };
}

const TILT = CHEF_ART.tilt * DEG;

/**
 * Where piece `c` lands in the mosaic at `place`: each quadrant one
 * `CHEF_ART.mosaic` cell, the grid centred on the place (nudged down
 * `mosaic.dy`) and turned to the logo's tilt, so the pixel hat lands on
 * the vector hat.
 */
export function cellInMosaic(
  c: ChefCell,
  place: Rect = CHEF_ART.place,
): Placed {
  const { cellW, cellH, dy } = CHEF_ART.mosaic;
  const cw = 2 * cellW;
  const ch = 2 * cellH;
  const lx = -(LOGO_COLS * cw) / 2 + (c.col + c.piece.x + c.piece.w / 2) * cw;
  const ly = -(LOGO_ROWS * ch) / 2 + (c.row + c.piece.y + c.piece.h / 2) * ch;
  const cos = Math.cos(TILT);
  const sin = Math.sin(TILT);
  return {
    cx: place.x + place.w / 2 + lx * cos - ly * sin,
    cy: place.y + place.h / 2 + dy + lx * sin + ly * cos,
    w: c.piece.w * cw,
    h: c.piece.h * ch,
    rot: TILT,
  };
}

/** A placed piece's rectangle, grown by `over` px on every side. */
function fillPlaced(
  ctx: CanvasRenderingContext2D,
  p: Placed,
  over = 0.5,
): void {
  if (p.rot) {
    ctx.save();
    ctx.translate(p.cx, p.cy);
    ctx.rotate(p.rot);
    ctx.fillRect(
      -p.w / 2 - over,
      -p.h / 2 - over,
      p.w + 2 * over,
      p.h + 2 * over,
    );
    ctx.restore();
  } else {
    ctx.fillRect(
      p.cx - p.w / 2 - over,
      p.cy - p.h / 2 - over,
      p.w + 2 * over,
      p.h + 2 * over,
    );
  }
}

// The vector chef.

const VB = CHEF_ART.viewBox;

/** Pixels per SVG unit at `place` (the place keeps the viewBox's aspect). */
export const chefScale = (place: Rect = CHEF_ART.place): number =>
  place.w / VB.w;

/** An SVG-unit point of the chef, in px at `place`. */
export function chefToPx(
  x: number,
  y: number,
  place: Rect = CHEF_ART.place,
): Pt {
  const s = chefScale(place);
  return { x: place.x + (x - VB.x) * s, y: place.y + (y - VB.y) * s };
}

/**
 * The toque, measured off logo-dark.svg in Chromium (test/chef.test.ts
 * holds both to the file), in SVG units: kit/style.ts CHEF_ART's lobes and
 * hat cut, which the brand pass checked with an overlay.
 */
export const TOQUE = {
  lobes: CHEF_ART.lobes,
  hat: CHEF_ART.hat,
  /** The vertices after this one run along the cut (the rest are the viewBox's edges). */
  cutFrom: CHEF_ART.hatCutFrom,
} as const;

/** Lobe `i` of the toque (0 left, 1 middle, 2 right) in px at `place`: its centre, and its radius to the stroke's middle. */
export function lobeAt(
  i: number,
  place: Rect = CHEF_ART.place,
): { x: number; y: number; r: number } {
  const l = TOQUE.lobes[i];
  const p = chefToPx(l.x, l.y, place);
  return { ...p, r: l.r * chefScale(place) };
}

/** The pillar whose header becomes lobe `i`. */
export const lobePillar = (i: number): Pillar => CHEF_ART.lobeOf[i];

let paths: {
  all: Path2D;
  parts: Path2D[];
  outline: Path2D;
  hat: Path2D;
  body: Path2D;
} | null = null;

/**
 * The chef's paths in SVG units, built once: each of the SVG's paths (the
 * fill: filled one by one, as the browser draws the file, they match its
 * render to a level), every path as one (for clips and the shadow, where
 * the joins' antialiasing does not show), the outer silhouette alone, the hat
 * (TOQUE.hat, its cut let down one unit so hat and body overlap
 * rather than meet in a seam), and the body (the viewBox less the hat's
 * own polygon). Null where there is no Path2D (Node).
 */
function chefPaths(): typeof paths {
  if (paths) return paths;
  if (typeof Path2D === "undefined") return null;
  const parts = CHEF_PATHS.map((d) => new Path2D(d));
  const all = new Path2D();
  for (const q of parts) all.addPath(q);
  const outline = new Path2D(
    CHEF_PATHS[2].slice(0, CHEF_PATHS[2].indexOf("M", 1)),
  );
  const poly = (down: number) => {
    const p = new Path2D();
    TOQUE.hat.forEach(([x, y], i) => {
      // Only the cut moves, not the viewBox's edges.
      const yy = i >= TOQUE.cutFrom ? y + down : y;
      if (i) p.lineTo(x, yy);
      else p.moveTo(x, yy);
    });
    p.closePath();
    return p;
  };
  const body = new Path2D();
  body.rect(VB.x - 10, VB.y - 10, VB.w + 20, VB.h + 20);
  body.addPath(poly(0));
  paths = { all, parts, outline, hat: poly(1), body };
  return paths;
}

/** Multiply the chef's SVG units onto the caller's transform at `place`. */
function toChef(ctx: CanvasRenderingContext2D, place: Rect): void {
  const s = chefScale(place);
  ctx.translate(place.x, place.y);
  ctx.scale(s, s);
  ctx.translate(-VB.x, -VB.y);
}

/** A radial glow's size and strength (fx.glow). */
export interface Glow {
  radius: number;
  alpha: number;
}

export interface ChefOptions {
  /** Group opacity. */
  alpha?: number;
  /** Which part: the whole chef, the hat (above the band) or the rest. */
  part?: "all" | "hat" | "body";
  /** Moves the hat part by this much, px (the toque dropping). */
  hatDy?: number;
  /** The raised shadow's strength, 0 to 1 (1 is SHADOW.raised at 60 %). */
  shadow?: number;
  /** The backlight behind it, centred on the place; none by default. */
  glow?: Glow | null;
  /**
   * The lobes' tint, 0 to 1: each lobe's pillar over the hat's paper
   * (circles of the lobe's radius less 6 units, at 55 %), clipped to the
   * fill. The morph lands the lobes tinted and drains this to 0.
   */
  tint?: number;
  /**
   * The glint's sweep across the hat, 0 to 1 (drawn only strictly
   * between), clipped to the hat within the chef's silhouette.
   */
  glint?: number;
  /**
   * An extra clip for the fill (not the shadow), in px: the open's mask
   * reveal. Called with a fresh path begun; it adds to the path, and the
   * fill is clipped to it.
   */
  mask?: (ctx: CanvasRenderingContext2D) => void;
  /** Fill the silhouette behind the chef in this colour first, so nothing shows through its dark lines. */
  backing?: string | null;
  /**
   * Draw only the shadow, or only the rest: a chef drawn in parts casts
   * every part's shadow before it fills any, so no part's shadow falls on
   * another (default both).
   */
  layer?: "both" | "shadow" | "fill";
}

/**
 * The vector chef at `place` (CHEF_ART.place by default): logo-dark.svg's
 * paths in paper, with the raised shadow at 60 %, and any of the backlight,
 * the lobes' tint, the glint and a mask. The eyes are the logo's own and
 * never change.
 */
export function drawChef(
  ctx: CanvasRenderingContext2D,
  place: Rect = CHEF_ART.place,
  o: ChefOptions = {},
): void {
  const alpha = o.alpha ?? 1;
  if (alpha <= 0) return;
  const P = chefPaths();
  const part = o.part ?? "all";
  const hatDy = part === "hat" ? (o.hatDy ?? 0) : 0;
  ctx.save();
  ctx.globalAlpha *= alpha;
  if (o.glow && o.glow.alpha > 0 && (o.layer ?? "both") !== "fill")
    glow(
      ctx,
      place.x + place.w / 2,
      place.y + place.h / 2 + hatDy,
      o.glow.radius,
      CHEF_ART.fill,
      o.glow.alpha,
    );
  if (!P) {
    ctx.restore();
    return;
  }
  ctx.translate(0, hatDy);
  const clipPart = () => {
    if (part === "hat") ctx.clip(P.hat);
    else if (part === "body") ctx.clip(P.body, "evenodd");
  };
  // The shadow, never masked (it fades with `shadow`): the whole chef's,
  // clipped for a part to that part moved by the shadow's offset, so the
  // hat's and the body's shadows tile into the whole one at rest.
  const layer = o.layer ?? "both";
  const shadow = layer === "fill" ? 0 : 0.6 * (o.shadow ?? 1);
  if (shadow > 0) {
    ctx.save();
    if (part !== "all") {
      const m = ctx.getTransform();
      ctx.translate(SHADOW.raised.dx, SHADOW.raised.dy);
      toChef(ctx, place);
      clipPart();
      ctx.setTransform(m);
    }
    toChef(ctx, place);
    castShadow(ctx, SHADOW.raised, P.all, shadow);
    ctx.restore();
  }
  if (layer === "shadow") {
    ctx.restore();
    return;
  }
  if (o.mask) {
    ctx.beginPath();
    o.mask(ctx);
    ctx.clip();
  }
  toChef(ctx, place);
  clipPart();
  if (o.backing) {
    ctx.fillStyle = o.backing;
    ctx.fill(P.outline);
  }
  ctx.fillStyle = CHEF_ART.fill;
  for (const q of P.parts) ctx.fill(q);
  const tint = clamp(o.tint ?? 0);
  const g = o.glint ?? 0;
  if (tint > 0) {
    ctx.save();
    ctx.clip(P.all);
    TOQUE.lobes.forEach((l, i) => {
      ctx.fillStyle = rgba(PILLAR[lobePillar(i)], 0.55 * tint);
      ctx.beginPath();
      ctx.arc(l.x, l.y, l.r - 6, 0, TAU);
      ctx.fill();
    });
    ctx.restore();
  }
  // The glint inside the silhouette, dark lines and all: on the paper
  // alone a near-white stripe does not read, so it shows where it crosses
  // the hat's lines, as light running over an embossed mark.
  if (g > 0 && g < 1) {
    ctx.save();
    ctx.clip(P.outline);
    drawGlintStripe(ctx, P.hat, g);
    ctx.restore();
  }
  ctx.restore();
}

/** The band's direction (it rises to the right), and its normal. */
const ALONG = { x: Math.cos(TILT), y: Math.sin(TILT) };

/**
 * How far along the band the hat reaches, SVG units: the lobes projected
 * onto it, with their outline (the silhouette sits about 10 units outside
 * the stroke's middle) and the stripe's soft half-width either side.
 */
const SWEEP = (() => {
  const pad = 10 + CHEF_ART.glint.width / 2 + CHEF_ART.glint.soft;
  const along = TOQUE.lobes.map((l) => l.x * ALONG.x + l.y * ALONG.y);
  const r = Math.max(...TOQUE.lobes.map((l) => l.r));
  return {
    from: Math.min(...along) - r - pad,
    to: Math.max(...along) + r + pad,
  };
})();

/**
 * The glint at `u`: a stripe perpendicular to the band, 7 units wide with
 * 12 of soft falloff either side, COLOR.glint at 90 %, `u` of the way along
 * the band across the hat, inside the current clip and the hat's.
 */
function drawGlintStripe(
  ctx: CanvasRenderingContext2D,
  hat: Path2D,
  u: number,
): void {
  const G = CHEF_ART.glint;
  const at = lerp(SWEEP.from, SWEEP.to, u);
  ctx.save();
  ctx.clip(hat);
  // Stand on the band's axis at `at`, the stripe across it.
  ctx.transform(ALONG.x, ALONG.y, -ALONG.y, ALONG.x, 0, 0);
  const half = G.width / 2 + G.soft;
  const grad = ctx.createLinearGradient(at - half, 0, at + half, 0);
  grad.addColorStop(0, rgba(G.color, 0));
  grad.addColorStop(G.soft / (2 * half), rgba(G.color, G.alpha));
  grad.addColorStop(1 - G.soft / (2 * half), rgba(G.color, G.alpha));
  grad.addColorStop(1, rgba(G.color, 0));
  ctx.fillStyle = grad;
  ctx.fillRect(at - half, -400, 2 * half, 800);
  ctx.restore();
}

/** Where the glint's sparkle blooms: the top of the right lobe's outline, px at `place`. */
export function sparklePoint(place: Rect = CHEF_ART.place): Pt {
  const l = TOQUE.lobes[2];
  return chefToPx(l.x, l.y - l.r - 5, place);
}

/**
 * hk's sparkle (fx.ts sparkle) at (x, y) px in COLOR.glint, blooming as
 * `u` goes 0 to 1: its radius rises to CHEF_ART.sparkle.r at 0.5 and falls
 * back to 0 at 1.
 */
export function drawSparkle(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  u: number,
): void {
  if (!(u > 0 && u < 1)) return;
  sparkle(
    ctx,
    x,
    y,
    CHEF_ART.sparkle.r * Math.sin(Math.PI * u),
    1,
    COLOR.glint,
  );
}

/** The chef's backlight at rest: the halo it settles to. */
export const HALO: Glow = { ...GLOW.chefHalo };

/**
 * The backlight `u` beats after a bloom starts: from `from` up to
 * GLOW.chefBloom over 1/8 beat (arrive), then settling to the halo over
 * `settle` beats (default 2, glide); `from` before it starts. Exactly the
 * halo from 1/8 + `settle` beats.
 */
export function bloomAt(u: number, from: Glow = HALO, settle = 2): Glow {
  const B = GLOW.chefBloom;
  const up = DUR.flick;
  if (u <= 0) return { ...from };
  if (u < up) {
    const k = EASE.arrive(u / up);
    return {
      radius: lerp(from.radius, B.radius, k),
      alpha: lerp(from.alpha, B.alpha, k),
    };
  }
  if (u >= up + settle) return { ...HALO };
  const k = EASE.glide((u - up) / settle);
  return {
    radius: lerp(B.radius, HALO.radius, k),
    alpha: lerp(B.alpha, HALO.alpha, k),
  };
}

/** The chef as a lit screen for the vignette (Scene.lit), on open and end. */
export const chefLit = (place: Rect = CHEF_ART.place, alpha = 1): LitRect => ({
  ...place,
  alpha,
});

// Motes.

/** How far the motes keep from anything they must not cross, px. */
const MOTE_CLEAR = 16;

/**
 * The motes round the chef at `place` at GLOBAL time `t` (fx.ts motes: its
 * rect grown by STAGE_FX.motes.spread, each speck a pure function of `t`
 * and its index), kept clear of every `avoid` rect grown 16 px, so they
 * never cross a pane or a line of the name card.
 */
export function drawMotes(
  ctx: CanvasRenderingContext2D,
  t: number,
  o: { place?: Rect; alpha?: number; avoid?: readonly Rect[] } = {},
): void {
  const a = o.alpha ?? 1;
  if (a <= 0) return;
  ctx.save();
  // One clip per rect: each cuts its rect out of what the last left.
  for (const v of o.avoid ?? []) {
    ctx.beginPath();
    ctx.rect(-1e4, -1e4, 2e4, 2e4);
    ctx.rect(
      v.x - MOTE_CLEAR,
      v.y - MOTE_CLEAR,
      v.w + 2 * MOTE_CLEAR,
      v.h + 2 * MOTE_CLEAR,
    );
    ctx.clip("evenodd");
  }
  motes(ctx, o.place ?? CHEF_ART.place, t, a);
  ctx.restore();
}

// The open (ART.md §11 Open), in the open's beats.

/** The open's cues, section beats. */
export const OPEN_CUE = {
  /** Every block cell dips before it lifts. */
  dip: 2.5,
  /** The cells lift, column by column, 1/64 beat apart. */
  fly: 2.5 + DUR.anticipate,
  stagger: 1 / 64,
  /** The mask reveal, and the cells fading out, to the resolve on beat 4. */
  reveal: 3.25,
  resolved: 4,
  /** Name card and chef leave; the chef 1/8 beat after the card. */
  leave: 6.5 + DUR.flick,
} as const;

/** The anticipation dip, px (ART.md §11: 4 px, inside MOTION.anticipate's 4–8). */
const CELL_DIP = 4;

/** Piece `c`'s flight in the open at beat `b`: 0 before it lifts, 1 once it has landed. */
export function cellFlight(c: ChefCell, b: number): number {
  const s = OPEN_CUE.fly + c.qcol * OPEN_CUE.stagger;
  return progress(s, s + DUR.cellsFly, b);
}

/** When piece `c` lands in the open, beats. */
export const cellLands = (c: ChefCell): number =>
  OPEN_CUE.fly + c.qcol * OPEN_CUE.stagger + DUR.cellsFly;

/** How far the mask reveal has come for piece `c` at beat `b`, 0 to 1 (from its landing or 3.25, whichever is later). */
export function cellReveal(c: ChefCell, b: number): number {
  return EASE.arrive(
    progress(Math.max(OPEN_CUE.reveal, cellLands(c)), OPEN_CUE.resolved, b),
  );
}

/** The whole reveal at beat `b`, 0 to 1: the chef's shadow comes in on it. */
export const openReveal = (b: number): number =>
  EASE.arrive(progress(OPEN_CUE.reveal, OPEN_CUE.resolved, b));

export interface OpenChefOptions {
  /** The help screen's cells: the pane's TermLayout.cell. */
  cell: CellAt;
  /** GLOBAL seconds, for the motes. */
  t: number;
  place?: Rect;
  /** The block glyphs' colour as captured (default the terminal's ANSI green). */
  color?: string;
  /** Screen line of logo row 0, and column of logo column 0 (CHEF_ART.block). */
  line0?: number;
  col0?: number;
  /** Rects the motes keep clear of (the name card's lines). */
  avoid?: readonly Rect[];
  /** Behind the chef's dark lines while the pane is still up (see drawChef). */
  backing?: string | null;
}

/**
 * The chef in the open at beat `b` (ART.md §11 Open): nothing before 2.5
 * (the pane prints the block chef itself); copies of its cells dip, lift
 * out of the help screen on arcs and land in the mosaic, green turning to
 * paper; the vector chef shows through a square per landed cell that
 * grows until it resolves on beat 4, where it blooms and the motes start;
 * it leaves from 6⅝. The pane's own glyphs stay where they printed: copies
 * lift, originals stay.
 */
export function drawOpenChef(
  ctx: CanvasRenderingContext2D,
  b: number,
  o: OpenChefOptions,
): void {
  if (b < OPEN_CUE.dip) return;
  const place = o.place ?? CHEF_ART.place;
  const leave = EASE.leave(
    progress(OPEN_CUE.leave, OPEN_CUE.leave + DUR.exit, b),
  );
  if (leave >= 1) return;
  ctx.save();
  ctx.globalAlpha *= 1 - leave;
  ctx.translate(0, -20 * leave);
  const resolved = b >= OPEN_CUE.resolved;
  if (b >= OPEN_CUE.reveal) {
    const { cellW, cellH } = CHEF_ART.mosaic;
    const R = CHEF_ART.reveal;
    // The mask: one rect per landed piece, in the mosaic's turn, its half
    // size growing from 0.5 to 2.2 mosaic cells (never smaller than the piece).
    const mask = (c2: CanvasRenderingContext2D) => {
      for (const c of CHEF_CELLS) {
        const k = cellReveal(c, b);
        if (k <= 0) continue;
        const p = cellInMosaic(c, place);
        const h = lerp(R.from, R.to, k);
        const hw = Math.max(p.w / 2, h * cellW);
        const hh = Math.max(p.h / 2, h * cellH);
        const cos = Math.cos(p.rot);
        const sin = Math.sin(p.rot);
        const corner = (sx: number, sy: number) => ({
          x: p.cx + sx * hw * cos - sy * hh * sin,
          y: p.cy + sx * hw * sin + sy * hh * cos,
        });
        const q = [corner(-1, -1), corner(1, -1), corner(1, 1), corner(-1, 1)];
        c2.moveTo(q[0].x, q[0].y);
        for (const v of q.slice(1)) c2.lineTo(v.x, v.y);
        c2.closePath();
      }
    };
    const bloom = resolved
      ? bloomAt(b - OPEN_CUE.resolved, {
          radius: GLOW.chefBloom.radius,
          alpha: 0,
        })
      : null;
    drawChef(ctx, place, {
      shadow: openReveal(b),
      glow: bloom,
      mask: resolved ? undefined : mask,
      backing: o.backing,
    });
    if (resolved)
      drawMotes(ctx, o.t, {
        place,
        alpha: smoothstep(OPEN_CUE.resolved, OPEN_CUE.resolved + 1, b),
        avoid: o.avoid,
      });
  }
  // The cells: dipping, flying, landed, each fading as the chef shows
  // through its own square, so none is left as a ghost over bare stage.
  if (!resolved) {
    const dip = CELL_DIP * EASE.wind(progress(OPEN_CUE.dip, OPEN_CUE.fly, b));
    for (const c of CHEF_CELLS) {
      const fade = 1 - smoothstep(0, 0.6, cellReveal(c, b));
      if (fade <= 0) continue;
      const u = cellFlight(c, b);
      const from = cellOnScreen(c, o.cell, o.line0, o.col0);
      from.cy += dip;
      const to = cellInMosaic(c, place);
      const e = EASE.travel(u);
      const at = curveAt(
        arc({ x: from.cx, y: from.cy }, { x: to.cx, y: to.cy }, MOTION.arcLift),
        e,
      );
      ctx.fillStyle = rgba(
        mix(o.color ?? TERM.ansi[2], CHEF_ART.fill, clamp(u / 0.5)),
        c.piece.alpha * fade,
      );
      fillPlaced(ctx, {
        cx: at.x,
        cy: at.y,
        w: lerp(from.w, to.w, e),
        h: lerp(from.h, to.h, e),
        rot: lerp(0, to.rot, e),
      });
    }
  }
  ctx.restore();
}

// The morph (ART.md §11 Morph), in the morph's beats.

/** Where EASE.drop first reaches 1, as a fraction of the drop (bisection: it rises monotonically to there). */
function dropContact(): number {
  let lo = 0;
  let hi = 1;
  for (let i = 0; i < 50; i++) {
    const m = (lo + hi) / 2;
    if (EASE.drop(m) < 1) lo = m;
    else hi = m;
  }
  return hi;
}

/** The morph's cues, section beats. */
export const MORPH_CUE = {
  /**
   * The bands dip before they lift (1/8 beat), then fly, 1/8 beat apart
   * (at 1/16 two of them crossed at once and the three piled up leaving
   * the card), each settling over its lobe on its own beat: tools on 2,
   * env on 2.5, tasks on 3. The flights spend the held bar landing, so
   * the hover over the bald head is never a frozen frame.
   */
  dip: 0,
  fly: DUR.anticipate,
  stagger: 2 * DUR.stagger,
  lands: [2, 2.5, 3] as readonly number[],
  /**
   * The chef below the hat comes up quickly, whole by 7/8 as before (a
   * slow fade read as a grey chef for a third of a second), but eased in
   * (glide over 3/8 beat from 1/2): on arrive's steep start it went from
   * nothing to half in a single frame.
   */
  body: 0.5,
  bodyDur: DUR.short,
  /**
   * The toque drops (1 beat, 6 % overshoot), and as it falls the vector
   * hat resolves out of its discs over 1/2 beat: drawn through circles on
   * the lobes that grow from the discs' size to cover it, while the discs
   * fade into its tint.
   */
  drop: 4,
  dropDur: 1,
  cross: 4.25,
  crossDur: DUR.half,
  /**
   * The toque touches the head: the first beat its drop reaches 1
   * (EASE.drop's overshoot starts there). The bloom starts on it, rising
   * over 1/8 beat (arrive) and settling by `still`, so the light comes on
   * with the landing, not 0.35 s after it.
   */
  land: 4 + dropContact(),
  bloom: 4 + dropContact(),
  /** The tint drains to paper, with the glide (CHEF_ART.glide). */
  drain: 5,
  drainDur: 3 / 2,
  /**
   * Still from here to the bar line: a beat and a half before the end
   * card's "mise" (PACE rule 6).
   */
  still: 13 / 2,
} as const;

/** How far above its lobe a band hovers before the toque drops, px. */
const HOVER = 60;

/**
 * How far from its nearest lobe's centre any of the hat reaches, SVG
 * units, with its outline: the resolve's circles end this big (the brim's
 * middle is 80 units from every lobe; the test holds the hat inside).
 */
const RESOLVE = 96;

/**
 * How much of the resolve the hat's body takes to come up whole, under the
 * discs (arrive): its first quarter, 1/8 beat. Through the growing circles
 * alone, the hat's lower half (the middle lobe's circle is 69 units short
 * of the brim) was a hole onto the stage for most of the resolve; wiped up
 * off the brim, the strip's ends at the ear notches read as slivers.
 */
const BODY_IN = 0.25;

/** The morph's state at beat `b`: what drawMorphChef draws, as numbers. */
export interface MorphState {
  /** Each band's flight, 0 on the card to 1 hovering over its lobe. */
  flight: [number, number, number];
  /** The anticipation dip, px (0 to MOTION.anticipate). */
  dip: number;
  /** The toque's drop, 0 hovering to 1 on the head (overshooting past 1 between). */
  drop: number;
  /** The discs' opacity (fading into the hat's tint as it resolves). */
  discs: number;
  /** The vector hat resolving out of the discs, 0 to 1: its mask's growth. */
  hat: number;
  /** The chef below the hat. */
  body: number;
  /** The lobes' tint on the vector hat. */
  tint: number;
  glow: Glow | null;
}

export function morphState(b: number): MorphState {
  const C = MORPH_CUE;
  const flight = [0, 1, 2].map((i) =>
    EASE.travel(progress(C.fly + i * C.stagger, C.lands[i], b)),
  ) as [number, number, number];
  // Rest to rest (move): arrive's steep start pops the crown in a frame.
  const cross = EASE.move(progress(C.cross, C.cross + C.crossDur, b));
  // Math.max: glide's sine starts at -0.
  const body = Math.max(0, EASE.glide(progress(C.body, C.body + C.bodyDur, b)));
  return {
    flight,
    dip: MOTION.anticipate * Math.sin(Math.PI * progress(C.dip, C.fly, b)),
    drop:
      b >= C.drop + C.dropDur
        ? 1
        : EASE.drop(progress(C.drop, C.drop + C.dropDur, b)),
    discs: 1 - cross,
    hat: cross,
    body,
    tint: 1 - EASE.glide(progress(C.drain, C.drain + C.drainDur, b)),
    glow:
      b >= C.bloom
        ? bloomAt(
            b - C.bloom,
            { radius: HALO.radius, alpha: HALO.alpha * body },
            C.still - C.bloom - DUR.flick,
          )
        : { radius: HALO.radius, alpha: HALO.alpha * body },
  };
}

export interface MorphChefOptions {
  /**
   * The three lit header bands on the compact card, px, in the card's
   * reading order (tools, env, tasks): each lifts off and becomes lobe
   * 0, 1, 2. The card stops drawing a band once it lifts (b > 0).
   */
  bands: readonly Rect[];
  /** Their headers' text, which rides the band and fades as it rounds. */
  labels?: readonly string[];
  /** GLOBAL seconds, for the motes. */
  t: number;
  place?: Rect;
  /**
   * Rects the motes keep clear of: the end card's lines (namecard.ts
   * endCardRects), so the morph's last frame is the morph|end rest the
   * end card starts from (kit/rest.ts chefRest).
   */
  avoid?: readonly Rect[];
}

/**
 * A band `k` of the way to a lobe: its rect `r`, corners rounding from
 * the band's to full, its fill mixing from the lit band's tone to the
 * pillar at 90 % with PILLAR_BRIGHT at the centre, casting SHADOW.float.
 */
export function drawLobeDisc(
  ctx: CanvasRenderingContext2D,
  r: Rect,
  pillar: Pillar,
  k: number,
  alpha = 1,
): void {
  if (alpha <= 0 || r.w <= 0 || r.h <= 0) return;
  const radius = lerp(0, Math.min(r.w, r.h) / 2, EASE.move(clamp(k)));
  ctx.save();
  ctx.globalAlpha *= alpha;
  castShadow(
    ctx,
    SHADOW.float,
    () => roundedRect(ctx, r.x, r.y, r.w, r.h, radius),
    clamp(k),
  );
  const cx = r.x + r.w / 2;
  const cy = r.y + r.h / 2;
  const g = ctx.createRadialGradient(cx, cy, 0, cx, cy, Math.max(r.w, r.h) / 2);
  const rim = mix(PILLAR_BAND[pillar], PILLAR[pillar], clamp(k));
  g.addColorStop(0, mix(PILLAR_BAND[pillar], PILLAR_BRIGHT[pillar], clamp(k)));
  g.addColorStop(1, rim);
  roundedRect(ctx, r.x, r.y, r.w, r.h, radius);
  ctx.save();
  ctx.fillStyle = g;
  ctx.globalAlpha *= lerp(1, 0.9, clamp(k));
  ctx.fill();
  ctx.restore();
  // The lit header's bar down its left edge, fading as the band rounds.
  const bar = 1 - smoothstep(0, 0.25, k);
  if (bar > 0) {
    ctx.clip();
    ctx.globalAlpha *= bar;
    ctx.fillStyle = PILLAR[pillar];
    ctx.fillRect(r.x, r.y, STROKE.headerBar, r.h);
  }
  ctx.restore();
}

/**
 * The chef in the morph at beat `b` (ART.md §11 Morph): the bands dip,
 * then fly arcs to hover 60 px over their lobes, rounding into discs; the
 * chef below the hat fades up; on beat 4 the toque drops onto the head and
 * its discs cross-fade into the vector hat, tinted in the pillars; the
 * bloom peaks as it lands and the tint drains to paper by beat 6.5. From
 * beat 6.5 the frame is the chef at rest (drawChefRest), the morph|end bar
 * line's.
 */
export function drawMorphChef(
  ctx: CanvasRenderingContext2D,
  b: number,
  o: MorphChefOptions,
): void {
  const place = o.place ?? CHEF_ART.place;
  const s = morphState(b);
  if (b >= MORPH_CUE.still) {
    drawChefRest(ctx, o.t, place, { avoid: o.avoid });
    return;
  }
  const hover = HOVER * (1 - s.drop);
  const body: ChefOptions = {
    part: "body",
    alpha: s.body,
    shadow: s.body,
    glow: s.glow,
  };
  if (s.body > 0) drawChef(ctx, place, { ...body, layer: "shadow" });
  let hat: ChefOptions | null = null;
  // The hat through circles on its lobes, from the discs' radius to the
  // reach of the hat's farthest corner (RESOLVE units), so the lobes
  // resolve out of the discs, while the whole hat comes up under them over
  // the resolve's first quarter (BODY_IN): no hole onto the stage and no
  // sliver of it ever stands alone.
  let lobes: ((c: CanvasRenderingContext2D) => void) | undefined;
  let under = 1;
  if (s.hat > 0) {
    const k = s.hat;
    const sc = chefScale(place);
    under = EASE.arrive(clamp(k / BODY_IN));
    lobes =
      k < 1
        ? (c: CanvasRenderingContext2D) =>
            [0, 1, 2].forEach((i) => {
              const l = lobeAt(i, place);
              const r = lerp(l.r, RESOLVE * sc, k);
              c.moveTo(l.x + r, l.y);
              c.arc(l.x, l.y, r, 0, TAU);
            })
        : undefined;
    hat = { part: "hat", hatDy: -hover, tint: s.tint, shadow: k };
    drawChef(ctx, place, { ...hat, layer: "shadow" });
  }
  if (s.body > 0) {
    drawChef(ctx, place, { ...body, layer: "fill" });
    drawMotes(ctx, o.t, { place, alpha: s.body, avoid: o.avoid });
  }
  if (hat) {
    if (lobes && under < 1)
      drawChef(ctx, place, { ...hat, layer: "fill", mask: lobes });
    drawChef(ctx, place, { ...hat, layer: "fill", alpha: under });
  }
  if (s.discs > 0 && b > 0)
    o.bands.slice(0, 3).forEach((band, i) => {
      const k = s.flight[i];
      const lobe = lobeAt(i, place);
      const a = { x: band.x + band.w / 2, y: band.y + band.h / 2 + s.dip };
      const to = { x: lobe.x, y: lobe.y - hover };
      const at = k > 0 ? curveAt(arc(a, to, MOTION.arcLift), k) : a;
      // It gathers into a disc early in its flight (its width eased out),
      // so the three do not cross as wide bars.
      const w = lerp(band.w, 2 * lobe.r, EASE.arrive(k));
      const h = lerp(band.h, 2 * lobe.r, k);
      drawLobeDisc(
        ctx,
        { x: at.x - w / 2, y: at.y - h / 2, w, h },
        lobePillar(i),
        k,
        s.discs,
      );
      const label = o.labels?.[i];
      const la = 1 - smoothstep(0, 0.35, k);
      if (label && la > 0) {
        ctx.save();
        ctx.globalAlpha *= la * s.discs;
        // Where the card sets it (ART.md §7): 16 px inside the band (the
        // card's 28 px inset less the band's 12), its em box centred on the row.
        const x = at.x - w / 2 + CARD_ART.inset - CARD_ART.bandInset;
        drawText(ctx, label, x, at.y + 0.3 * TYPE.card.size, {
          font: font(TYPE.card.size, TYPE.card.header, MONO),
          fill: PILLAR[lobePillar(i)],
        });
        ctx.restore();
      }
    });
}

/**
 * The chef at rest, as every bar line that keeps it holds it (morph|end,
 * and the end card after its keyed moments): at `place`, whole, with the
 * halo behind and the motes at GLOBAL time `t`.
 */
export function drawChefRest(
  ctx: CanvasRenderingContext2D,
  t: number,
  place: Rect = CHEF_ART.place,
  o: { glow?: Glow; glint?: number; avoid?: readonly Rect[] } = {},
): void {
  drawChef(ctx, place, { glow: o.glow ?? HALO, glint: o.glint });
  drawMotes(ctx, t, { place, avoid: o.avoid });
}

/** Seconds of the glint's sweep and the sparkle either side of its end (DUR.glint; hk's ±1/4 beat). */
export const GLINT = {
  dur: beats(DUR.glint),
  sparkle: beats(DUR.tick),
} as const;
