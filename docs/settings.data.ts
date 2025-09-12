import * as fs from "node:fs";
import { load } from "js-toml";
import markdownit from "markdown-it";

const md = markdownit();

export default {
  watch: ["./settings.toml"],
  load() {
    const settings = {};
    const raw = fs.readFileSync("./settings.toml", "utf-8");
    const doc = load(raw);

    function getParseEnv(parseEnv) {
      if (parseEnv === "list_by_comma") {
        return "comma";
      }
      if (parseEnv === "list_by_colon") {
        return "colon";
      }
      return undefined;
    }

    function buildElement(key, props) {
      let type = props.type;
      type = type.replaceAll("PathBuf", "String");
      let default_ = props.default_docs ?? props.default;
      if (default_ === undefined && type === "Bool" && !props.optional) {
        default_ = false;
      }
      if (default_ === undefined && props.optional) {
        default_ = "None";
      }
      if (type === "Integer") {
        type = "integer";
      } else if (type === "String") {
        type = "string";
      } else if (type === "ListString" || type === "ListPath") {
        type = "string[]";
      } else if (type === "IndexMap<String, String>") {
        type = "object";
      }
      // } else if (type === "String" || type === "PathBuf") {
      //   type = 'string';
      // } else if (type === "usize" || type === "u64") {
      //   type = 'number';
      // } else {
      //   throw new Error(`Unknown type: ${type}`);
      // }
      const ele = {
        key,
        default: default_,
        docs: md.render(props.docs ?? props.description),
        deprecated: props.deprecated,
        enum: props.enum,
        env: props.env,
        parseEnv: getParseEnv(props.parse_env),
        optional: !props.default_docs && props.optional,
        type,
      };
      return ele;
    }

    for (const key in doc) {
      const props = doc[key];
      if (props.hide) continue;
      if (props.type) {
        settings[key] = buildElement(key, props);
      } else {
        for (const subkey in props) {
          if (props.hide) continue;
          settings[key] = settings[key] || {
            key,
            additionalProperties: false,
            description: props.description,
            settings: [],
          };
          settings[key].settings.push(
            buildElement(`${key}.${subkey}`, props[subkey]),
          );
        }
      }
    }
    return Object.values(settings).sort((a, b) => a.key.localeCompare(b.key));
  },
};
