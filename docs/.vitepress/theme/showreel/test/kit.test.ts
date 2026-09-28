// The shared kit's pure parts: the terminal's colours and grid, its block
// elements and chrome title, the file syntax the cards set, the motion
// vocabulary's rests, the stage's lamp, and the fonts every render loads.

import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import { BEAT, PALETTE, PILLAR, sec, TERM } from "../bible";
import { FONTS } from "../fonts";
import { lampAt } from "../fx";
import {
  anticipate,
  cascade,
  enter,
  exit,
  hop,
  slam,
  span,
  swing,
} from "../kit/motion";
import {
  CARD,
  envRuns,
  FULL,
  LEFT,
  shellRuns,
  syntaxFor,
  tomlRuns,
  yamlRuns,
} from "../kit/parts";
import {
  COLOR,
  DUR,
  LAYOUT,
  MOTION,
  PANES,
  STAGE_FX,
  SYNTAX,
} from "../kit/style";
import {
  blockRects,
  CHROME,
  cwdOf,
  isBlockGlyph,
  isVectorGlyph,
  lineText,
  PANE_FULL,
  run,
  runsOf,
  sgrRuns,
  spinnerFrame,
  SPINNER,
  styleLine,
} from "../kit/term";
import { REPO } from "./repo";

test("a plain line colours only the shell's prompt", () => {
  assert.deepEqual(styleLine("~/work/api $ mise run ci"), [
    run("~/work/api $ ", TERM.prompt),
    run("mise run ci"),
  ]);
  assert.deepEqual(styleLine("$ cd api"), [
    run("$ ", TERM.prompt),
    run("cd api"),
  ]);
  assert.deepEqual(styleLine("[ci] api ready"), [run("[ci] api ready")]);
});

test("captured SGR codes become runs in the terminal's ANSI colours", () => {
  assert.deepEqual(sgrRuns("\x1b[32m✓\x1b[0m done"), [
    run("✓", TERM.ansi[2]),
    run(" done"),
  ]);
  assert.deepEqual(sgrRuns("\x1b[1;33mwarn\x1b[22m x\x1b[39m y"), [
    run("warn", TERM.ansi[3], true),
    run(" x", TERM.ansi[3]),
    run(" y"),
  ]);
  assert.deepEqual(sgrRuns("\x1b[2mdim\x1b[0m \x1b[94mblue"), [
    run("dim", TERM.dim),
    run(" "),
    run("blue", TERM.ansi[12]),
  ]);
  // 256-colour and true-colour codes, and other escapes, are skipped without eating the next code.
  assert.deepEqual(
    sgrRuns("\x1b[38;5;208;1mA\x1b[38;2;1;2;3mB\x1b[?25l\x1b[K"),
    [run("A", TERM.text, true), run("B", TERM.text, true)],
  );
  assert.deepEqual(
    sgrRuns("\x1b]8;;https://mise.jdx.dev\x07link\x1b]8;;\x07\x1b(B!"),
    [run("link!")],
  );
  // styleLine hands escaped text to sgrRuns, and the grid sets one character per column.
  assert.equal(lineText("\x1b[35mmise\x1b[0m ok"), "mise ok");
  assert.deepEqual(runsOf("\x1b[31mERROR\x1b[0m"), [
    run("ERROR", TERM.ansi[1]),
  ]);
});

test("ANSI yellow is lemon, never the env pillar's gold", () => {
  assert.equal(TERM.ansi[3], "#f0d27a");
  assert.equal(TERM.ansi.length, 16);
});

test("the glyphs JetBrains Mono lacks are drawn, and the spinner turns on global time", () => {
  for (const ch of ["✔", "ℹ", "↳", ...SPINNER])
    assert.ok(isVectorGlyph(ch), ch);
  for (const ch of ["✓", "✗", "❯", "⚠", "a", "=", "─"])
    assert.ok(!isVectorGlyph(ch), ch);
  assert.equal(spinnerFrame(0), SPINNER[0]);
  assert.equal(spinnerFrame(0.2), SPINNER[1]);
  assert.equal(spinnerFrame(2), SPINNER[0]);
});

test("every font the reel loads is bundled, with its licence beside it", () => {
  const dir = join(REPO, "docs/.vitepress/fonts");
  const readme = readFileSync(join(dir, "README.md"), "utf8");
  for (const f of FONTS) {
    const file = join(dir, f.file);
    assert.ok(existsSync(file), `${f.file} is not bundled`);
    // TrueType: a 0x00010000 sfnt version.
    assert.equal(
      readFileSync(file).readUInt32BE(0),
      0x00010000,
      `${f.file} is not a TrueType font`,
    );
    assert.ok(
      readme.includes(`\`${f.file}\``),
      `the fonts README does not list ${f.file}`,
    );
  }
  for (const licence of [
    "OFL.txt",
    "JetBrainsMono-OFL.txt",
    "CormorantGaramond-OFL.txt",
  ]) {
    assert.match(
      readFileSync(join(dir, licence), "utf8"),
      /SIL Open Font License, Version 1\.1/,
      licence,
    );
  }
});

test("block elements fill their whole cells, quadrant by quadrant, so a block drawing has no seams", () => {
  // Every code point of U+2580–U+259F is a block, and nothing either side is.
  for (let cp = 0x2580; cp <= 0x259f; cp++) {
    const ch = String.fromCodePoint(cp);
    assert.ok(isBlockGlyph(ch) && isVectorGlyph(ch), ch);
    const b = blockRects(ch);
    assert.ok(b && b.rects.length > 0, ch);
    for (const [x, y, w, h] of b.rects) {
      assert.ok(x >= 0 && y >= 0 && w > 0 && h > 0, ch);
      assert.ok(x + w <= 1 + 1e-9 && y + h <= 1 + 1e-9, ch);
    }
  }
  assert.ok(!isBlockGlyph("\u257f") && !isBlockGlyph("\u25a0"));
  assert.equal(blockRects("a"), null);
  const area = (ch: string) =>
    blockRects(ch)!.rects.reduce((a, [, , w, h]) => a + w * h, 0);
  // The full block is the whole cell; halves, eighths and quadrants their shares.
  assert.deepEqual(blockRects("█"), { rects: [[0, 0, 1, 1]], alpha: 1 });
  assert.deepEqual(blockRects("▀")!.rects, [[0, 0, 1, 0.5]]);
  assert.deepEqual(blockRects("▄")!.rects, [[0, 0.5, 1, 0.5]]);
  assert.deepEqual(blockRects("▌")!.rects, [[0, 0, 0.5, 1]]);
  assert.deepEqual(blockRects("▐")!.rects, [[0.5, 0, 0.5, 1]]);
  assert.deepEqual(blockRects("▔")!.rects, [[0, 0, 1, 1 / 8]]);
  assert.equal(area("▁"), 1 / 8);
  assert.equal(area("▏"), 1 / 8);
  // The block chef's quadrants: three for ▙ ▛ ▜ ▟, two for ▚ ▞, one for the corners.
  for (const ch of ["▖", "▗", "▘", "▝"]) assert.equal(area(ch), 0.25, ch);
  for (const ch of ["▚", "▞"]) assert.equal(area(ch), 0.5, ch);
  for (const ch of ["▙", "▛", "▜", "▟"]) assert.equal(area(ch), 0.75, ch);
  assert.deepEqual(blockRects("▟")!.rects, [
    [0.5, 0, 0.5, 0.5],
    [0, 0.5, 0.5, 0.5],
    [0.5, 0.5, 0.5, 0.5],
  ]);
  // Shades fill the whole cell at their coverage.
  assert.equal(blockRects("░")!.alpha, 0.25);
  assert.equal(blockRects("▒")!.alpha, 0.5);
  assert.equal(blockRects("▓")!.alpha, 0.75);
});

test("a window's title is the cwd of the latest prompt on its screen", () => {
  assert.equal(
    cwdOf(["~/work $ cd api", "~/work/api $ node --version", "v1"]),
    "~/work/api",
  );
  assert.equal(cwdOf(["~ $ mise", "Usage: mise [FLAGS]"]), "~");
  assert.equal(
    cwdOf([[run("~/work/api $ ", TERM.prompt), run("")]]),
    "~/work/api",
  );
  assert.equal(cwdOf(["[ci] api ready", "$ echo hi"]), null);
  assert.equal(cwdOf([]), null);
});

test("the prompt is a neutral colour: pink means tools, and never a prompt", () => {
  assert.equal(TERM.prompt, COLOR.prompt);
  for (const c of Object.values(PILLAR)) assert.notEqual(TERM.prompt, c);
});

test("the panes and cards stand on the layout grid, with the 56 px chrome", () => {
  assert.equal(CHROME, 56);
  assert.equal(PANE_FULL, PANES.solo);
  assert.equal(FULL, PANES.solo);
  assert.equal(LEFT, PANES.side);
  assert.equal(CARD, LAYOUT.card);
  // Solo: 80 columns of 32 px in 1600 px, 11 rows; beside the card: 12 rows of 26 px.
  assert.equal(PANES.solo.rows, 11);
  assert.ok(
    PANES.solo.x0 + 80 * 0.6 * PANES.solo.size <= PANES.solo.x + PANES.solo.w,
  );
  assert.equal(PANES.side.rows, 12);
  // The terminal's column and the card's never overlap: a gutter runs between them.
  assert.ok(LEFT.x + LEFT.w < CARD.x);
  // The card fits 36 columns of 26 px inside its 28 px insets.
  assert.ok(Math.floor((CARD.w - 56) / (0.6 * 26)) >= 36);
});

test("a file's lines are set in the file syntax: values brightest, keys, then punctuation", () => {
  const text = (runs: ReturnType<typeof tomlRuns>) =>
    runs.map((r) => r.text).join("");
  const colorOf = (runs: ReturnType<typeof tomlRuns>, s: string) =>
    runs.find((r) => r.text.includes(s))?.color;
  for (const line of [
    'node = "24"',
    'depends = ["lint", "test", "build"]',
    '"npm:prettier" = "latest"',
    '_.file = ".env"',
    "# a comment",
    '  "a", # trailing',
  ]) {
    // Verbatim: the runs spell the line exactly.
    assert.equal(text(tomlRuns(line)), line, line);
  }
  const d = tomlRuns('depends = ["lint", "test"]');
  assert.equal(colorOf(d, "depends"), SYNTAX.key);
  assert.equal(colorOf(d, "lint"), SYNTAX.value);
  assert.equal(colorOf(d, "["), SYNTAX.punct);
  assert.equal(
    colorOf(tomlRuns('"npm:prettier" = "x"'), "npm:prettier"),
    SYNTAX.key,
  );
  assert.equal(SYNTAX.value, PALETTE.paper);
  const e = envRuns("DEPLOY_TOKEN=tok_demo");
  assert.equal(text(e), "DEPLOY_TOKEN=tok_demo");
  assert.equal(colorOf(e, "DEPLOY_TOKEN"), SYNTAX.key);
  assert.equal(colorOf(e, "tok_demo"), SYNTAX.value);
  for (const line of [
    "on: [push]",
    "      - uses: jdx/mise-action@v2",
    "        with:",
    "  run: mise run ci # go",
  ])
    assert.equal(text(yamlRuns(line)), line, line);
  assert.equal(
    colorOf(yamlRuns("  install_args: --locked"), "install_args"),
    SYNTAX.key,
  );
  assert.equal(
    colorOf(yamlRuns("  install_args: --locked"), "--locked"),
    SYNTAX.value,
  );
  assert.equal(text(shellRuns('#USAGE arg "<env>"')), '#USAGE arg "<env>"');
  assert.equal(
    colorOf(shellRuns("#!/usr/bin/env bash"), "bash"),
    SYNTAX.comment,
  );
  // The file card picks the syntax from the file's name.
  assert.equal(syntaxFor("~/work/api/mise.toml"), tomlRuns);
  assert.equal(syntaxFor("mise.lock"), tomlRuns);
  assert.equal(syntaxFor(".env"), envRuns);
  assert.equal(syntaxFor(".env.deploy"), envRuns);
  assert.equal(syntaxFor(".github/workflows/ci.yml"), yamlRuns);
  assert.equal(syntaxFor("~/.zshrc"), shellRuns);
  assert.equal(syntaxFor("mise-tasks/deploy"), shellRuns);
});

test("the motion vocabulary is exactly at rest outside its spans", () => {
  // Spans: exactly 0 before and 1 after, whatever the ease.
  for (const ease of [
    "arrive",
    "leave",
    "move",
    "snap",
    "drop",
    "print",
  ] as const) {
    assert.equal(span(0.999, 1, 0.5, ease), 0, ease);
    assert.equal(span(1.5, 1, 0.5, ease), 1, ease);
    assert.equal(span(9, 1, 0.5, ease), 1, ease);
  }
  // Entrances and exits end on their marks.
  assert.deepEqual(enter(2, 1), { alpha: 1, dy: 0 });
  assert.deepEqual(enter(1, 1), { alpha: 0, dy: MOTION.riseIn });
  assert.deepEqual(exit(0.5, 1), { alpha: 1, dy: 0 });
  assert.deepEqual(exit(1 + DUR.exit, 1), { alpha: 0, dy: MOTION.fallOut });
  // Anticipation is 0 off its span, and back to 0 on the move's beat.
  assert.equal(anticipate(0.5, 1), 0);
  assert.equal(anticipate(1, 1), 0);
  assert.ok(anticipate(1 - DUR.anticipate / 4, 1) > 0.5);
  // The slam: from 1.4× and transparent, landing on its beat, still 1× after the settle.
  assert.deepEqual(slam(0, 1), {
    scale: MOTION.slamFrom,
    alpha: 0,
    impact: 0,
    landed: 0,
  });
  assert.equal(slam(1, 1).impact, 1);
  assert.equal(slam(1, 1).landed, 1);
  assert.ok(slam(1 + DUR.flick / 2, 1).scale < 1);
  assert.deepEqual(slam(1 + DUR.half, 1), {
    scale: 1,
    alpha: 1,
    impact: 0,
    landed: 1,
  });
  // A swing is 0 before it starts and from its end on; a hop lands flat.
  assert.equal(swing(0.5, 0.75, 2.2, 0.35, 1.1), 0);
  assert.equal(swing(1.1, 0.75, 2.2, 0.35, 1.1), 0);
  assert.ok(Math.abs(swing(0.75, 0.75, 2.2, 0.35, 1.1)) > 2);
  assert.equal(hop(0), 0);
  assert.equal(hop(1), 0);
  assert.equal(hop(0.5), 1);
  // A cascade's items start DUR.stagger apart, capped at DUR.staggerMax overall.
  assert.equal(cascade(1 + DUR.stagger - 1e-9, 1, 1, 4, 0.5), 0);
  assert.ok(cascade(1 + DUR.stagger + 0.01, 1, 1, 4, 0.5) > 0);
  assert.ok(cascade(1 + DUR.staggerMax + 0.01, 1, 39, 40, 0.5) > 0);
});

test("the pass lamp holds over the stage until the morph's chef glides to its place, then glides with it to the end card's", () => {
  const m = sec("morph").start;
  assert.deepEqual(lampAt(0), STAGE_FX.lamp);
  assert.deepEqual(lampAt(m + 5 * BEAT - 0.01), STAGE_FX.lamp);
  assert.deepEqual(lampAt(m + 7 * BEAT), STAGE_FX.lampEnd);
  assert.deepEqual(lampAt(sec("end").end - 0.01), STAGE_FX.lampEnd);
  // CHEF_ART.glide is beats 5 to 6.5: half way at 5.75.
  const mid = lampAt(m + 5.75 * BEAT);
  assert.ok(mid.x > STAGE_FX.lamp.x && mid.x < STAGE_FX.lampEnd.x);
  // On the chef's own curve: half way across as the chef is half way.
  const k = (mid.x - STAGE_FX.lamp.x) / (STAGE_FX.lampEnd.x - STAGE_FX.lamp.x);
  assert.ok(Math.abs(k - 0.5) < 1e-9);
});
