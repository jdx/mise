#!/usr/bin/env bun

//MISE description="Render JSON schema"
//MISE depends=["docs:setup"]

import * as fs from "node:fs";
import * as child_process from "node:child_process";
import * as toml from "toml";

type Props = {
  type: string;
  description: string;
  default?: unknown;
  deprecated?: string;
  enum?: [string, ...string[]][];
};

type SettingsToml = Record<string, Props | Record<string, Props>>;

type Element = {
  type: string;
  default: unknown;
  description: string;
  deprecated?: true;
  enum?: string[];
  items?: {
    type: string;
  };
  additionalProperties?: {
    type: string;
  };
};

type NestedElement = {
  type: "object";
  additionalProperties: false;
  deprecated?: true;
  properties: Record<string, Element>;
};

function buildElement(key: string, props: Props): Element {
  const typeMap: Record<string, string> = {
    String: "string",
    Path: "string",
    Url: "string",
    Duration: "string",
    Bool: "boolean",
    Integer: "number",
    ListString: "string[]",
    ListPath: "string[]",
    SetString: "string[]",
    "IndexMap<String, String>": "object",
  };
  const type = props.type ? typeMap[props.type] : undefined;
  if (!type) {
    throw new Error(`Unknown type: ${props.type}`);
  }

  if (!props.description) {
    throw new Error(`Missing description for ${key}`);
  }

  const element: Element = {
    default: props.default,
    description: props.description,
    type,
  };

  if (props.deprecated) {
    element.deprecated = true;
  }
  if (props.enum) {
    element.enum = props.enum.map((e) => e[0]);
  }

  if (type === "string[]") {
    element.type = "array";
    element.items = {
      type: "string",
    };
  }

  if (type === "object") {
    element.additionalProperties = {
      type: "string",
    };
  }

  return element;
}

const doc = toml.parse(
  fs.readFileSync("settings.toml", "utf-8"),
) as SettingsToml;
const settings: Record<string, Element | NestedElement> = {};

const hasSubkeys = (props: SettingsToml[string]): props is Props => {
  return "type" in props;
};

for (const key in doc) {
  const props = doc[key];
  if (hasSubkeys(props)) {
    settings[key] = buildElement(key, props);
  } else {
    for (const subkey in props) {
      settings[key] ??= {
        type: "object",
        additionalProperties: false,
        properties: {},
      };
      if (props.deprecated) {
        settings[key].deprecated = true;
      }
      (settings[key] as NestedElement).properties[subkey] = buildElement(
        `${key}.${subkey}`,
        props[subkey],
      );
    }
  }
}

const schema = JSON.parse(fs.readFileSync("schema/mise.json", "utf-8"));
schema["$defs"].settings.properties = settings;
fs.writeFileSync("schema/mise.json.tmp", JSON.stringify(schema));

child_process.execSync("jq . < schema/mise.json.tmp > schema/mise.json");
child_process.execSync("prettier --write schema/mise.json");
fs.unlinkSync("schema/mise.json.tmp");

const taskSchema = JSON.parse(
  fs.readFileSync("schema/mise-task.json", "utf-8"),
);
taskSchema["$defs"].env_directive = schema["$defs"].env_directive;
taskSchema["$defs"].env = schema["$defs"].env;
taskSchema["$defs"].task_run_entry = schema["$defs"].task_run_entry;
taskSchema["$defs"].task = schema["$defs"].task;
fs.writeFileSync("schema/mise-task.json.tmp", JSON.stringify(taskSchema));
child_process.execSync(
  "jq . < schema/mise-task.json.tmp > schema/mise-task.json",
);
child_process.execSync("prettier --write schema/mise-task.json");
fs.unlinkSync("schema/mise-task.json.tmp");
