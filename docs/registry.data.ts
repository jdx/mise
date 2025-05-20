import * as fs from "node:fs";
import { load } from "js-toml";

type Registry = {
  [key: string]: {
    short: string;
    aliases?: string[];
    backends?: [{ name: string; url: string }?];
    os?: string[];
  };
};

type Backend = string | { full: string; platforms: string[] };

export default {
  watch: ["./registry.toml"],
  load() {
    const raw = fs.readFileSync("./registry.toml", "utf-8");
    const doc: any = load(raw);
    const registry: Registry = {};

    const tools = doc["tools"];
    for (const key in tools) {
      const tool = tools[key];
      const backends = tool.backends || [];

      registry[key] = {
        short: key,
        aliases: tool.aliases || [],
        backends: backends.map((backend: Backend) => {
          let name = typeof backend === "string" ? backend : backend.full;
          // replace selector square brackets
          name = name.replace(/(.*?)\[.*\]/g, "$1");
          const parts = name.toString().split(":");
          const prefix = parts[0];
          const slug = parts[1];
          const urlMap: { [key: string]: string } = {
            core: `https://mise.jdx.dev/lang/${slug}.html`,
            cargo: `https://crates.io/crates/${slug}`,
            go: `https://pkg.go.dev/${slug}`,
            pipx: `https://pypi.org/project/${slug}`,
            npm: `https://www.npmjs.com/package/${slug}`,
          };
          const url = urlMap[prefix] || `https://github.com/${slug}`;
          return {
            name,
            url,
          };
        }),
        os: tool.os || [],
      };
    }

    return Object.values(registry).sort((a, b) =>
      a.short.localeCompare(b.short),
    );
  },
};
