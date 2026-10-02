// The landing-page films: an editorial cut of the recorded scenes, with
// new, concise brand cards and large callouts drawn from those same takes.
import type { ReelFacts } from "./bible";
import { capture, fileOf, frameAt, reelData, rowText } from "./captures";
import type { Reel } from "./compose";
import { cutAt, cuts, filmChapters, filmDuration, type Edition } from "./edit";
import { clipTime } from "./kit/grey";
import { createReel } from "./reel";
import { pacing as pitchPacing } from "./scenes/pitch";
import { pacing as switchPacing } from "./scenes/switch";
import { sec, W, H } from "./bible";
import { DISPLAY, font, MONO } from "./type";

const COLORS = {
  bg: "#171417",
  paper: "#f4eee3",
  muted: "#c2b6a4",
  tools: "#ed9fbc",
  env: "#e1bd87",
  tasks: "#adce9c",
  panel: "#211d21",
};
const fade = (t: number, at = 0, duration = 0.35) =>
  Math.max(0, Math.min(1, (t - at) / duration));

function text(
  ctx: CanvasRenderingContext2D,
  value: string,
  x: number,
  y: number,
  size: number,
  color: string = COLORS.paper,
  mono = false,
  width = 1600,
) {
  ctx.fillStyle = color;
  ctx.font = font(size, mono ? 500 : 600, mono ? MONO : DISPLAY);
  // Long captured strings keep their text and fit inside the safe area.
  const measured = ctx.measureText(value).width;
  if (measured > width)
    ctx.font = font(
      (size * width) / measured,
      mono ? 500 : 600,
      mono ? MONO : DISPLAY,
    );
  ctx.fillText(value, x, y);
}

function stage(ctx: CanvasRenderingContext2D) {
  ctx.fillStyle = COLORS.bg;
  ctx.fillRect(0, 0, W, H);
  ctx.strokeStyle = "#41353d";
  ctx.lineWidth = 2;
  ctx.beginPath();
  ctx.moveTo(120, 155);
  ctx.lineTo(1800, 155);
  ctx.stroke();
  text(ctx, "mise", 120, 112, 58);
  text(ctx, "mise.jdx.dev", 1370, 112, 36, COLORS.muted);
}

function intro(
  ctx: CanvasRenderingContext2D,
  t: number,
  facts: ReelFacts | null,
) {
  stage(ctx);
  const lines = ["Dev tools.", "Environments.", "Tasks."];
  const colors = [COLORS.tools, COLORS.env, COLORS.tasks];
  lines.forEach((line, i) => {
    ctx.save();
    const a = fade(t, i * 0.35);
    ctx.globalAlpha = a;
    text(ctx, line, 120, 360 + i * 146 + 18 * (1 - a), 126, colors[i]);
    ctx.restore();
  });
  ctx.save();
  ctx.globalAlpha = fade(t, 1.1);
  ctx.fillStyle = COLORS.panel;
  ctx.fillRect(1110, 250, 690, 580);
  text(ctx, "mise.toml", 1160, 330, 48, COLORS.paper, true, 590);
  const file = fileOf(reelData(facts), "C2", "work/api/mise.toml") ?? "";
  const node = file.split("\n").find((line) => line.startsWith("node ="));
  const env = file.split("\n").find((line) => line.startsWith("APP_ENV ="));
  [
    "[tools]",
    node ?? "",
    "[env]",
    env ?? "",
    "[tasks.ci]",
    "depends = [ … ]",
  ].forEach((line, i) =>
    text(
      ctx,
      line,
      1160,
      426 + i * 64,
      40,
      colors[Math.floor(i / 2)],
      true,
      590,
    ),
  );
  ctx.restore();
  ctx.save();
  ctx.globalAlpha = fade(t, 2);
  text(ctx, "One configuration per project.", 120, 958, 64);
  ctx.restore();
}

function outro(ctx: CanvasRenderingContext2D, t: number) {
  stage(ctx);
  ctx.save();
  ctx.globalAlpha = fade(t);
  text(ctx, "Install mise.", 120, 380, 160);
  ctx.restore();
  ctx.save();
  ctx.globalAlpha = fade(t, 0.4);
  ctx.fillStyle = COLORS.panel;
  ctx.fillRect(120, 464, 1680, 160);
  text(
    ctx,
    "$ curl https://mise.run | sh",
    168,
    568,
    78,
    COLORS.paper,
    true,
    1570,
  );
  ctx.restore();
  ctx.save();
  ctx.globalAlpha = fade(t, 0.9);
  text(ctx, "macOS & Linux", 120, 728, 48, COLORS.muted);
  text(
    ctx,
    "Windows: winget install jdx.mise",
    120,
    812,
    48,
    COLORS.muted,
    true,
  );
  text(ctx, "mise.jdx.dev/getting-started", 120, 966, 64, COLORS.tools);
  ctx.restore();
}

/** A recorded command and its short result, at the scene's exact take time. */
export function commandCallout(
  id: string,
  localTime: number,
  facts: ReelFacts | null,
): string | null {
  const plan =
    id === "switch"
      ? switchPacing(facts)
      : id === "pitch"
        ? pitchPacing(facts)
        : null;
  if (!plan) return null;
  const d = reelData(facts);
  const captureId = id === "switch" ? "C7" : "C2";
  const take = capture(d, captureId);
  const plays = plan.takes[captureId];
  if (!take || !plays?.length || localTime < plays[0].at) return null;
  const { ct } = clipTime(plays, localTime);
  const frame = take.frames[frameAt(take, ct)];
  const rows = frame.rows.map((r) => rowText(take, r).trimEnd());
  for (let i = rows.length - 1; i >= 0; i--) {
    const command = rows[i].match(
      /^.*\$ (node --version|echo APP_ENV=\$APP_ENV)$/,
    )?.[1];
    if (!command) continue;
    const result = rows[i + 1]?.trim();
    // No partial commands, elapsed times, install summaries or invented output.
    if (result && /^(v\S+|APP_ENV=\S+)$/.test(result))
      return `${command}  →  ${result}`;
  }
  return null;
}

export function createFilm(
  facts: ReelFacts | null,
  edition: Edition = "tour",
  options = {},
): Reel {
  const source = createReel(facts, options);
  const list = cuts(edition);
  return {
    duration: filmDuration(edition),
    chapters: filmChapters(edition),
    render(ctx, time, pw, ph) {
      const cut = cutAt(edition, time);
      const local = Math.max(
        0,
        Math.min(cut.end - cut.start - 1e-6, time - cut.start),
      );
      const sourceTime = cut.from + local;
      if (!cut.brand) source.render(ctx, sourceTime, pw, ph);
      ctx.save();
      ctx.setTransform(pw / W, 0, 0, ph / H, 0, 0);
      if (cut.brand === "intro") intro(ctx, local, facts);
      else if (cut.brand === "outro") outro(ctx, local);
      else {
        const callout = commandCallout(
          cut.id,
          sourceTime - sec(cut.id).start,
          facts,
        );
        if (callout) text(ctx, callout, 160, 92, 64, COLORS.paper, true);
      }
      const index = list.findIndex((c) => c.start === cut.start);
      const previous = list[index - 1];
      const next = list[index + 1];
      // Continuous source neighbours keep their carefully matched handoff.
      // Editorial jumps dip briefly, concealing inherited source-stage props.
      const incoming =
        previous && previous.to !== cut.from ? 1 - fade(local, 0, 0.18) : 0;
      const outgoing =
        next && next.from !== cut.to ? 1 - fade(cut.end - time, 0, 0.18) : 0;
      const alpha = Math.max(incoming, outgoing);
      if (alpha > 0) {
        ctx.globalAlpha = alpha;
        ctx.fillStyle = COLORS.bg;
        ctx.fillRect(0, 0, W, H);
      }
      ctx.restore();
      ctx.setTransform(1, 0, 0, 1, 0, 0);
    },
  };
}

/** Poster composed for small displays, rather than a dense terminal frame. */
export function drawFilmPoster(
  ctx: CanvasRenderingContext2D,
  pw: number,
  ph: number,
  facts: ReelFacts | null,
) {
  ctx.save();
  ctx.setTransform(pw / W, 0, 0, ph / H, 0, 0);
  stage(ctx);
  // Leave the centre clear for the player's play glyph, including its
  // larger relative size on phones.
  text(ctx, "Switch between projects.", 120, 360, 126);
  const d = reelData(facts);
  const file = (path: string) => fileOf(d, "C7", path) ?? "";
  const node = (path: string) =>
    file(path)
      .split("\n")
      .find((line) => line.startsWith("node =")) ?? "";
  ["api", "dashboard"].forEach((name, i) => {
    const x = 120 + i * 860;
    ctx.fillStyle = COLORS.panel;
    ctx.fillRect(x, 640, 820, 245);
    text(ctx, name, x + 44, 712, 56, COLORS.muted, true);
    text(
      ctx,
      node(`work/${name}/mise.toml`),
      x + 44,
      820,
      82,
      COLORS.tools,
      true,
      732,
    );
  });
  text(ctx, "Dev tools · environments · tasks", 120, 967, 64);
  ctx.restore();
}
