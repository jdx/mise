import * as fs from "node:fs";
import { load } from "js-toml";

type Registry = {
  tools: Record<
    string,
    {
      aliases?: string[];
      backends: (
        | string
        | {
            full: string;
            platforms?: string[];
          }
      )[];
      os?: string[];
    }
  >;
};

type Tool = {
  short: string;
  backends: { name: string; url: string }[];
  aliases: string[];
  os: string[];
};

export default {
  watch: ["./registry.toml"],
  load() {
    const raw = fs.readFileSync("./registry.toml", "utf-8");
    const { tools } = load(raw) as Registry;
    const registry: Record<string, Tool> = {};

    for (const key in tools) {
      const tool = tools[key];

      registry[key] = {
        short: key,
        backends: tool.backends.map((backend) => {
          let name = typeof backend === "string" ? backend : backend.full;
          // replace selector square brackets
          name = name.replace(/(.*?)\[.*\]/g, "$1");
          const parts = name.split(":", 2);
          const prefix = parts.at(0) ?? "";
          const slug = parts.at(1) ?? "";
          const repoName = slug.split("/").slice(0, 1).join("/");
          const urlMap: { [key: string]: string } = {
            core: `https://mise.jdx.dev/lang/${slug}.html`,
            asdf: slug.startsWith("http") ? slug : `https://github.com/${slug}`,
            aqua: `https://github.com/${repoName}`,
            cargo: `https://crates.io/crates/${slug}`,
            go: `https://pkg.go.dev/${slug}`,
            pipx: `https://pypi.org/project/${slug}`,
            npm: `https://www.npmjs.com/package/${slug}`,
          };
          const url = urlMap[prefix] ?? `https://github.com/${slug}`;
          return {
            name,
            url,
          };
        }),
        aliases: tool.aliases ?? [],
        os: tool.os ?? [],
      };
    }

    return Object.values(registry).sort((a, b) =>
      a.short.localeCompare(b.short, "en"),
    );
  },
};
