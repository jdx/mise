#!/usr/bin/env node

//MISE description="Render JSON schema"
//MISE depends=["docs:setup"]

import { spawnSync } from "node:child_process";
import * as fs from "node:fs";
import * as toml from "toml";

type EnumValue = string | boolean | number;
type EnumItem = EnumValue | { value: EnumValue; description?: string };

type Props = {
  type: string;
  description: string;
  default?: unknown;
  rust_type?: string;
  deprecated?: string;
  enum?: EnumItem[];
  rc?: boolean;
  deserialize_with?: string;
};

type SettingsNode = Props | { [key: string]: SettingsNode };
type SettingsToml = Record<string, SettingsNode>;
type JsonObject = Record<string, unknown>;

type Element = {
  type: string | string[];
  default: unknown;
  description: string;
  deprecated?: true;
  minimum?: number;
  enum?: EnumValue[];
  items?: {
    type: string | string[];
    enum?: EnumValue[];
  };
  additionalProperties?: {
    type: string;
  };
};

type NestedElement = {
  type: "object";
  unevaluatedProperties: false;
  deprecated?: true;
  properties: Record<string, Element | NestedElement>;
};

const writtenPaths: string[] = [];

function writeFormattedJson(path: string, value: unknown) {
  fs.writeFileSync(path, `${JSON.stringify(value, null, 2)}\n`);
  writtenPaths.push(path);
}

function formatWithOxfmt(paths: string[]) {
  if (paths.length === 0) return;
  const result = spawnSync("oxfmt", ["--write", ...paths], {
    stdio: "inherit",
  });
  if (result.error) {
    throw new Error(`oxfmt failed to spawn: ${result.error.message}`);
  }
  if (result.status !== 0) {
    throw new Error(`oxfmt exited with status ${result.status}`);
  }
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
    $id: "https://mise.jdx.dev/schema/mise-task.json",
    $schema: schema["$schema"],
    title: "mise-task-schema",
    type: "object",
    description:
      "Config file for included mise tasks (https://mise.jdx.dev/tasks/task-configuration.html)",
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
    Integer: "integer",
    ListString: "string[]",
    ListPath: "string[]",
    SetString: "string[]",
    "IndexMap<String, String>": "object",
    BoolOrString: "__bool_or_string__",
    BoolOrList: "__bool_or_list__",
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
  if (type === "integer" && props.rust_type?.startsWith("u")) {
    element.minimum = 0;
  }
  const enumValues = props.enum?.map((e) =>
    typeof e === "object" && e !== null && "value" in e ? e.value : e,
  );

  if (type === "string[]") {
    element.type = "array";
    element.items = {
      type: "string",
      ...(enumValues ? { enum: enumValues } : {}),
    };
  } else if (enumValues) {
    element.enum = enumValues;
  }

  if (type === "object") {
    element.additionalProperties = {
      type: "string",
    };
  }

  // A list setting that also takes a single value: use oneOf for AJV strictTypes
  if (type === "string[]" && props.deserialize_with === "path_or_paths") {
    const items = element.items;
    delete (element as Record<string, unknown>).type;
    delete (element as Record<string, unknown>).items;
    (element as Record<string, unknown>).oneOf = [
      { type: "string" },
      { type: "array", items },
    ];
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

  // BoolOrList: a boolean, or a list of the strings the setting's enum allows.
  if (type === "__bool_or_list__") {
    delete (element as Record<string, unknown>).type;
    delete element.enum;
    (element as Record<string, unknown>).oneOf = [
      { type: "boolean" },
      {
        type: "array",
        items: { type: "string", ...(enumValues ? { enum: enumValues } : {}) },
      },
    ];
  }

  return element;
}

const doc = toml.parse(
  fs.readFileSync("settings.toml", "utf-8"),
) as SettingsToml;
const isLeaf = (node: SettingsNode): node is Props => {
  return "type" in node;
};

function buildSettings(
  table: Record<string, SettingsNode>,
  prefix = "",
): Record<string, Element | NestedElement> {
  const settings: Record<string, Element | NestedElement> = {};
  for (const [key, node] of Object.entries(table)) {
    const name = prefix ? `${prefix}.${key}` : key;
    if (isLeaf(node)) {
      settings[key] = buildElement(name, node);
    } else {
      settings[key] = {
        type: "object",
        unevaluatedProperties: false,
        properties: buildSettings(node, name),
      };
    }
  }
  return settings;
}

const settings = buildSettings(doc);

// The sections whose types live in crates/mise-config, generated from those types.
function generatedFragments(): Record<string, unknown> {
  const result = spawnSync(
    "cargo",
    [
      "run",
      "--quiet",
      "--package",
      "mise-config",
      "--example",
      "render-schema",
    ],
    {
      encoding: "utf-8",
      stdio: ["ignore", "pipe", "inherit"],
      maxBuffer: 64 * 1024 * 1024,
    },
  );
  if (result.error) {
    throw new Error(`cargo failed to spawn: ${result.error.message}`);
  }
  if (result.status !== 0) {
    throw new Error(
      `mise-config's render-schema example exited with status ${result.status}`,
    );
  }
  return JSON.parse(result.stdout);
}

// Replaces the value at a JSON pointer. The key must already exist, so a mistyped pointer
// fails instead of adding a section nothing references.
function replaceAtPointer(root: JsonObject, pointer: string, value: unknown) {
  const keys = pointer
    .split("/")
    .slice(1)
    .map((key) => key.replaceAll("~1", "/").replaceAll("~0", "~"));
  const last = keys.pop();
  let node: unknown = root;
  for (const key of keys) {
    node = (node as JsonObject | undefined)?.[key];
  }
  if (
    last === undefined ||
    typeof node !== "object" ||
    node === null ||
    !(last in node)
  ) {
    throw new Error(`schema/mise.json has no ${pointer} to replace`);
  }
  (node as JsonObject)[last] = value;
}

const schema = JSON.parse(fs.readFileSync("schema/mise.json", "utf-8"));
schema["$defs"].settings.properties = settings;
for (const [pointer, fragment] of Object.entries(generatedFragments())) {
  replaceAtPointer(schema, pointer, fragment);
}

writeFormattedJson("schema/mise.json", schema);
writeFormattedJson("schema/mise-task.json", buildTaskSchema(schema));

// Generate .miserc.toml schema with only rc=true settings
const misercSettings: Record<string, Element> = {};

for (const key in doc) {
  const props = doc[key];
  if (isLeaf(props) && props.rc === true) {
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

formatWithOxfmt(writtenPaths);
