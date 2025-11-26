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

    // dotnet, gem, spm backends are not supported
    const urlBuilders: Record<
      string,
      (slug: string, options: string | undefined) => string
    > = {
      aqua: (slug) => {
        const repoName = slug.split("/").slice(0, 2).join("/");
        return `https://github.com/${repoName}`;
      },
      asdf: (slug) =>
        slug.startsWith("http") ? slug : `https://github.com/${slug}`,
      cargo: (slug) => `https://crates.io/crates/${slug}`,
      core: (slug) => `https://mise.jdx.dev/lang/${slug}.html`,
      github: (slug) => `https://github.com/${slug}`,
      go: (slug) => `https://pkg.go.dev/${slug}`,
      npm: (slug) => `https://www.npmjs.com/package/${slug}`,
      pipx: (slug) => `https://pypi.org/project/${slug}`,
      ubi: (slug, options) => {
        const provider = options
          ?.split(",")
          .filter((str) => str.startsWith("provider="))
          .at(0)
          ?.replace("provider=", "");
        const repoName = slug.split("/").slice(0, 2).join("/");
        return `https://${
          provider === "gitlab" ? "gitlab.com" : "github.com"
        }/${repoName}`;
      },
      vfox: (slug) => `https://github.com/${slug}`,
    };

    const nameRegex = /^(?<prefix>.+?):(?<slug>.+?)(?:\[(?<options>.+)\])?$/;

    for (const key in tools) {
      const tool = tools[key];

      registry[key] = {
        short: key,
        backends: tool.backends.map((backend) => {
          const name = typeof backend === "string" ? backend : backend.full;
          const match = name.match(nameRegex);
          const prefix = match?.groups?.prefix ?? "";
          const slug = match?.groups?.slug ?? "";
          return {
            name: `${prefix}:${slug}`,
            url: urlBuilders[prefix]
              ? urlBuilders[prefix](slug, match?.groups?.options ?? "")
              : "",
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
