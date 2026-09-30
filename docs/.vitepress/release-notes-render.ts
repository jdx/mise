// Renders release notes to HTML with the site's own markdown settings, so a
// code block in a release looks like one anywhere else in the docs. The Releases
// page loads the result one release at a time (see config.ts, which writes a
// file per release at build time and serves them in dev).
import { createMarkdownRenderer, type SiteConfig } from "vitepress";
import { releaseNotes } from "./release-notes.mjs";

export type RenderedNotes = { title: string; html: string };

let renderer: ReturnType<typeof createMarkdownRenderer> | undefined;

function markdownFor(siteConfig: SiteConfig) {
  // Release bodies are pull request titles and generated prose, so raw HTML in
  // them is shown as text rather than passed through.
  renderer ??= createMarkdownRenderer(
    siteConfig.srcDir,
    { ...siteConfig.markdown, html: false },
    siteConfig.site.base,
    siteConfig.logger,
  );
  return renderer;
}

// GitHub's heading ids: lowercase, punctuation dropped, spaces to hyphens. The
// notes are written for GitHub, so their in-page links use these (they differ
// from the site's own, which turn the dots in a version into hyphens).
function githubSlug(html: string) {
  return html
    .replace(/<[^>]*>/g, "")
    .replace(
      /&(amp|lt|gt|quot|#39);/g,
      (_, e) =>
        ({ amp: "&", lt: "<", gt: ">", quot: '"', "#39": "'" })[e as string]!,
    )
    .trim()
    .toLowerCase()
    .replace(/[^\p{L}\p{N}\s_-]/gu, "")
    .replace(/\s/g, "-");
}

function render(
  md: Awaited<typeof renderer>,
  version: string,
  markdown: string,
) {
  // Many releases share headings ("Added", "Fixed"), and several can be open on
  // the page at once, so a heading's id is prefixed with its release and links
  // within the notes are pointed at the prefixed id.
  const prefix = `notes-${version}-`;
  const seen = new Map<string, number>();
  return md!
    .render(markdown)
    .replace(/<a class="header-anchor"[^>]*>.*?<\/a>/g, "")
    .replace(
      /<(h[1-6]) id="[^"]*"([^>]*)>([\s\S]*?)<\/\1>/g,
      (_, tag, attrs, inner) => {
        const slug = githubSlug(inner);
        const n = seen.get(slug) ?? 0;
        seen.set(slug, n + 1);
        return `<${tag} id="${prefix}${n ? `${slug}-${n}` : slug}"${attrs}>${inner}</${tag}>`;
      },
    )
    .replace(/ href="#([^"]*)"/g, ` href="#${prefix}$1"`);
}

/** One release's rendered notes, or null when it has none. */
export async function renderReleaseNotes(
  siteConfig: SiteConfig,
  version: string,
): Promise<RenderedNotes | null> {
  const notes = (await releaseNotes()).get(version);
  if (!notes) return null;
  const md = await markdownFor(siteConfig);
  return { title: notes.title, html: render(md, version, notes.markdown) };
}

/** Every release's rendered notes, as Map<version, RenderedNotes>. */
export async function renderAllReleaseNotes(siteConfig: SiteConfig) {
  const md = await markdownFor(siteConfig);
  const out = new Map<string, RenderedNotes>();
  for (const [version, notes] of await releaseNotes()) {
    out.set(version, {
      title: notes.title,
      html: render(md, version, notes.markdown),
    });
  }
  return out;
}
