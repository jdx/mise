#!/usr/bin/env bun

//MISE description="Render JSON schema"
//MISE depends=["docs:setup"]

import * as fs from "node:fs";
import * as child_process from "node:child_process";
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
  additionalProperties: false;
  deprecated?: true;
  properties: Record<string, Element>;
};

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
    BoolOrString: ["boolean", "string"],
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

// Generate task and task_template from task_props to avoid unevaluatedProperties
// (which Tombi doesn't support) while keeping extends only on tasks, not templates.
const taskProps = schema["$defs"].task_props;

// task_template: task_props + additionalProperties: false
schema["$defs"].task_template = {
  description: "task template that can be extended by tasks",
  properties: { ...taskProps.properties },
  additionalProperties: false,
  type: "object",
};

// task (object variant): task_props + extends + additionalProperties: false
const taskObjectVariant = {
  properties: {
    ...taskProps.properties,
    extends: {
      description: "name of the task template to extend",
      type: "string",
    },
  },
  additionalProperties: false,
  type: "object",
};

// Overwrite the object variant (last entry) in task oneOf with inlined properties
const taskDef = schema["$defs"].task;
taskDef.oneOf[taskDef.oneOf.length - 1] = taskObjectVariant;

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
taskSchema["$defs"].task_template = schema["$defs"].task_template;
delete taskSchema["$defs"].task_props;
fs.writeFileSync("schema/mise-task.json.tmp", JSON.stringify(taskSchema));
child_process.execSync(
  "jq . < schema/mise-task.json.tmp > schema/mise-task.json",
);
child_process.execSync("prettier --write schema/mise-task.json");
fs.unlinkSync("schema/mise-task.json.tmp");

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
  additionalProperties: false,
  properties: misercSettings,
};

fs.writeFileSync("schema/miserc.json.tmp", JSON.stringify(misercSchema));
child_process.execSync("jq . < schema/miserc.json.tmp > schema/miserc.json");
child_process.execSync("prettier --write schema/miserc.json");
fs.unlinkSync("schema/miserc.json.tmp");
