import { parse } from "node-html-parser";
import TurndownService from "turndown";
import { gfm } from "turndown-plugin-gfm";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import type { PageData } from "vitepress";

type CollectedPage = {
  relativePath: string;
  title: string;
  markdown: string;
};

const pages = new Map<string, CollectedPage>();

const turndown = createTurndownService();

function createTurndownService(): TurndownService {
  const td = new TurndownService({
    headingStyle: "atx",
    codeBlockStyle: "fenced",
    bulletListMarker: "-",
  });
  td.use(gfm);

  td.addRule("headerAnchor", {
    filter: (node) =>
      node.nodeName === "A" &&
      /\bheader-anchor\b/.test(node.getAttribute("class") || ""),
    replacement: () => "",
  });

  td.addRule("codeMeta", {
    filter: (node) => {
      const cls = node.getAttribute("class") || "";
      return (
        (node.nodeName === "BUTTON" && cls.includes("copy")) ||
        (node.nodeName === "SPAN" && /\blang\b/.test(cls))
      );
    },
    replacement: () => "",
  });

  td.addRule("tabButtons", {
    filter: (node) =>
      /\bplugin-tabs--tab-list\b/.test(node.getAttribute("class") || ""),
    replacement: () => "",
  });

  td.addRule("fencedCodeBlock", {
    filter: (node) =>
      node.nodeName === "PRE" &&
      !!node.firstChild &&
      node.firstChild.nodeName === "CODE",
    replacement: (_content, node) => {
      const parentClass =
        (node.parentNode as HTMLElement)?.getAttribute?.("class") || "";
      const lang = parentClass.match(/\blanguage-([\w-]+)\b/)?.[1] || "";
      const code = (node.firstChild as HTMLElement)?.textContent || "";
      return `\n\n\`\`\`${lang}\n${code.trimEnd()}\n\`\`\`\n\n`;
    },
  });

  return td;
}

function stripLeadingH1(markdown: string): string {
  return markdown
    .replace(/^#\s+.+\n*/, "")
    .replace(/^.+\n=+\n*/, "")
    .trimStart();
}

function stripSourceBoilerplate(source: string): string {
  return source
    .replace(/^\ufeff?---\r?\n[\s\S]*?\r?\n---\r?\n*/g, "")
    .replace(/<script\b[^>]*\bsetup\b[^>]*>[\s\S]*?<\/script>\s*/g, "");
}

function extractMarkdown(
  html: string,
  pageData: PageData,
  srcDir: string,
): string {
  let source: string;
  try {
    source = readFileSync(resolve(srcDir, pageData.relativePath), "utf-8");
  } catch {
    return "";
  }

  if (/<script\b[^>]*\bsetup\b/.test(source)) {
    const root = parse(html);
    const vpDoc = root.querySelector(".vp-doc");
    if (!vpDoc) return "";
    return turndown.turndown(vpDoc.innerHTML).trim();
  }

  return stripSourceBoilerplate(source).trim();
}

export function collectPage(
  code: string,
  _id: string,
  ctx: { pageData: PageData; siteConfig: { srcDir: string } },
): void {
  const { pageData, siteConfig } = ctx;
  if (pageData.frontmatter?.layout === "home") return;
  if (pageData.isNotFound) return;

  const markdown = extractMarkdown(code, pageData, siteConfig.srcDir);
  if (!markdown) return;

  pages.set(pageData.relativePath, {
    relativePath: pageData.relativePath,
    title: pageData.title || pageData.relativePath,
    markdown,
  });
}

export function generateLlmsTxt(siteConfig: {
  outDir: string;
  sitemap?: { hostname?: string };
}): void {
  try {
  const baseUrl = (
    siteConfig.sitemap?.hostname || "https://mise.jdx.dev"
  ).replace(/\/$/, "");

  const sorted = [...pages.values()].sort((a, b) =>
    a.relativePath.localeCompare(b.relativePath),
  );

  for (const p of sorted) {
    const filePath = resolve(siteConfig.outDir, p.relativePath);
    mkdirSync(dirname(filePath), { recursive: true });
    writeFileSync(filePath, `# ${p.title}\n\n${stripLeadingH1(p.markdown)}\n`);
  }

  const index = [
    "# mise-en-place",
    "",
    "> mise is a polyglot tool version manager that replaces tools like asdf, nvm, pyenv, rbenv, etc. It also manages environment variables and tasks.",
    "",
    "## Documentation",
    "",
    ...sorted.map((p) => `- [${p.title}](${baseUrl}/${p.relativePath})`),
    "",
  ].join("\n");

  writeFileSync(resolve(siteConfig.outDir, "llms.txt"), index);

  const full = sorted
    .map(
      (p) =>
        `# ${p.title}\n\nURL: ${baseUrl}/${p.relativePath}\n\n${stripLeadingH1(p.markdown)}`,
    )
    .join("\n\n---\n\n");

  writeFileSync(resolve(siteConfig.outDir, "llms-full.txt"), full);

  const sizeKB = (Buffer.byteLength(full) / 1024).toFixed(0);
  console.log(
    `[llms.txt] Generated ${pages.size} .md files + llms.txt + llms-full.txt (${sizeKB}KB)`,
  );
} finally {
  pages.clear();
}
}
