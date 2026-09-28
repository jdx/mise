// from jdx/hk@37937824 docs/.vitepress/theme/showreel/color.ts
import { clamp, lerp } from "./math";

export type RGB = readonly [number, number, number];

const cache = new Map<string, RGB>();

/** Parse `#rgb`, `#rrggbb`, `rgb(...)`, or `rgba(...)` (alpha is ignored). */
export function rgb(color: string): RGB {
  let c = cache.get(color);
  if (!c) {
    if (color.startsWith("rgb")) {
      const [r, g, b] = color
        .slice(color.indexOf("(") + 1, color.indexOf(")"))
        .split(",")
        .map((v) => parseFloat(v));
      c = [r || 0, g || 0, b || 0];
    } else {
      const h = color.replace("#", "");
      const n = parseInt(
        h.length === 3
          ? h
              .split("")
              .map((x) => x + x)
              .join("")
          : h,
        16,
      );
      c = [(n >> 16) & 255, (n >> 8) & 255, n & 255];
    }
    // Animated mixes produce new strings every frame; keep the cache bounded.
    if (cache.size > 2000) cache.clear();
    cache.set(color, c);
  }
  return c;
}

/** A CSS color string for `color` at opacity `a`. */
export function rgba(color: string | RGB, a = 1): string {
  const [r, g, b] = typeof color === "string" ? rgb(color) : color;
  // Fixed-point alpha: exponent forms such as 9e-7 are not valid CSS.
  const alpha = a < 1e-4 ? 0 : clamp(a).toFixed(4);
  return `rgba(${r | 0},${g | 0},${b | 0},${alpha})`;
}

export function mixRGB(a: string | RGB, b: string | RGB, t: number): RGB {
  const x = typeof a === "string" ? rgb(a) : a;
  const y = typeof b === "string" ? rgb(b) : b;
  return [lerp(x[0], y[0], t), lerp(x[1], y[1], t), lerp(x[2], y[2], t)];
}

/** Blend two colors and return a CSS string. */
export function mix(
  a: string | RGB,
  b: string | RGB,
  t: number,
  alpha = 1,
): string {
  return rgba(mixRGB(a, b, clamp(t)), alpha);
}
