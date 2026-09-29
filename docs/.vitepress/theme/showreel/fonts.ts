// The faces the reel draws with, bundled in docs/.vitepress/fonts (OFL, see
// its README) and loaded with FontFace before the first frame by the video
// renderer, the frame previewer and the Chromium tests, so every render sets
// the same glyphs whatever the machine has installed. Pure data: the
// renderers read it in Node.

export interface FontFile {
  /** In docs/.vitepress/fonts. */
  file: string;
  family: string;
  /** A FontFace weight descriptor: one weight, or a variable face's range. */
  weight: string;
  style: "normal" | "italic";
}

export const FONTS: readonly FontFile[] = [
  // Captions and everything set in type (type.ts DISPLAY).
  {
    file: "SpaceGrotesk.ttf",
    family: "Space Grotesk",
    weight: "300 700",
    style: "normal",
  },
  // Terminals, code and hashes (type.ts MONO).
  {
    file: "JetBrainsMono.ttf",
    family: "JetBrains Mono",
    weight: "100 800",
    style: "normal",
  },
  // Tickets, act titles and the name card (type.ts serifItalic). The reel
  // sets Cormorant only in italic, so the upright face is not bundled.
  {
    file: "CormorantGaramond-Italic.ttf",
    family: "Cormorant Garamond",
    weight: "300 700",
    style: "italic",
  },
];
