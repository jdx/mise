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

    const typeMap = {
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
      BoolOrString: "boolean | string",
    };

    function buildElement(key, props) {
      const type = typeMap[props.type] || props.type;
      let default_ = props.default_docs ?? props.default;
      if (default_ === undefined && type === "boolean" && !props.optional) {
        default_ = false;
      }
      if (default_ === undefined && props.optional) {
        default_ = "None";
      }

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
