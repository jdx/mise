import * as fs from "node:fs";
import * as path from "node:path";
import { load } from "js-toml";
import { createMarkdownRenderer } from "vitepress";

// Render settings.toml text with the site's own markdown pipeline, so `:::`
// containers, Shiki code blocks and link handling match the rest of the docs.
// VitePress sets VITEPRESS_CONFIG while it builds or serves the site; the
// fallback only matters when this loader runs on its own.
async function markdownRenderer() {
  const config = (globalThis as any).VITEPRESS_CONFIG;
  if (config) {
    return createMarkdownRenderer(
      config.srcDir,
      config.markdown,
      config.site.base,
      config.logger,
    );
  }
  return createMarkdownRenderer(path.resolve("docs"));
}

// How a list setting's environment variable is split.
const parseEnvLabels: Record<string, string> = {
  list_by_comma: "comma-separated",
  set_by_comma: "comma-separated",
  list_by_colon: "colon-separated",
  list_by_os_path_separator:
    "separated by <code>:</code> (<code>;</code> on Windows)",
  parse_url_replacements: "a JSON object",
};

const typeLabels: Record<string, string> = {
  String: "string",
  Path: "path",
  Url: "URL",
  Duration: "duration",
  Bool: "boolean",
  Integer: "integer",
  ListString: "string[]",
  ListPath: "path[]",
  SetString: "string[]",
  "IndexMap<String, String>": "table",
  BoolOrString: "boolean | string",
};

const typeHints: Record<string, string> = {
  Duration:
    "for example <code>30s</code>, <code>10m</code>, <code>1h</code> or <code>7d</code>",
};

// Where a setting may be set, when that is narrower than everywhere. `text`
// gets the setting's environment variable, if it has one.
const restrictions = {
  global_only: {
    badge: "global only",
    title: "Ignored in project config",
    text: (env?: string) =>
      `global or system config${env ? `, or <code>${env}</code>` : ""}. Ignored, with a warning, in project config. See <a href="/configuration/settings.html#global-only-settings">global-only settings</a>.`,
  },
  rc: {
    badge: "miserc",
    title: "Set in .miserc.toml or the environment, not mise.toml",
    text: (env?: string) =>
      `a <a href="/configuration.html#miserc"><code>.miserc.toml</code></a> file${env ? ` or <code>${env}</code>` : ""}. mise reads it before config files load, so <code>[settings]</code> in <code>mise.toml</code> has no effect. See <a href="/configuration/settings.html#early-initialization">settings read before config files</a>.`,
  },
  env_only: {
    badge: "env only",
    title: "Set with the environment variable only",
    text: (env?: string) =>
      `${env ? `<code>${env}</code>` : "the environment variable"} only. mise reads it before config files load and ignores it in any config file. See <a href="/configuration/settings.html#environment-only-settings">environment-only settings</a>.`,
  },
};

const escapeHtml = (s: string) =>
  s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");

// A default as it would be written in TOML: `"30s"`, `8`, `["a", "b"]`.
function tomlLiteral(value: unknown): string {
  if (typeof value === "string") return JSON.stringify(value);
  if (Array.isArray(value)) return `[${value.map(tomlLiteral).join(", ")}]`;
  if (value && typeof value === "object") {
    const entries = Object.entries(value).map(
      ([k, v]) => `${k} = ${tomlLiteral(v)}`,
    );
    return entries.length ? `{ ${entries.join(", ")} }` : "{}";
  }
  return String(value);
}

export default {
  watch: ["./settings.toml"],
  async load() {
    const md = await markdownRenderer();
    const inline = (s: string) => md.renderInline(s, {});
    const block = (s: string) => md.render(s, {});

    const raw = fs.readFileSync("./settings.toml", "utf-8");
    const doc = load(raw) as Record<string, any>;

    function renderDefault(props: Record<string, any>): string {
      // default_docs is either prose, rendered as markdown, or a single
      // value, shown as TOML like other defaults.
      if (props.default_docs !== undefined) {
        const text = String(props.default_docs);
        if (/[`\s]/.test(text)) return inline(text);
        const literal = ["Bool", "Integer"].includes(props.type)
          ? text
          : tomlLiteral(text);
        return `<code>${escapeHtml(literal)}</code>`;
      }
      let value = props.default;
      if (value === undefined && props.type === "Bool" && !props.optional) {
        value = false;
      }
      if (value === undefined) return "unset";
      return `<code>${escapeHtml(tomlLiteral(value))}</code>`;
    }

    function buildElement(key: string, props: Record<string, any>) {
      const description: string = props.description ?? "";
      const experimental = /^\[experimental\]\s*/.test(description);
      const lead = description.replace(/^\[(experimental|deprecated)\]\s*/, "");
      const scope = (
        Object.keys(restrictions) as Array<keyof typeof restrictions>
      )
        .filter((flag) => props[flag])
        .map((flag) => restrictions[flag]);
      return {
        key,
        type: typeLabels[props.type] ?? props.type,
        typeHint: typeHints[props.type],
        default: renderDefault(props),
        description: lead ? inline(lead) : "",
        docs: props.docs ? block(props.docs) : "",
        deprecated: props.deprecated ? inline(props.deprecated) : undefined,
        experimental,
        badges: scope.map(({ badge, title }) => ({ text: badge, title })),
        scope: scope.map(({ text }) => text(props.env)),
        enum: props.enum?.map((choice: any) =>
          typeof choice === "object" && choice !== null
            ? {
                value: choice.value,
                description: choice.description
                  ? inline(choice.description)
                  : undefined,
              }
            : { value: choice },
        ),
        env: props.env,
        parseEnv: parseEnvLabels[props.parse_env],
      };
    }

    function appendSettings(group, node, keyPath: string[]) {
      for (const key in node) {
        const props = node[key];
        if (typeof props !== "object" || props === null || props.hide) {
          continue;
        }
        const settingPath = [...keyPath, key];
        if (props.type) {
          group.settings.push(buildElement(settingPath.join("."), props));
        } else {
          appendSettings(group, props, settingPath);
        }
      }
    }

    const settings = [];
    for (const key in doc) {
      const props = doc[key];
      if (props.hide) continue;
      if (props.type) {
        settings.push(buildElement(key, props));
      } else {
        const group = { key, settings: [] };
        appendSettings(group, props, [key]);
        // A group whose settings are all hidden has nothing to show.
        if (group.settings.length > 0) settings.push(group);
      }
    }
    return settings.sort((a, b) => a.key.localeCompare(b.key));
  },
};
