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
            options?: Record<string, string>;
          }
      )[];
      os?: string[];
      url?: string;
    }
  >;
};

type Tool = {
  short: string;
  url: string;
  backends: { name: string; url: string }[];
  aliases: string[];
  os: string[];
};

export default {
  watch: ["../registry/*.toml"],
  load() {
    const registryDir = "./registry";
    const files = fs
      .readdirSync(registryDir)
      .filter((f) => f.endsWith(".toml"))
      .sort();

    const tools: Registry["tools"] = {};
    for (const file of files) {
      const toolName = file.replace(/\.toml$/, "");
      const raw = fs.readFileSync(`${registryDir}/${file}`, "utf-8");
      const toolInfo = load(raw) as Registry["tools"][string];
      tools[toolName] = toolInfo;
    }

    const registry: Record<string, Tool> = {};

    const urlBuilders: Record<
      string,
      (slug: string, options: Record<string, string>) => string
    > = {
      aqua: (slug) => {
        const repoName = slug.split("/").slice(0, 2).join("/");
        return `https://github.com/${repoName}`;
      },
      asdf: (slug) =>
        slug.startsWith("http") ? slug : `https://github.com/${slug}`,
      conda: (slug, options) =>
        `https://anaconda.org/${options.channel ?? "conda-forge"}/${slug}`,
      cargo: (slug) => `https://crates.io/crates/${slug}`,
      core: (slug) => `https://mise.jdx.dev/lang/${slug}.html`,
      dotnet: (slug) => `https://www.nuget.org/packages/${slug}`,
      gem: (slug) => `https://rubygems.org/gems/${slug}`,
      github: (slug) => `https://github.com/${slug}`,
      gitlab: (slug) => `https://gitlab.com/${slug}`,
      go: (slug) => `https://pkg.go.dev/${slug}`,
      npm: (slug) => `https://www.npmjs.com/package/${slug}`,
      pipx: (slug) => `https://pypi.org/project/${slug}`,
      spm: (slug, options) =>
        slug.startsWith("http")
          ? slug
          : `https://${options.provider == "gitlab" ? "gitlab.com" : "github.com"}/${slug}`,
      http: () => "",
      ubi: (slug, options) => {
        const repoName = slug.split("/").slice(0, 2).join("/");
        return `https://${
          options.provider === "gitlab" ? "gitlab.com" : "github.com"
        }/${repoName}`;
      },
      vfox: (slug) => `https://github.com/${slug}`,
    };

    const nameRegex = /^(?<prefix>.+?):(?<slug>.+?)(?:\[(?<options>.+)\])?$/;

    for (const key in tools) {
      const tool = tools[key];

      const backends = tool.backends.map((backend) => {
        const name = typeof backend === "string" ? backend : backend.full;
        const match = name.match(nameRegex);
        const prefix = match?.groups?.prefix ?? "";
        const slug = match?.groups?.slug ?? "";
        const options = {
          ...(typeof backend === "object" && backend.options
            ? backend.options
            : {}),
          ...(match?.groups?.options
            ? Object.fromEntries(
                match.groups.options.split(",").map((opt) => {
                  const [k, v] = opt.split("=");
                  return [k, v];
                }),
              )
            : {}),
        };
        return {
          name: `${prefix}:${slug}`,
          url: urlBuilders[prefix] ? urlBuilders[prefix](slug, options) : "",
        };
      });

      registry[key] = {
        short: key,
        // Prefer the registry's `url`; the backend URLs are guesses from the backend
        // slug, and some backends (such as http) have none. Only http(s) links are
        // rendered, matching the check build.rs applies to `url`.
        url:
          [tool.url, ...backends.map((backend) => backend.url)].find((url) =>
            /^https?:\/\/[^{}\s]+$/.test(url ?? ""),
          ) ?? "",
        backends,
        aliases: tool.aliases ?? [],
        os: tool.os ?? [],
      };
    }

    return Object.values(registry).sort((a, b) =>
      a.short.localeCompare(b.short, "en"),
    );
  },
};
