import { socialCard, writeSocialCard } from "./social-images.mjs";
import { readdirSync, readFileSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { defineConfig } from "vitepress";
import { sidebar } from "./sidebar";
import {
  groupIconMdPlugin,
  groupIconVitePlugin,
} from "vitepress-plugin-group-icons";
import { tabsMarkdownPlugin } from "vitepress-plugin-tabs";
import { withMermaid } from "vitepress-plugin-mermaid";
import kdlGrammar from "./grammars/kdl.tmLanguage.json";
import miseTomlGrammar from "./grammars/mise-toml.tmLanguage.json";

const configDir = dirname(fileURLToPath(import.meta.url));
const cargoToml = readFileSync(resolve(configDir, "../../Cargo.toml"), "utf8");
const versionMatch = cargoToml.match(
  /^\[package\][\s\S]*?^\s*version\s*=\s*"([^"]+)"/m,
);
if (!versionMatch) {
  console.warn("Unable to find package version in Cargo.toml");
}
const latestVersion = versionMatch?.[1] ?? "0.0.0";
const siteUrl = "https://mise.jdx.dev";
// Keep in sync with --vp-c-brand-1 in theme/custom.css and theme_color in
// public/site.webmanifest so browser chrome matches the installed-app chrome.
const brandColor = "#8B2252";
const siteDescription =
  "mise manages developer tools, environment variables, tasks, packages, and dotfiles in one project configuration for macOS, Linux, and Windows.";

// `foo/index.md` publishes as `foo/`, everything else as `foo/bar.html`. Anchor
// the index match on the leading slash so `guide/myindex.md` keeps its name.
const pageUrl = (relativePath: string) =>
  `${siteUrl}/${relativePath}`
    .replace(/\/index\.md$/, "/")
    .replace(/\.md$/, ".html");

// VitePress writes an `application/ld+json` body through as raw HTML, so a page
// whose title or description contains `</script>` would otherwise break out of
// the tag. JSON allows `\u003c` anywhere `<` is legal.
const ldJson = (data: unknown) => JSON.stringify(data).replace(/</g, "\\u003c");
const publicSchemas = [
  "mise.json",
  "mise.plugin.json",
  "mise-task.json",
  "mise-settings.json",
  "mise-registry-tool.json",
];

/** Return whether VitePress emitted a documentation container with no content. */
function hasEmptyDocContainer(html: string) {
  const divPattern = /<div\b[^>]*\sclass="([^"]*)"[^>]*>/g;
  for (const match of html.matchAll(divPattern)) {
    if (!match[1].split(/\s+/).includes("vp-doc")) continue;

    let content = html.slice((match.index ?? 0) + match[0].length).trimStart();
    while (content.startsWith("<!--")) {
      const commentEnd = content.indexOf("-->");
      if (commentEnd === -1) return false;
      content = content.slice(commentEnd + 3).trimStart();
    }
    return content.startsWith("</div>");
  }
  return false;
}

/** Fail the build when server-side rendering leaves a documentation page empty. */
function assertNoEmptyDocPages(outDir: string) {
  const emptyPages: string[] = [];

  /** Recursively inspect generated HTML files beneath the output directory. */
  function visit(dir: string) {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      const path = join(dir, entry.name);
      if (entry.isDirectory()) {
        visit(path);
      } else if (entry.name.endsWith(".html")) {
        const html = readFileSync(path, "utf8");
        if (hasEmptyDocContainer(html)) {
          emptyPages.push(relative(outDir, path));
        }
      }
    }
  }

  visit(outDir);
  if (emptyPages.length > 0) {
    throw new Error(
      `generated empty documentation pages:\n${emptyPages.map((page) => `- ${page}`).join("\n")}`,
    );
  }
}

// https://vitepress.dev/reference/site-config
export default withMermaid(
  defineConfig({
    title: "mise-en-place",
    description: siteDescription,
    lang: "en-US",
    lastUpdated: true,
    appearance: true,
    mermaid: {},
    sitemap: {
      hostname: "https://mise.jdx.dev",
    },
    themeConfig: {
      // https://vitepress.dev/reference/default-theme-config
      logo: { light: "/logo-light.svg", dark: "/logo-dark.svg" },
      outline: "deep",
      nav: [
        { text: "mise-versions", link: "https://mise-versions.jdx.dev/" },
        { text: "Dev Tools", link: "/dev-tools/" },
        { text: "Environments", link: "/environments/" },
        { text: "Tasks", link: "/tasks/" },
        {
          text: `v${latestVersion}`,
          link: "https://github.com/jdx/mise/releases",
        },
      ],
      sidebar,

      socialLinks: [
        { icon: "github", link: "https://github.com/jdx/mise" },
        { icon: "discord", link: "https://discord.gg/UBa7pJUN7Z" },
      ],

      editLink: {
        pattern: "https://github.com/jdx/mise/edit/main/docs/:path",
      },
      search: {
        provider: "algolia",
        options: {
          indexName: "rtx",
          appId: "1452G4RPSJ",
          apiKey: "ad09b96a7d2a30eddc2771800da7a1cf",
          insights: true,
        },
      },
      footer: false,
      carbonAds: {
        code: "CWYIPKQN",
        placement: "misejdxdev",
      },
    },
    markdown: {
      languages: [
        // Load base languages needed for embedded support
        "toml",
        "shell",
        "bash",
        // TODO: Once Shiki bundles KDL (tracked in shikijs/textmate-grammars-themes),
        // we can import it from 'shiki/langs/kdl' instead of storing locally
        {
          ...kdlGrammar,
          name: "kdl",
          scopeName: "source.kdl",
        } as any,
        // Custom mise.toml grammar with embedded KDL (usage fields) and bash (run fields)
        {
          ...miseTomlGrammar,
          name: "mise-toml",
          aliases: ["mise.toml"],
          scopeName: "source.mise-toml",
        } as any,
      ],
      config(md) {
        md.use(groupIconMdPlugin);
        md.use(tabsMarkdownPlugin);
      },
    },
    vite: {
      build: {
        target: "es2022",
      },
      plugins: [
        {
          name: "mise-schema-assets",
          apply: "build",
          buildStart() {
            for (const filename of publicSchemas) {
              this.emitFile({
                type: "asset",
                fileName: `schema/${filename}`,
                source: readFileSync(
                  resolve(configDir, "../../schema", filename),
                ),
              });
            }
          },
        },
        groupIconVitePlugin({
          customIcon: {
            ".toml": "vscode-icons:file-type-toml",
            brew: "logos:homebrew",
            python: "logos:python",
            node: "logos:nodejs",
            ruby: "logos:ruby",
          },
        }),
      ],
    },
    head: [
      // Favicon
      ["link", { rel: "icon", href: "/favicon.ico", sizes: "any" }],
      [
        "link",
        {
          rel: "icon",
          href: "/favicon-16x16.png",
          type: "image/png",
          sizes: "16x16",
        },
      ],
      [
        "link",
        {
          rel: "icon",
          href: "/favicon-32x32.png",
          type: "image/png",
          sizes: "32x32",
        },
      ],
      ["link", { rel: "icon", href: "/favicon.svg", type: "image/svg+xml" }],
      ["link", { rel: "manifest", href: "/site.webmanifest" }],
      ["meta", { name: "theme-color", content: brandColor }],
      // Pre-paint setup to avoid first-load pop-in (see custom.css "preboot"
      // rules; Layout.vue removes the preboot classes right after hydration):
      // - `preboot` disables navbar transitions so hydration state
      //   corrections snap instead of visibly fading
      // - other pages: pre-apply the has-sidebar navbar layout so the
      //   search/menu don't jump right when hydration adds the class
      // - reserve the announcement banner's space from the cached height so
      //   the header doesn't jump when the banner arrives (banner.ts)
      [
        "script",
        {},
        `(function () {
  try {
    var d = document.documentElement;
    var p = location.pathname;
    d.classList.add("preboot");
    if (p !== "/" && p !== "/index.html") {
      d.classList.add("preboot-sidebar");
    }
    var c = JSON.parse(localStorage.getItem("jdx-banner-cache") || "null");
    var expires = c && c.expires ? Date.parse(c.expires) : NaN;
    var now = Date.now();
    var metadataValid =
      c &&
      typeof c.id === "string" &&
      typeof c.height === "string" &&
      /^[1-9]\\d*(?:\\.\\d+)?px$/.test(c.height) &&
      Number.isFinite(c.width) &&
      typeof c.fontSize === "string" &&
      Number.isFinite(c.pixelRatio) &&
      Number.isFinite(c.cachedAt) &&
      c.cachedAt <= now &&
      now - c.cachedAt < 300000 &&
      (!c.expires || (typeof c.expires === "string" && Number.isFinite(expires) && now < expires));
    var contextMatches =
      metadataValid &&
      c.width === innerWidth &&
      c.fontSize === getComputedStyle(d).fontSize &&
      c.pixelRatio === devicePixelRatio;
    if (contextMatches && localStorage.getItem("jdx-banner-dismissed") !== c.id)
      d.style.setProperty("--vp-layout-top-height", c.height);
    else if (c && !metadataValid)
      localStorage.removeItem("jdx-banner-cache");
  } catch (e) {}
})();`,
      ],
      [
        "link",
        {
          rel: "apple-touch-icon",
          href: "/apple-touch-icon.png",
          sizes: "180x180",
        },
      ],
      // Google Fonts
      [
        "link",
        {
          rel: "preconnect",
          href: "https://fonts.googleapis.com",
        },
      ],
      [
        "link",
        {
          rel: "preconnect",
          href: "https://fonts.gstatic.com",
          crossorigin: "",
        },
      ],
      [
        "link",
        {
          href: "https://fonts.googleapis.com/css2?family=Cormorant+Garamond:ital,wght@0,300;0,400;0,500;0,600;0,700;1,300;1,400&family=DM+Sans:ital,opsz,wght@0,9..40,100..1000;1,9..40,100..1000&family=JetBrains+Mono:wght@400;500;600;700&display=swap",
          rel: "stylesheet",
        },
      ],
      [
        "script",
        {
          async: "",
          src: "https://www.googletagmanager.com/gtag/js?id=G-B69G389C8T",
        },
      ],
      [
        "script",
        {},
        `window.dataLayer = window.dataLayer || [];
      function gtag(){dataLayer.push(arguments);}
      gtag('js', new Date());
      gtag('config', 'G-B69G389C8T');`,
      ],
      // Open Graph
      ["meta", { property: "og:site_name", content: "mise-en-place" }],
      ["meta", { property: "og:type", content: "website" }],
      ["meta", { property: "og:locale", content: "en_US" }],
      ["meta", { property: "og:image:width", content: "1200" }],
      ["meta", { property: "og:image:height", content: "630" }],
      ["meta", { name: "twitter:card", content: "summary_large_image" }],
      ["meta", { name: "twitter:site", content: "@jdxcode" }],
    ],
    transformHead({ pageData, title, description, siteConfig }) {
      const heading =
        pageData.relativePath === "index.md"
          ? "Dev tools, environments, and tasks"
          : pageData.title || "mise";
      const card = socialCard(heading);
      writeSocialCard(siteConfig.outDir, card);
      const image = new URL(card.path, `${siteUrl}/`).toString();
      const imageAlt = `${heading} — mise docs`;
      const url = pageUrl(pageData.relativePath);

      return [
        ["meta", { property: "og:url", content: url }],
        ["meta", { property: "og:image", content: image }],
        ["meta", { property: "og:image:alt", content: imageAlt }],
        ["meta", { name: "twitter:image", content: image }],
        ["meta", { name: "twitter:image:alt", content: imageAlt }],
        ["meta", { property: "og:title", content: title }],
        ["meta", { property: "og:description", content: description }],
        ["meta", { name: "twitter:title", content: title }],
        ["meta", { name: "twitter:description", content: description }],
        [
          "script",
          { type: "application/ld+json" },
          ldJson({
            "@context": "https://schema.org",
            "@type": "WebPage",
            name: title,
            description,
            url,
            isPartOf: {
              "@type": "WebSite",
              name: "mise-en-place",
              url: siteUrl,
            },
          }),
        ],
      ];
    },
    transformPageData(pageData) {
      const canonicalUrl = pageUrl(pageData.relativePath);

      pageData.frontmatter.head ??= [];
      pageData.frontmatter.head.push([
        "link",
        { rel: "canonical", href: canonicalUrl },
      ]);
      pageData.frontmatter.head.push([
        "link",
        {
          rel: "sitemap",
          href: "https://mise.jdx.dev/sitemap.xml",
          type: "application/xml",
          title: "Sitemap",
        },
      ]);
    },
    transformHtml(code) {
      return code.replace(
        /<script id="check-dark-mode">/,
        '<script id="check-dark-mode" data-cfasync="false">',
      );
    },
    buildEnd(siteConfig) {
      assertNoEmptyDocPages(siteConfig.outDir);
    },
  }),
);
