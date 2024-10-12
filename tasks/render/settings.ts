import * as fs from "node:fs";
import * as child_process from "node:child_process";
import * as toml from "toml";
import * as Handlebars from "handlebars";
import { match, P } from "ts-pattern";

const doc = toml.parse(fs.readFileSync("settings.toml", "utf-8"));
const settings = {};

type Element = {
  default: string;
  description: string;
  deprecated: boolean;
  type: string;
  enum?: string[];
  items?: {
    type: string;
  };
};

function buildElement(key, props): Element {
  let type = props.type;
  if (type.startsWith("Option<")) {
    type = type.slice(7, -1);
  }
  type = type.replaceAll("PathBuf", "String");
  type = match(type)
    .with("String", () => "string")
    .with("Path", () => "string")
    .with("Url", () => "string")
    .with("Duration", () => "string")
    .with("Bool", () => "boolean")
    .with("Integer", () => "number")
    .with("ListString", () => "string[]")
    .with("ListPath", () => "string[]")
    .otherwise(() => {
      throw new Error(`Unknown type: ${type}`);
    });
  if (!props.description) {
    console.error(`Missing description for ${key}`);
    process.exit(1);
  }
  const ele: Element = {
    default: props.default,
    description: props.description,
    deprecated: props.deprecated,
    type,
  };
  if (props.enum) {
    ele.enum = props.enum.map((e) => e[0]);
  }
  if (type === "string[]") {
    ele.type = "array";
    ele.items = {
      type: "string",
    };
  }
  return ele;
}

for (const key in doc) {
  const props = doc[key];
  if (props.type) {
    settings[key] = buildElement(key, props);
  } else {
    for (const subkey in props) {
      settings[key] = settings[key] || {
        additionalProperties: false,
        description: props.description,
        properties: {},
      };
      settings[key].properties[subkey] = buildElement(
        `${key}.${subkey}`,
        props[subkey],
      );
    }
  }
}

const schema_tmpl = Handlebars.compile(
  fs.readFileSync("schema/mise.json.hbs", "utf-8"),
);
fs.writeFileSync(
  "schema/mise.json.tmp",
  schema_tmpl({
    settings_json: new Handlebars.SafeString(JSON.stringify(settings, null, 2)),
  }),
);

child_process.execSync("jq . < schema/mise.json.tmp > schema/mise.json");
child_process.execSync("prettier --write schema/mise.json");
fs.unlinkSync("schema/mise.json.tmp");
