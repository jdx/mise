import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { Resvg } from "@resvg/resvg-js";

const fontFile = fileURLToPath(
  new URL("./fonts/SpaceGrotesk.ttf", import.meta.url),
);
const logo = readFileSync(
  new URL("../public/logo-dark.svg", import.meta.url),
  "utf8",
);
const escapeXml = (value) =>
  String(value).replace(
    /[<>&"']/g,
    (c) =>
      ({
        "<": "&lt;",
        ">": "&gt;",
        "&": "&amp;",
        '"': "&quot;",
        "'": "&apos;",
      })[c],
  );

const fontOptions = { fontFiles: [fontFile], loadSystemFonts: false };
const widths = new Map();
export function textWidth(text, size) {
  const key = `${size}:${text}`;
  if (!widths.has(key)) {
    const measure = new Resvg(
      `<svg xmlns="http://www.w3.org/2000/svg" width="10000" height="200"><text y="100" font-family="Space Grotesk" font-size="${size}">${escapeXml(text)}</text></svg>`,
      { font: fontOptions },
    );
    widths.set(key, measure.innerBBox()?.width ?? 0);
  }
  return widths.get(key);
}

// Measure with the same bundled font used to render, including long CLI names.
export function wrapTitle(title, size = 62, width = 720) {
  const words = String(title)
    .trim()
    .split(/\s+/)
    .flatMap((word) => {
      const chunks = [];
      let chunk = "";
      for (const letter of word) {
        if (chunk && textWidth(chunk + letter, size) > width) {
          chunks.push(chunk);
          chunk = "";
        }
        chunk += letter;
      }
      if (chunk) chunks.push(chunk);
      return chunks;
    });
  const lines = [];
  let line = "";
  for (const word of words) {
    if (line && textWidth(`${line} ${word}`, size) > width) {
      lines.push(line);
      line = word;
    } else line = line ? `${line} ${word}` : word;
  }
  if (line) lines.push(line);
  return lines;
}

export function socialCard(title) {
  let size = 62;
  let lines = wrapTitle(title, size);
  if (lines.length > 4) {
    size = 54;
    lines = wrapTitle(title, size);
  }
  if (lines.length > 4) {
    lines = lines.slice(0, 4);
    while (textWidth(`${lines[3]}…`, size) > 720)
      lines[3] = Array.from(lines[3]).slice(0, -1).join("");
    lines[3] += "…";
  }
  const start = 298 - ((lines.length - 1) * (size + 14)) / 2;
  const heading = lines
    .map(
      (line, i) =>
        `<text x="64" y="${start + i * (size + 14)}" font-size="${size}" fill="#f4eee3">${escapeXml(line)}</text>`,
    )
    .join("");
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="1200" height="630" viewBox="0 0 1200 630">
    <rect width="1200" height="630" fill="#171417"/>
    <rect width="1200" height="8" fill="#c75b7a"/>
    <g font-family="Space Grotesk">
      <text x="64" y="85" font-size="30" fill="#c75b7a">mise / docs</text>
      ${heading}
      <path d="M64 508 H1136" stroke="#3d3540"/>
      <text x="64" y="564" font-size="26" fill="#c2b6a4">mise.jdx.dev</text>
      <text x="1136" y="564" text-anchor="end" font-size="26" fill="#c75b7a">mise</text>
    </g>
    <image x="855" y="145" width="280" height="280" xlink:href="data:image/svg+xml;base64,${Buffer.from(logo).toString("base64")}"/>
  </svg>`;
  const hash = createHash("sha256")
    .update(svg)
    .update(readFileSync(fontFile))
    .digest("hex")
    .slice(0, 16);
  return { svg, path: `social/${hash}.png` };
}

export function writeSocialCard(outDir, card) {
  const image = new Resvg(card.svg, { font: fontOptions }).render().asPng();
  mkdirSync(resolve(outDir, "social"), { recursive: true });
  writeFileSync(resolve(outDir, card.path), image);
}
