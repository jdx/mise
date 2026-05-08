#!/usr/bin/env bun

//MISE description="Render JSON schema"
//MISE depends=["docs:setup"]

import * as fs from "node:fs";
import * as toml from "toml";

type EnumValue = string | boolean | number;
type EnumItem = EnumValue | { value: EnumValue; description?: string };

type Props = {
  type: string;
  description: string;
  default?: unknown;
  deprecated?: string;
  enum?: EnumItem[];
  rc?: boolean;
};

type SettingsToml = Record<string, Props | Record<string, Props>>;
type JsonObject = Record<string, unknown>;

type Element = {
  type: string | string[];
  default: unknown;
  description: string;
  deprecated?: true;
  enum?: EnumValue[];
  items?: {
    type: string | string[];
  };
  additionalProperties?: {
    type: string;
  };
};

type NestedElement = {
  type: "object";
  unevaluatedProperties: false;
  deprecated?: true;
  properties: Record<string, Element>;
};

function writeFormattedJson(path: string, value: unknown) {
  fs.writeFileSync(path, `${JSON.stringify(value, null, 2)}\n`);
}

function crawlReferencedDefs(schema: JsonObject, root: unknown) {
  const defs = schema["$defs"] as JsonObject | undefined;
  if (!defs) {
    throw new Error("schema/mise.json is missing $defs");
  }

  const queued = [root];
  const seenDefs = new Set<string>();
  const picked: JsonObject = {};

  for (let i = 0; i < queued.length; i++) {
    const value = queued[i];
    if (Array.isArray(value)) {
      queued.push(...value);
      continue;
    }
    if (!value || typeof value !== "object") {
      continue;
    }

    const obj = value as JsonObject;
    const ref = obj["$ref"];
    if (typeof ref === "string" && ref.startsWith("#/$defs/")) {
      const key = ref.slice("#/$defs/".length).split("/")[0];
      if (defs[key] === undefined) {
        throw new Error(`schema/mise.json is missing $defs.${key}`);
      }
      if (!seenDefs.has(key)) {
        seenDefs.add(key);
        picked[key] = defs[key];
        queued.push(defs[key]);
      }
    } else if (typeof ref === "string" && ref.startsWith("#/")) {
      throw new Error(`unsupported local JSON schema ref: ${ref}`);
    }

    queued.push(...Object.values(obj));
  }

  return picked;
}

function buildTaskSchema(schema: JsonObject) {
  const taskSchema: JsonObject = {
    $id: "https://mise.en.dev/schema/mise-task.json",
    $schema: schema["$schema"],
    title: "mise-task-schema",
    type: "object",
    description:
      "Config file for included mise tasks (https://mise.en.dev/tasks/#task-configuration)",
    additionalProperties: {
      $ref: "#/$defs/task",
    },
  };
  taskSchema["$defs"] = crawlReferencedDefs(schema, taskSchema);
  return taskSchema;
}

function buildElement(key: string, props: Props): Element {
  const typeMap: Record<string, string | string[]> = {
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
    BoolOrString: "__bool_or_string__",
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
    element.enum = props.enum.map((e) =>
      typeof e === "object" && e !== null && "value" in e ? e.value : e,
    );
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

  // BoolOrString: use oneOf instead of union type array for AJV strictTypes
  if (type === "__bool_or_string__") {
    delete (element as Record<string, unknown>).type;
    const oneOfTypes: Array<{ type: string }> = [
      { type: "boolean" },
      { type: "string" },
    ];
    (element as Record<string, unknown>).oneOf = oneOfTypes;
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
        unevaluatedProperties: false,
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

writeFormattedJson("schema/mise.json", schema);
writeFormattedJson("schema/mise-task.json", buildTaskSchema(schema));

// Generate .miserc.toml schema with only rc=true settings
const misercSettings: Record<string, Element> = {};

for (const key in doc) {
  const props = doc[key];
  if (hasSubkeys(props) && props.rc === true) {
    misercSettings[key] = buildElement(key, props);
  }
}

const misercSchema = {
  $schema: "https://json-schema.org/draft/2020-12/schema",
  title: "mise rc config",
  description:
    "Early initialization settings for mise. These settings are loaded before the main config files.",
  type: "object",
  unevaluatedProperties: false,
  properties: misercSettings,
};

writeFormattedJson("schema/miserc.json", misercSchema);
