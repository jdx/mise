// If not being published, these need to manually downloaded from https://github.com/withfig/autocomplete/tree/master/src
import { createNpmSearchHandler } from "./npm";
import { searchGenerator as createCargoSearchGenerator } from "./cargo";

const envVarGenerator = {
  script: ["sh", "-c", "env"],
  postProcess: (output: string) => {
    return output.split("\n").map((l) => ({ name: l.split("=")[0] }));
  },
};

const singleCmdNewLineGenerator = (completion_cmd: string): Fig.Generator => ({
  script: completion_cmd.split(" "),
  splitOn: "\n",
});

const singleCmdJsonGenerator = (cmd: string): Fig.Generator => ({
  script: cmd.split(" "),
  postProcess: (out) =>
    JSON.parse(out).map((r: any) => ({
      name: r.name,
      description: r.description,
    })),
});

const contextualGeneratorLastWord = (cmd: string): Fig.Generator => ({
  script: (context) => {
    if (context.length < 2) {
      return [];
    }

    const prev = context[context.length - 2]; // -1 is the current word
    return ["sh", "-c", [cmd, prev].join(" ")];
  },
});

const aliasGenerator: Fig.Generator = {
  ...contextualGeneratorLastWord("mise alias ls"),
  postProcess: (out) => {
    //return [{name: out}]
    //return out.split('\t').map(l => ({name: l}))
    //return [{name: "test", "description": out}]
    const tokens = out.split(/\s+/);
    if (tokens.length == 0) return [];

    return tokens
      .flatMap((_, i) => {
        if (i % 3 == 0) {
          return [tokens[i + 1]];
        }
        return [];
      })
      .filter((l) => l.trim().length > 0)
      .map((l) => ({ name: l.trim() }));
  },
};

const pluginWithAlias: Fig.Generator = {
  script: "mise alias ls".split(" "),
  postProcess: (output: string) => {
    const plugins = output.split("\n").map((line) => {
      const tokens = line.split(/\s+/);
      return tokens[0];
    });
    return [...new Set(plugins)].map((p) => ({ name: p }));
  },
};

const getInstalledTools = async (
  executeShellCommand: Fig.ExecuteCommandFunction,
) => {
  const { stdout } = await executeShellCommand({
    command: "sh",
    args: ["-c", "mise ls --installed"],
  });
  return [
    ...new Set(
      stdout.split("\n").map((l) => {
        const tokens = l.split(/\s+/);
        return { name: tokens[0], version: tokens[1] };
      }),
    ),
  ];
};

type ConfigLsOutput = {
  path: string;
  tools: string[];
};

const configPathGenerator: Fig.Generator = {
  ...singleCmdJsonGenerator("mise config ls -J"),
  postProcess: (out) =>
    JSON.parse(out).map((r: ConfigLsOutput) => ({
      name: r.path,
      description: r.path,
    })),
};

type ObjectKeyType = string | symbol | number;
type ObjectAcceptableKeyValues = {
  [key: string]: ObjectKeyType;
};

function groupBy<T extends ObjectAcceptableKeyValues>(
  array: T[],
  key: keyof T,
): Record<T[keyof T], T[]> {
  return array.reduce(
    (result, currentItem) => {
      (result[currentItem[key] as ObjectKeyType] =
        result[currentItem[key] as ObjectKeyType] || []).push(currentItem);
      return result;
    },
    {} as Record<ObjectKeyType, T[]>,
  );
}

const installedToolsGenerator: Fig.Generator = {
  script: ["sh", "-c", "mise ls --installed"],
  postProcess: (stdout: string) => {
    return [
      ...new Set(
        stdout.split("\n").map((l) => {
          const tokens = l.split(/\s+/);
          return { name: tokens[0], version: tokens[1] };
        }),
      ),
    ];
  },
};

const pluginGenerator: Fig.Generator = installedToolsGenerator;
const allPluginsGenerator: Fig.Generator =
  singleCmdNewLineGenerator("mise plugins --all");
const simpleTaskGenerator = singleCmdJsonGenerator("mise tasks -J");
const settingsGenerator = singleCmdNewLineGenerator(`mise settings --keys`);

const atsInStr = (s: string) => (s.match(/@/g) || []).length != 0;
const backendSepInStr = (s: string) => (s.match(/:/g) || []).length != 0;

const searchBackend = async (
  backend: string,
  context: string[],
  executeShellCommand: Fig.ExecuteCommandFunction,
  shellContext: Fig.GeneratorContext,
): Promise<Fig.Suggestion[]> => {
  const customContext = context;
  customContext[context.length - 1] = customContext[context.length - 1].replace(
    `${backend}:`,
    "",
  );
  switch (backend) {
    case "npm":
      return await createNpmSearchHandler()(
        context,
        executeShellCommand,
        shellContext,
      );
    case "cargo":
      //return [{name: customContext[context.length - 1]}]
      return await createCargoSearchGenerator.custom(
        customContext,
        executeShellCommand,
        shellContext,
      );
    case "asdf":
      const { stdout } = await executeShellCommand({
        command: "sh",
        args: ["-c", "mise registry"],
      });
      return [
        ...new Set(
          stdout.split("\n").map((l) => {
            const tokens = l.split(/\s+/);
            return { name: tokens[1].replace(`${backend}:`, "") };
          }),
        ),
      ];
    case "ubi":
      const { stdout: ubiOut } = await executeShellCommand({
        command: "sh",
        args: ["-c", "mise registry"],
      });
      return [
        ...new Set(
          ubiOut.split("\n").flatMap((l) => {
            const tokens = l.split(/\s+/);
            if (!tokens[1].includes("ubi:")) return [];
            return [{ name: tokens[1].replace("ubi:", "") }] as Fig.Suggestion;
          }),
        ),
      ];
    default:
      return [];
  }
};

const compareVersions = (a: string, b: string): number => {
  const result = [a, b].sort(); // Unless we can add semversort
  if (result[0] != a) return 1;
  return -1;
};

const toolVersionGenerator: Fig.Generator = {
  trigger: (newToken: string, oldToken: string): boolean => {
    return (
      (backendSepInStr(newToken) && !backendSepInStr(oldToken)) ||
      (atsInStr(newToken) && !atsInStr(oldToken))
    );
  },
  getQueryTerm: "@",

  custom: async (
    context: string[],
    executeShellCommand: Fig.ExecuteCommandFunction,
    shellContext: Fig.GeneratorContext,
  ) => {
    const currentWord = context[context.length - 1];
    if (backendSepInStr(currentWord)) {
      // Let's handle backends
      const backend = currentWord.slice(0, currentWord.lastIndexOf(":"));

      return (
        await searchBackend(backend, context, executeShellCommand, shellContext)
      ).map((s) => ({
        ...s,
        name: `${backend}:${s.name}`,
        displayName: s.name,
        icon: "📦",
      }));
    } else if (atsInStr(currentWord)) {
      const tool = currentWord.slice(0, currentWord.lastIndexOf("@"));
      const { stdout } = await executeShellCommand({
        command: "sh",
        args: ["-c", `mise ls-remote ${tool}`],
      });
      const remote_versions_suggestions = stdout
        .split("\n")
        .sort((a, b) => compareVersions(b, a))
        .map((l) => ({ name: l }));
      const { stdout: aliasStdout } = await executeShellCommand({
        command: "sh",
        args: ["-c", `mise alias ls ${tool}`],
      });
      const aliases_suggestions = aliasStdout.split("\n").map((l) => {
        const tokens = l.split(/\s+/);
        return { name: tokens[1] };
      });
      return [...aliases_suggestions, ...remote_versions_suggestions];
    }

    const { stdout } = await executeShellCommand({
      command: "sh",
      args: ["-c", "mise registry"],
    });
    return [
      ...new Set(
        stdout.split("\n").map((l) => {
          const tokens = l.split(/\s+/);
          return { name: tokens[0] };
        }),
      ),
    ];
  },
};

const installedToolVersionGenerator: Fig.Generator = {
  trigger: "@",
  getQueryTerm: "@",
  custom: async (
    context: string[],
    executeShellCommand: Fig.ExecuteCommandFunction,
  ) => {
    const tools = await getInstalledTools(executeShellCommand);
    const toolsVersions = groupBy(tools, "name");

    const currentWord = context[context.length - 1];
    if (atsInStr(currentWord)) {
      const tool = currentWord.slice(0, currentWord.lastIndexOf("@"));

      const { stdout: aliasStdout } = await executeShellCommand({
        command: "sh",
        args: ["-c", `mise alias ls ${tool}`],
      });

      // This lists all aliases even if they are not installed
      /*
      const aliases_suggestions = aliasStdout.split('\n').map(l => {
        const tokens = l.split(/\s+/)
        return {name: tokens[1], description: tokens[2]}
      }) as Fig.Suggestion[]
      */

      const toolVersions = (toolsVersions[tool] || []) as {
        name: string;
        version: string;
      }[];
      const suggestions = toolVersions.map((s) => ({
        name: s.version,
      })) as Fig.Suggestion[];

      return [...suggestions];
    }

    const suggestions: Fig.Suggestion[] = [];
    Object.keys(toolsVersions).forEach((k) => {
      if (toolsVersions[k].length == 1) {
        suggestions.push({ name: k });
      } else {
        suggestions.push({ name: `${k}@` });
      }
    });

    return suggestions;
  },
};

const usageGenerateSpec = (cmds: string[]) => {
    return async (context: string[], executeCommand: Fig.ExecuteCommandFunction): Promise<Fig.Spec> => {
        const promises = cmds.map(async (cmd): Promise<Fig.Subcommand[]> => {
            try {
                const args = cmd.split(" ");
                const { stdout, stderr: cmdStderr, status: cmdStatus, } = await executeCommand({
                    command: args[0],
                    args: args.splice(1),
                });
                if (cmdStatus !== 0) {
                    return [{ name: "error", description: cmdStderr }];
                }
                const { stdout: figSpecOut, stderr: figSpecStderr, status: usageFigStatus, } = await executeCommand({
                    command: "usage",
                    args: ["g", "fig", "--spec", stdout],
                });
                if (usageFigStatus !== 0) {
                    return [{ name: "error", description: figSpecStderr }];
                }
                const start_of_json = figSpecOut.indexOf("{");
                const j = figSpecOut.slice(start_of_json);
                return JSON.parse(j).subcommands as Fig.Subcommand[];
            }
            catch (e) {
                return [{ name: "error", description: e }] as Fig.Subcommand[];
            }
        });
        // eslint-disable-next-line compat/compat
        const results = await Promise.allSettled(promises);
        const subcommands = results
            .filter((p) => p.status === "fulfilled")
            .map((p) => p.value);
        const failed = results
            .filter((p) => p.status === "rejected")
            .map((p) => ({ name: "error", description: p.reason }));
        return { subcommands: [...subcommands.flat(), ...failed] } as Fig.Spec;
    };
};
const completionGeneratorTemplate = (argSuggestionBash: string): Fig.Generator => {
    return {
        custom: async (tokens: string[], executeCommand) => {
            let arg = argSuggestionBash;
            if (tokens.length >= 1) {
                arg = argSuggestionBash.replace("{{words[CURRENT]}}", tokens[tokens.length - 1]);
            }
            if (tokens.length >= 2) {
                arg = arg.replace(`{{words[PREV]}}`, tokens[tokens.length - 2]);
            }
            const { stdout: text } = await executeCommand({
                command: "sh",
                args: ["-c", arg],
            });
            if (text.trim().length == 0)
                return [];
            return text.split("\n").map((elm) => ({ name: elm }));
        },
    };
};
const completionSpec: Fig.Spec = {
    "name": [
        "mise"
    ],
    "subcommands": [
        {
            "name": [
                "activate"
            ],
            "description": "Initializes mise in the current shell session",
            "options": [
                {
                    "name": [
                        "--shims"
                    ],
                    "description": "Use shims instead of modifying PATH\nEffectively the same as:",
                    "isRepeatable": false
                },
                {
                    "name": [
                        "-q",
                        "--quiet"
                    ],
                    "description": "Suppress non-error messages",
                    "isRepeatable": false
                }
            ],
            "args": [
                {
                    "name": "shell_type",
                    "description": "Shell type to generate the script for",
                    "isOptional": true,
                    "isVariadic": false,
                    "suggestions": [
                        "bash",
                        "elvish",
                        "fish",
                        "nu",
                        "xonsh",
                        "zsh"
                    ]
                }
            ]
        },
        {
            "name": [
                "alias",
                "a"
            ],
            "description": "Manage aliases",
            "subcommands": [
                {
                    "name": [
                        "get"
                    ],
                    "description": "Show an alias for a plugin",
                    "args": [
                        {
                            "name": "plugin",
                            "description": "The plugin to show the alias for",
                            "isOptional": false,
                            "isVariadic": false,
                            "generators": pluginGenerator
                        },
                        {
                            "name": "alias",
                            "description": "The alias to show",
                            "isOptional": false,
                            "isVariadic": false,
                            "generators": aliasGenerator
                        }
                    ]
                },
                {
                    "name": [
                        "ls",
                        "list"
                    ],
                    "description": "List aliases\nShows the aliases that can be specified.\nThese can come from user config or from plugins in `bin/list-aliases`.",
                    "options": [
                        {
                            "name": [
                                "--no-header"
                            ],
                            "description": "Don't show table header",
                            "isRepeatable": false
                        }
                    ],
                    "args": [
                        {
                            "name": "tool",
                            "description": "Show aliases for <TOOL>",
                            "isOptional": true,
                            "isVariadic": false
                        }
                    ]
                },
                {
                    "name": [
                        "set",
                        "add",
                        "create"
                    ],
                    "description": "Add/update an alias for a plugin",
                    "args": [
                        {
                            "name": "plugin",
                            "description": "The plugin to set the alias for",
                            "isOptional": false,
                            "isVariadic": false,
                            "generators": pluginGenerator
                        },
                        {
                            "name": "alias",
                            "description": "The alias to set",
                            "isOptional": false,
                            "isVariadic": false,
                            "generators": aliasGenerator
                        },
                        {
                            "name": "value",
                            "description": "The value to set the alias to",
                            "isOptional": false,
                            "isVariadic": false
                        }
                    ]
                },
                {
                    "name": [
                        "unset",
                        "rm",
                        "remove",
                        "delete",
                        "del"
                    ],
                    "description": "Clears an alias for a plugin",
                    "args": [
                        {
                            "name": "plugin",
                            "description": "The plugin to remove the alias from",
                            "isOptional": false,
                            "isVariadic": false,
                            "generators": pluginGenerator
                        },
                        {
                            "name": "alias",
                            "description": "The alias to remove",
                            "isOptional": false,
                            "isVariadic": false,
                            "generators": aliasGenerator
                        }
                    ]
                }
            ],
            "options": [
                {
                    "name": [
                        "-p",
                        "--plugin"
                    ],
                    "description": "filter aliases by plugin",
                    "isRepeatable": false,
                    "args": {
                        "name": "plugin",
                        "isOptional": false,
                        "isVariadic": false,
                        "generators": pluginGenerator
                    }
                },
                {
                    "name": [
                        "--no-header"
                    ],
                    "description": "Don't show table header",
                    "isRepeatable": false
                }
            ]
        },
        {
            "name": [
                "backends",
                "b"
            ],
            "description": "Manage backends",
            "subcommands": [
                {
                    "name": [
                        "ls",
                        "list"
                    ],
                    "description": "List built-in backends"
                }
            ]
        },
        {
            "name": [
                "bin-paths"
            ],
            "description": "List all the active runtime bin paths",
            "args": [
                {
                    "name": "tool@version",
                    "description": "Tool(s) to look up\ne.g.: ruby@3",
                    "isOptional": true,
                    "isVariadic": true,
                    "generators": toolVersionGenerator
                }
            ]
        },
        {
            "name": [
                "cache"
            ],
            "description": "Manage the mise cache",
            "subcommands": [
                {
                    "name": [
                        "clear",
                        "c"
                    ],
                    "description": "Deletes all cache files in mise",
                    "args": [
                        {
                            "name": "plugin",
                            "description": "Plugin(s) to clear cache for e.g.: node, python",
                            "isOptional": true,
                            "isVariadic": true,
                            "generators": pluginGenerator
                        }
                    ]
                },
                {
                    "name": [
                        "prune",
                        "p"
                    ],
                    "description": "Removes stale mise cache files",
                    "options": [
                        {
                            "name": [
                                "--dry-run"
                            ],
                            "description": "Just show what would be pruned",
                            "isRepeatable": false
                        },
                        {
                            "name": [
                                "-v",
                                "--verbose"
                            ],
                            "description": "Show pruned files",
                            "isRepeatable": true
                        }
                    ],
                    "args": [
                        {
                            "name": "plugin",
                            "description": "Plugin(s) to clear cache for e.g.: node, python",
                            "isOptional": true,
                            "isVariadic": true,
                            "generators": pluginGenerator
                        }
                    ]
                }
            ]
        },
        {
            "name": [
                "completion"
            ],
            "description": "Generate shell completions",
            "args": [
                {
                    "name": "shell",
                    "description": "Shell type to generate completions for",
                    "isOptional": true,
                    "isVariadic": false,
                    "suggestions": [
                        "bash",
                        "fish",
                        "zsh"
                    ]
                }
            ]
        },
        {
            "name": [
                "config",
                "cfg"
            ],
            "description": "Manage config files",
            "subcommands": [
                {
                    "name": [
                        "generate",
                        "g"
                    ],
                    "description": "[experimental] Generate a mise.toml file",
                    "options": [
                        {
                            "name": [
                                "-o",
                                "--output"
                            ],
                            "description": "Output to file instead of stdout",
                            "isRepeatable": false,
                            "args": {
                                "name": "output",
                                "isOptional": false,
                                "isVariadic": false
                            }
                        }
                    ]
                },
                {
                    "name": [
                        "get"
                    ],
                    "description": "Display the value of a setting in a mise.toml file",
                    "options": [
                        {
                            "name": [
                                "-f",
                                "--file"
                            ],
                            "description": "The path to the mise.toml file to edit",
                            "isRepeatable": false,
                            "args": {
                                "name": "file",
                                "isOptional": false,
                                "isVariadic": false,
                                "template": "filepaths"
                            }
                        }
                    ],
                    "args": [
                        {
                            "name": "key",
                            "description": "The path of the config to display",
                            "isOptional": true,
                            "isVariadic": false
                        }
                    ]
                },
                {
                    "name": [
                        "ls"
                    ],
                    "description": "List config files currently in use",
                    "options": [
                        {
                            "name": [
                                "--no-header"
                            ],
                            "description": "Do not print table header",
                            "isRepeatable": false
                        },
                        {
                            "name": [
                                "-J",
                                "--json"
                            ],
                            "description": "Output in JSON format",
                            "isRepeatable": false
                        }
                    ]
                },
                {
                    "name": [
                        "set"
                    ],
                    "description": "Set the value of a setting in a mise.toml file",
                    "options": [
                        {
                            "name": [
                                "-f",
                                "--file"
                            ],
                            "description": "The path to the mise.toml file to edit",
                            "isRepeatable": false,
                            "args": {
                                "name": "file",
                                "isOptional": false,
                                "isVariadic": false,
                                "template": "filepaths"
                            }
                        },
                        {
                            "name": [
                                "-t",
                                "--type"
                            ],
                            "isRepeatable": false,
                            "args": {
                                "name": "type",
                                "isOptional": false,
                                "isVariadic": false,
                                "suggestions": [
                                    "infer",
                                    "string",
                                    "integer",
                                    "float",
                                    "bool",
                                    "list"
                                ]
                            }
                        }
                    ],
                    "args": [
                        {
                            "name": "key",
                            "description": "The path of the config to display",
                            "isOptional": false,
                            "isVariadic": false
                        },
                        {
                            "name": "value",
                            "description": "The value to set the key to",
                            "isOptional": false,
                            "isVariadic": false
                        }
                    ]
                }
            ],
            "options": [
                {
                    "name": [
                        "--no-header"
                    ],
                    "description": "Do not print table header",
                    "isRepeatable": false
                },
                {
                    "name": [
                        "-J",
                        "--json"
                    ],
                    "description": "Output in JSON format",
                    "isRepeatable": false
                }
            ]
        },
        {
            "name": [
                "deactivate"
            ],
            "description": "Disable mise for current shell session"
        },
        {
            "name": [
                "doctor",
                "dr"
            ],
            "description": "Check mise installation for possible problems"
        },
        {
            "name": [
                "en"
            ],
            "description": "[experimental] starts a new shell with the mise environment built from the current configuration",
            "options": [
                {
                    "name": [
                        "-s",
                        "--shell"
                    ],
                    "description": "Shell to start",
                    "isRepeatable": false,
                    "args": {
                        "name": "shell",
                        "isOptional": false,
                        "isVariadic": false
                    }
                }
            ],
            "args": [
                {
                    "name": "dir",
                    "description": "Directory to start the shell in",
                    "isOptional": true,
                    "isVariadic": false,
                    "template": "folders"
                }
            ]
        },
        {
            "name": [
                "env",
                "e"
            ],
            "description": "Exports env vars to activate mise a single time",
            "options": [
                {
                    "name": [
                        "-J",
                        "--json"
                    ],
                    "description": "Output in JSON format",
                    "isRepeatable": false
                },
                {
                    "name": [
                        "--json-extended"
                    ],
                    "description": "Output in JSON format with additional information (source, tool)",
                    "isRepeatable": false
                },
                {
                    "name": [
                        "-D",
                        "--dotenv"
                    ],
                    "description": "Output in dotenv format",
                    "isRepeatable": false
                },
                {
                    "name": [
                        "-s",
                        "--shell"
                    ],
                    "description": "Shell type to generate environment variables for",
                    "isRepeatable": false,
                    "args": {
                        "name": "shell",
                        "isOptional": false,
                        "isVariadic": false,
                        "suggestions": [
                            "bash",
                            "elvish",
                            "fish",
                            "nu",
                            "xonsh",
                            "zsh"
                        ]
                    }
                }
            ],
            "args": [
                {
                    "name": "tool@version",
                    "description": "Tool(s) to use",
                    "isOptional": true,
                    "isVariadic": true,
                    "generators": toolVersionGenerator
                }
            ]
        },
        {
            "name": [
                "exec",
                "x"
            ],
            "description": "Execute a command with tool(s) set",
            "options": [
                {
                    "name": [
                        "-c",
                        "--command"
                    ],
                    "description": "Command string to execute",
                    "isRepeatable": false,
                    "args": {
                        "name": "c",
                        "isOptional": false,
                        "isVariadic": false
                    }
                },
                {
                    "name": [
                        "-j",
                        "--jobs"
                    ],
                    "description": "Number of jobs to run in parallel\n[default: 4]",
                    "isRepeatable": false,
                    "args": {
                        "name": "jobs",
                        "isOptional": false,
                        "isVariadic": false
                    }
                },
                {
                    "name": [
                        "--raw"
                    ],
                    "description": "Directly pipe stdin/stdout/stderr from plugin to user Sets --jobs=1",
                    "isRepeatable": false
                }
            ],
            "args": [
                {
                    "name": "tool@version",
                    "description": "Tool(s) to start e.g.: node@20 python@3.10",
                    "isOptional": true,
                    "isVariadic": true,
                    "generators": toolVersionGenerator
                },
                {
                    "name": "command",
                    "description": "Command string to execute (same as --command)",
                    "isOptional": true,
                    "isVariadic": true
                }
            ]
        },
        {
            "name": [
                "fmt"
            ],
            "description": "Formats mise.toml",
            "options": [
                {
                    "name": [
                        "-a",
                        "--all"
                    ],
                    "description": "Format all files from the current directory",
                    "isRepeatable": false
                }
            ]
        },
        {
            "name": [
                "generate",
                "gen"
            ],
            "description": "[experimental] Generate files for various tools/services",
            "subcommands": [
                {
                    "name": [
                        "git-pre-commit",
                        "pre-commit"
                    ],
                    "description": "[experimental] Generate a git pre-commit hook",
                    "options": [
                        {
                            "name": [
                                "--hook"
                            ],
                            "description": "Which hook to generate (saves to .git/hooks/$hook)",
                            "isRepeatable": false,
                            "args": {
                                "name": "hook",
                                "isOptional": false,
                                "isVariadic": false
                            }
                        },
                        {
                            "name": [
                                "-t",
                                "--task"
                            ],
                            "description": "The task to run when the pre-commit hook is triggered",
                            "isRepeatable": false,
                            "args": {
                                "name": "task",
                                "isOptional": false,
                                "isVariadic": false,
                                "generators": simpleTaskGenerator
                            }
                        },
                        {
                            "name": [
                                "-w",
                                "--write"
                            ],
                            "description": "write to .git/hooks/pre-commit and make it executable",
                            "isRepeatable": false
                        }
                    ]
                },
                {
                    "name": [
                        "github-action"
                    ],
                    "description": "[experimental] Generate a GitHub Action workflow file",
                    "options": [
                        {
                            "name": [
                                "--name"
                            ],
                            "description": "the name of the workflow to generate",
                            "isRepeatable": false,
                            "args": {
                                "name": "name",
                                "isOptional": false,
                                "isVariadic": false
                            }
                        },
                        {
                            "name": [
                                "-t",
                                "--task"
                            ],
                            "description": "The task to run when the workflow is triggered",
                            "isRepeatable": false,
                            "args": {
                                "name": "task",
                                "isOptional": false,
                                "isVariadic": false,
                                "generators": simpleTaskGenerator
                            }
                        },
                        {
                            "name": [
                                "-w",
                                "--write"
                            ],
                            "description": "write to .github/workflows/$name.yml",
                            "isRepeatable": false
                        }
                    ]
                },
                {
                    "name": [
                        "task-docs"
                    ],
                    "description": "Generate documentation for tasks in a project",
                    "options": [
                        {
                            "name": [
                                "-I",
                                "--index"
                            ],
                            "description": "write only an index of tasks, intended for use with `--multi`",
                            "isRepeatable": false
                        },
                        {
                            "name": [
                                "-i",
                                "--inject"
                            ],
                            "description": "inserts the documentation into an existing file",
                            "isRepeatable": false
                        },
                        {
                            "name": [
                                "-m",
                                "--multi"
                            ],
                            "description": "render each task as a separate document, requires `--output` to be a directory",
                            "isRepeatable": false
                        },
                        {
                            "name": [
                                "-o",
                                "--output"
                            ],
                            "description": "writes the generated docs to a file/directory",
                            "isRepeatable": false,
                            "args": {
                                "name": "output",
                                "isOptional": false,
                                "isVariadic": false
                            }
                        },
                        {
                            "name": [
                                "-r",
                                "--root"
                            ],
                            "description": "root directory to search for tasks",
                            "isRepeatable": false,
                            "args": {
                                "name": "root",
                                "isOptional": false,
                                "isVariadic": false
                            }
                        },
                        {
                            "name": [
                                "-s",
                                "--style"
                            ],
                            "isRepeatable": false,
                            "args": {
                                "name": "style",
                                "isOptional": false,
                                "isVariadic": false,
                                "suggestions": [
                                    "simple",
                                    "detailed"
                                ]
                            }
                        }
                    ]
                }
            ]
        },
        {
            "name": [
                "implode"
            ],
            "description": "Removes mise CLI and all related data",
            "options": [
                {
                    "name": [
                        "--config"
                    ],
                    "description": "Also remove config directory",
                    "isRepeatable": false
                },
                {
                    "name": [
                        "-n",
                        "--dry-run"
                    ],
                    "description": "List directories that would be removed without actually removing them",
                    "isRepeatable": false
                }
            ]
        },
        {
            "name": [
                "install",
                "i"
            ],
            "description": "Install a tool version",
            "options": [
                {
                    "name": [
                        "-f",
                        "--force"
                    ],
                    "description": "Force reinstall even if already installed",
                    "isRepeatable": false
                },
                {
                    "name": [
                        "-j",
                        "--jobs"
                    ],
                    "description": "Number of jobs to run in parallel\n[default: 4]",
                    "isRepeatable": false,
                    "args": {
                        "name": "jobs",
                        "isOptional": false,
                        "isVariadic": false
                    }
                },
                {
                    "name": [
                        "--raw"
                    ],
                    "description": "Directly pipe stdin/stdout/stderr from plugin to user Sets --jobs=1",
                    "isRepeatable": false
                },
                {
                    "name": [
                        "-v",
                        "--verbose"
                    ],
                    "description": "Show installation output",
                    "isRepeatable": true
                }
            ],
            "args": [
                {
                    "name": "tool@version",
                    "description": "Tool(s) to install e.g.: node@20",
                    "isOptional": true,
                    "isVariadic": true,
                    "generators": toolVersionGenerator
                }
            ]
        },
        {
            "name": [
                "latest"
            ],
            "description": "Gets the latest available version for a plugin",
            "options": [
                {
                    "name": [
                        "-i",
                        "--installed"
                    ],
                    "description": "Show latest installed instead of available version",
                    "isRepeatable": false
                }
            ],
            "args": [
                {
                    "name": "tool@version",
                    "description": "Tool to get the latest version of",
                    "isOptional": false,
                    "isVariadic": false,
                    "generators": toolVersionGenerator
                }
            ]
        },
        {
            "name": [
                "link",
                "ln"
            ],
            "description": "Symlinks a tool version into mise",
            "options": [
                {
                    "name": [
                        "-f",
                        "--force"
                    ],
                    "description": "Overwrite an existing tool version if it exists",
                    "isRepeatable": false
                }
            ],
            "args": [
                {
                    "name": "tool@version",
                    "description": "Tool name and version to create a symlink for",
                    "isOptional": false,
                    "isVariadic": false,
                    "generators": toolVersionGenerator
                },
                {
                    "name": "path",
                    "description": "The local path to the tool version\ne.g.: ~/.nvm/versions/node/v20.0.0",
                    "isOptional": false,
                    "isVariadic": false,
                    "template": "filepaths"
                }
            ]
        },
        {
            "name": [
                "ls",
                "list"
            ],
            "description": "List installed and active tool versions",
            "options": [
                {
                    "name": [
                        "-c",
                        "--current"
                    ],
                    "description": "Only show tool versions currently specified in a mise.toml",
                    "isRepeatable": false
                },
                {
                    "name": [
                        "-g",
                        "--global"
                    ],
                    "description": "Only show tool versions currently specified in the global mise.toml",
                    "isRepeatable": false
                },
                {
                    "name": [
                        "-i",
                        "--installed"
                    ],
                    "description": "Only show tool versions that are installed (Hides tools defined in mise.toml but not installed)",
                    "isRepeatable": false
                },
                {
                    "name": [
                        "-o",
                        "--offline"
                    ],
                    "description": "Don't fetch information such as outdated versions",
                    "isRepeatable": false
                },
                {
                    "name": [
                        "--outdated"
                    ],
                    "description": "Display whether a version is outdated",
                    "isRepeatable": false
                },
                {
                    "name": [
                        "-J",
                        "--json"
                    ],
                    "description": "Output in JSON format",
                    "isRepeatable": false
                },
                {
                    "name": [
                        "-m",
                        "--missing"
                    ],
                    "description": "Display missing tool versions",
                    "isRepeatable": false
                },
                {
                    "name": [
                        "--prefix"
                    ],
                    "description": "Display versions matching this prefix",
                    "isRepeatable": false,
                    "args": {
                        "name": "prefix",
                        "isOptional": false,
                        "isVariadic": false,
                        "generators": completionGeneratorTemplate(`mise ls-remote {{words[PREV]}}`)
                    }
                },
                {
                    "name": [
                        "--no-header"
                    ],
                    "description": "Don't display headers",
                    "isRepeatable": false
                }
            ],
            "args": [
                {
                    "name": "plugin",
                    "description": "Only show tool versions from [PLUGIN]",
                    "isOptional": true,
                    "isVariadic": true,
                    "generators": pluginGenerator
                }
            ]
        },
        {
            "name": [
                "ls-remote"
            ],
            "description": "List runtime versions available for install.",
            "options": [
                {
                    "name": [
                        "--all"
                    ],
                    "description": "Show all installed plugins and versions",
                    "isRepeatable": false
                }
            ],
            "args": [
                {
                    "name": "tool@version",
                    "description": "Tool to get versions for",
                    "isOptional": true,
                    "isVariadic": false,
                    "generators": toolVersionGenerator
                },
                {
                    "name": "prefix",
                    "description": "The version prefix to use when querying the latest version\nsame as the first argument after the \"@\"",
                    "isOptional": true,
                    "isVariadic": false,
                    "generators": completionGeneratorTemplate(`mise ls-remote {{words[PREV]}}`)
                }
            ]
        },
        {
            "name": [
                "outdated"
            ],
            "description": "Shows outdated tool versions",
            "options": [
                {
                    "name": [
                        "-l",
                        "--bump"
                    ],
                    "description": "Compares against the latest versions available, not what matches the current config",
                    "isRepeatable": false
                },
                {
                    "name": [
                        "-J",
                        "--json"
                    ],
                    "description": "Output in JSON format",
                    "isRepeatable": false
                },
                {
                    "name": [
                        "--no-header"
                    ],
                    "description": "Don't show table header",
                    "isRepeatable": false
                }
            ],
            "args": [
                {
                    "name": "tool@version",
                    "description": "Tool(s) to show outdated versions for\ne.g.: node@20 python@3.10\nIf not specified, all tools in global and local configs will be shown",
                    "isOptional": true,
                    "isVariadic": true,
                    "generators": toolVersionGenerator
                }
            ]
        },
        {
            "name": [
                "plugins",
                "p"
            ],
            "description": "Manage plugins",
            "subcommands": [
                {
                    "name": [
                        "install",
                        "i",
                        "a",
                        "add"
                    ],
                    "description": "Install a plugin",
                    "options": [
                        {
                            "name": [
                                "-f",
                                "--force"
                            ],
                            "description": "Reinstall even if plugin exists",
                            "isRepeatable": false
                        },
                        {
                            "name": [
                                "-a",
                                "--all"
                            ],
                            "description": "Install all missing plugins\nThis will only install plugins that have matching shorthands.\ni.e.: they don't need the full git repo url",
                            "isRepeatable": false
                        },
                        {
                            "name": [
                                "-v",
                                "--verbose"
                            ],
                            "description": "Show installation output",
                            "isRepeatable": true
                        }
                    ],
                    "args": [
                        {
                            "name": "new_plugin",
                            "description": "The name of the plugin to install\ne.g.: node, ruby\nCan specify multiple plugins: `mise plugins install node ruby python`",
                            "isOptional": true,
                            "isVariadic": false,
                            "generators": completionGeneratorTemplate(`mise plugins --all`)
                        },
                        {
                            "name": "git_url",
                            "description": "The git url of the plugin",
                            "isOptional": true,
                            "isVariadic": false
                        }
                    ]
                },
                {
                    "name": [
                        "link",
                        "ln"
                    ],
                    "description": "Symlinks a plugin into mise",
                    "options": [
                        {
                            "name": [
                                "-f",
                                "--force"
                            ],
                            "description": "Overwrite existing plugin",
                            "isRepeatable": false
                        }
                    ],
                    "args": [
                        {
                            "name": "name",
                            "description": "The name of the plugin\ne.g.: node, ruby",
                            "isOptional": false,
                            "isVariadic": false
                        },
                        {
                            "name": "dir",
                            "description": "The local path to the plugin\ne.g.: ./mise-node",
                            "isOptional": true,
                            "isVariadic": false,
                            "template": "folders"
                        }
                    ]
                },
                {
                    "name": [
                        "ls",
                        "list"
                    ],
                    "description": "List installed plugins",
                    "options": [
                        {
                            "name": [
                                "-u",
                                "--urls"
                            ],
                            "description": "Show the git url for each plugin\ne.g.: https://github.com/asdf-vm/asdf-nodejs.git",
                            "isRepeatable": false
                        }
                    ]
                },
                {
                    "name": [
                        "ls-remote",
                        "list-remote",
                        "list-all"
                    ],
                    "description": "List all available remote plugins",
                    "options": [
                        {
                            "name": [
                                "-u",
                                "--urls"
                            ],
                            "description": "Show the git url for each plugin e.g.: https://github.com/mise-plugins/mise-poetry.git",
                            "isRepeatable": false
                        },
                        {
                            "name": [
                                "--only-names"
                            ],
                            "description": "Only show the name of each plugin by default it will show a \"*\" next to installed plugins",
                            "isRepeatable": false
                        }
                    ]
                },
                {
                    "name": [
                        "uninstall",
                        "remove",
                        "rm"
                    ],
                    "description": "Removes a plugin",
                    "options": [
                        {
                            "name": [
                                "-p",
                                "--purge"
                            ],
                            "description": "Also remove the plugin's installs, downloads, and cache",
                            "isRepeatable": false
                        },
                        {
                            "name": [
                                "-a",
                                "--all"
                            ],
                            "description": "Remove all plugins",
                            "isRepeatable": false
                        }
                    ],
                    "args": [
                        {
                            "name": "plugin",
                            "description": "Plugin(s) to remove",
                            "isOptional": true,
                            "isVariadic": true,
                            "generators": pluginGenerator
                        }
                    ]
                },
                {
                    "name": [
                        "update",
                        "up",
                        "upgrade"
                    ],
                    "description": "Updates a plugin to the latest version",
                    "options": [
                        {
                            "name": [
                                "-j",
                                "--jobs"
                            ],
                            "description": "Number of jobs to run in parallel\nDefault: 4",
                            "isRepeatable": false,
                            "args": {
                                "name": "jobs",
                                "isOptional": false,
                                "isVariadic": false
                            }
                        }
                    ],
                    "args": [
                        {
                            "name": "plugin",
                            "description": "Plugin(s) to update",
                            "isOptional": true,
                            "isVariadic": true,
                            "generators": pluginGenerator
                        }
                    ]
                }
            ],
            "options": [
                {
                    "name": [
                        "-c",
                        "--core"
                    ],
                    "description": "The built-in plugins only\nNormally these are not shown",
                    "isRepeatable": false
                },
                {
                    "name": [
                        "--user"
                    ],
                    "description": "List installed plugins",
                    "isRepeatable": false
                },
                {
                    "name": [
                        "-u",
                        "--urls"
                    ],
                    "description": "Show the git url for each plugin\ne.g.: https://github.com/asdf-vm/asdf-nodejs.git",
                    "isRepeatable": false
                }
            ]
        },
        {
            "name": [
                "prune"
            ],
            "description": "Delete unused versions of tools",
            "options": [
                {
                    "name": [
                        "-n",
                        "--dry-run"
                    ],
                    "description": "Do not actually delete anything",
                    "isRepeatable": false
                },
                {
                    "name": [
                        "--configs"
                    ],
                    "description": "Prune only tracked and trusted configuration links that point to non-existent configurations",
                    "isRepeatable": false
                },
                {
                    "name": [
                        "--tools"
                    ],
                    "description": "Prune only unused versions of tools",
                    "isRepeatable": false
                }
            ],
            "args": [
                {
                    "name": "plugin",
                    "description": "Prune only versions from this plugin(s)",
                    "isOptional": true,
                    "isVariadic": true,
                    "generators": pluginGenerator
                }
            ]
        },
        {
            "name": [
                "registry"
            ],
            "description": "List available tools to install",
            "options": [
                {
                    "name": [
                        "-b",
                        "--backend"
                    ],
                    "description": "Show only tools for this backend",
                    "isRepeatable": false,
                    "args": {
                        "name": "backend",
                        "isOptional": false,
                        "isVariadic": false
                    }
                },
                {
                    "name": [
                        "--hide-aliased"
                    ],
                    "description": "Hide aliased tools",
                    "isRepeatable": false
                }
            ],
            "args": [
                {
                    "name": "name",
                    "description": "Show only the specified tool's full name",
                    "isOptional": true,
                    "isVariadic": false
                }
            ]
        },
        {
            "name": [
                "reshim"
            ],
            "description": "Creates new shims based on bin paths from currently installed tools.",
            "options": [
                {
                    "name": [
                        "-f",
                        "--force"
                    ],
                    "description": "Removes all shims before reshimming",
                    "isRepeatable": false
                }
            ]
        },
        {
            "name": [
                "run",
                "r"
            ],
            "description": "Run task(s)",
            "options": [
                {
                    "name": [
                        "-C",
                        "--cd"
                    ],
                    "description": "Change to this directory before executing the command",
                    "isRepeatable": false,
                    "args": {
                        "name": "cd",
                        "isOptional": false,
                        "isVariadic": false
                    }
                },
                {
                    "name": [
                        "-n",
                        "--dry-run"
                    ],
                    "description": "Don't actually run the tasks(s), just print them in order of execution",
                    "isRepeatable": false
                },
                {
                    "name": [
                        "-f",
                        "--force"
                    ],
                    "description": "Force the tasks to run even if outputs are up to date",
                    "isRepeatable": false
                },
                {
                    "name": [
                        "-p",
                        "--prefix"
                    ],
                    "description": "Print stdout/stderr by line, prefixed with the tasks's label\nDefaults to true if --jobs > 1\nConfigure with `task_output` config or `MISE_TASK_OUTPUT` env var",
                    "isRepeatable": false
                },
                {
                    "name": [
                        "-i",
                        "--interleave"
                    ],
                    "description": "Print directly to stdout/stderr instead of by line\nDefaults to true if --jobs == 1\nConfigure with `task_output` config or `MISE_TASK_OUTPUT` env var",
                    "isRepeatable": false
                },
                {
                    "name": [
                        "-s",
                        "--shell"
                    ],
                    "description": "Shell to use to run toml tasks",
                    "isRepeatable": false,
                    "args": {
                        "name": "shell",
                        "isOptional": false,
                        "isVariadic": false
                    }
                },
                {
                    "name": [
                        "-t",
                        "--tool"
                    ],
                    "description": "Tool(s) to run in addition to what is in mise.toml files e.g.: node@20 python@3.10",
                    "isRepeatable": true,
                    "args": {
                        "name": "tool@version",
                        "isOptional": false,
                        "isVariadic": false,
                        "generators": toolVersionGenerator
                    }
                },
                {
                    "name": [
                        "-j",
                        "--jobs"
                    ],
                    "description": "Number of tasks to run in parallel\n[default: 4]\nConfigure with `jobs` config or `MISE_JOBS` env var",
                    "isRepeatable": false,
                    "args": {
                        "name": "jobs",
                        "isOptional": false,
                        "isVariadic": false
                    }
                },
                {
                    "name": [
                        "-r",
                        "--raw"
                    ],
                    "description": "Read/write directly to stdin/stdout/stderr instead of by line\nConfigure with `raw` config or `MISE_RAW` env var",
                    "isRepeatable": false
                },
                {
                    "name": [
                        "--no-timings"
                    ],
                    "description": "Hides elapsed time after each task completes",
                    "isRepeatable": false
                },
                {
                    "name": [
                        "-q",
                        "--quiet"
                    ],
                    "description": "Don't show extra output",
                    "isRepeatable": false
                }
            ],
            "generateSpec": usageGenerateSpec(["mise tasks --usage"]),
            "cache": false
        },
        {
            "name": [
                "self-update"
            ],
            "description": "Updates mise itself.",
            "options": [
                {
                    "name": [
                        "-f",
                        "--force"
                    ],
                    "description": "Update even if already up to date",
                    "isRepeatable": false
                },
                {
                    "name": [
                        "--no-plugins"
                    ],
                    "description": "Disable auto-updating plugins",
                    "isRepeatable": false
                },
                {
                    "name": [
                        "-y",
                        "--yes"
                    ],
                    "description": "Skip confirmation prompt",
                    "isRepeatable": false
                }
            ],
            "args": [
                {
                    "name": "version",
                    "description": "Update to a specific version",
                    "isOptional": true,
                    "isVariadic": false
                }
            ]
        },
        {
            "name": [
                "set"
            ],
            "description": "Set environment variables in mise.toml",
            "options": [
                {
                    "name": [
                        "--file"
                    ],
                    "description": "The TOML file to update",
                    "isRepeatable": false,
                    "args": {
                        "name": "file",
                        "isOptional": false,
                        "isVariadic": false,
                        "template": "filepaths"
                    }
                },
                {
                    "name": [
                        "-g",
                        "--global"
                    ],
                    "description": "Set the environment variable in the global config file",
                    "isRepeatable": false
                }
            ],
            "args": [
                {
                    "name": "env_vars",
                    "description": "Environment variable(s) to set\ne.g.: NODE_ENV=production",
                    "isOptional": true,
                    "isVariadic": true,
                    "generators": envVarGenerator
                }
            ]
        },
        {
            "name": [
                "settings"
            ],
            "description": "Manage settings",
            "subcommands": [
                {
                    "name": [
                        "add"
                    ],
                    "description": "Adds a setting to the configuration file",
                    "options": [
                        {
                            "name": [
                                "-l",
                                "--local"
                            ],
                            "description": "Use the local config file instead of the global one",
                            "isRepeatable": false
                        }
                    ],
                    "args": [
                        {
                            "name": "key",
                            "description": "The setting to set",
                            "isOptional": false,
                            "isVariadic": false
                        },
                        {
                            "name": "value",
                            "description": "The value to set",
                            "isOptional": false,
                            "isVariadic": false
                        }
                    ]
                },
                {
                    "name": [
                        "get"
                    ],
                    "description": "Show a current setting",
                    "options": [
                        {
                            "name": [
                                "-l",
                                "--local"
                            ],
                            "description": "Use the local config file instead of the global one",
                            "isRepeatable": false
                        }
                    ],
                    "args": [
                        {
                            "name": "key",
                            "description": "The setting to show",
                            "isOptional": false,
                            "isVariadic": false
                        }
                    ]
                },
                {
                    "name": [
                        "ls",
                        "list"
                    ],
                    "description": "Show current settings",
                    "options": [
                        {
                            "name": [
                                "-a",
                                "--all"
                            ],
                            "description": "Display settings set to the default",
                            "isRepeatable": false
                        },
                        {
                            "name": [
                                "-l",
                                "--local"
                            ],
                            "description": "Use the local config file instead of the global one",
                            "isRepeatable": false
                        },
                        {
                            "name": [
                                "-J",
                                "--json"
                            ],
                            "description": "Output in JSON format",
                            "isRepeatable": false
                        },
                        {
                            "name": [
                                "--json-extended"
                            ],
                            "description": "Output in JSON format with sources",
                            "isRepeatable": false
                        },
                        {
                            "name": [
                                "-T",
                                "--toml"
                            ],
                            "description": "Output in TOML format",
                            "isRepeatable": false
                        }
                    ],
                    "args": [
                        {
                            "name": "key",
                            "description": "List keys under this key",
                            "isOptional": true,
                            "isVariadic": false
                        }
                    ]
                },
                {
                    "name": [
                        "set",
                        "create"
                    ],
                    "description": "Add/update a setting",
                    "options": [
                        {
                            "name": [
                                "-l",
                                "--local"
                            ],
                            "description": "Use the local config file instead of the global one",
                            "isRepeatable": false
                        }
                    ],
                    "args": [
                        {
                            "name": "key",
                            "description": "The setting to set",
                            "isOptional": false,
                            "isVariadic": false
                        },
                        {
                            "name": "value",
                            "description": "The value to set",
                            "isOptional": false,
                            "isVariadic": false
                        }
                    ]
                },
                {
                    "name": [
                        "unset",
                        "rm",
                        "remove",
                        "delete",
                        "del"
                    ],
                    "description": "Clears a setting",
                    "options": [
                        {
                            "name": [
                                "-l",
                                "--local"
                            ],
                            "description": "Use the local config file instead of the global one",
                            "isRepeatable": false
                        }
                    ],
                    "args": [
                        {
                            "name": "key",
                            "description": "The setting to remove",
                            "isOptional": false,
                            "isVariadic": false
                        }
                    ]
                }
            ],
            "options": [
                {
                    "name": [
                        "-a",
                        "--all"
                    ],
                    "description": "List all settings",
                    "isRepeatable": false
                },
                {
                    "name": [
                        "-l",
                        "--local"
                    ],
                    "description": "Use the local config file instead of the global one",
                    "isRepeatable": false
                },
                {
                    "name": [
                        "-J",
                        "--json"
                    ],
                    "description": "Output in JSON format",
                    "isRepeatable": false
                },
                {
                    "name": [
                        "--json-extended"
                    ],
                    "description": "Output in JSON format with sources",
                    "isRepeatable": false
                },
                {
                    "name": [
                        "-T",
                        "--toml"
                    ],
                    "description": "Output in TOML format",
                    "isRepeatable": false
                }
            ],
            "args": [
                {
                    "name": "key",
                    "description": "Setting name to get/set",
                    "isOptional": true,
                    "isVariadic": false
                },
                {
                    "name": "value",
                    "description": "Setting value to set",
                    "isOptional": true,
                    "isVariadic": false
                }
            ]
        },
        {
            "name": [
                "shell",
                "sh"
            ],
            "description": "Sets a tool version for the current session.",
            "options": [
                {
                    "name": [
                        "-j",
                        "--jobs"
                    ],
                    "description": "Number of jobs to run in parallel\n[default: 4]",
                    "isRepeatable": false,
                    "args": {
                        "name": "jobs",
                        "isOptional": false,
                        "isVariadic": false
                    }
                },
                {
                    "name": [
                        "--raw"
                    ],
                    "description": "Directly pipe stdin/stdout/stderr from plugin to user Sets --jobs=1",
                    "isRepeatable": false
                },
                {
                    "name": [
                        "-u",
                        "--unset"
                    ],
                    "description": "Removes a previously set version",
                    "isRepeatable": false
                }
            ],
            "args": [
                {
                    "name": "tool@version",
                    "description": "Tool(s) to use",
                    "isOptional": true,
                    "isVariadic": true,
                    "generators": toolVersionGenerator
                }
            ]
        },
        {
            "name": [
                "sync"
            ],
            "description": "Add tool versions from external tools to mise",
            "subcommands": [
                {
                    "name": [
                        "node"
                    ],
                    "description": "Symlinks all tool versions from an external tool into mise",
                    "options": [
                        {
                            "name": [
                                "--brew"
                            ],
                            "description": "Get tool versions from Homebrew",
                            "isRepeatable": false
                        },
                        {
                            "name": [
                                "--nvm"
                            ],
                            "description": "Get tool versions from nvm",
                            "isRepeatable": false
                        },
                        {
                            "name": [
                                "--nodenv"
                            ],
                            "description": "Get tool versions from nodenv",
                            "isRepeatable": false
                        }
                    ]
                },
                {
                    "name": [
                        "python"
                    ],
                    "description": "Symlinks all tool versions from an external tool into mise",
                    "options": [
                        {
                            "name": [
                                "--pyenv"
                            ],
                            "description": "Get tool versions from pyenv",
                            "isRepeatable": false
                        }
                    ]
                }
            ]
        },
        {
            "name": [
                "tasks",
                "t"
            ],
            "description": "Manage tasks",
            "subcommands": [
                {
                    "name": [
                        "deps"
                    ],
                    "description": "Display a tree visualization of a dependency graph",
                    "options": [
                        {
                            "name": [
                                "--hidden"
                            ],
                            "description": "Show hidden tasks",
                            "isRepeatable": false
                        },
                        {
                            "name": [
                                "--dot"
                            ],
                            "description": "Display dependencies in DOT format",
                            "isRepeatable": false
                        }
                    ],
                    "args": [
                        {
                            "name": "tasks",
                            "description": "Tasks to show dependencies for\nCan specify multiple tasks by separating with spaces\ne.g.: mise tasks deps lint test check",
                            "isOptional": true,
                            "isVariadic": true
                        }
                    ]
                },
                {
                    "name": [
                        "edit"
                    ],
                    "description": "Edit a tasks with $EDITOR",
                    "options": [
                        {
                            "name": [
                                "-p",
                                "--path"
                            ],
                            "description": "Display the path to the tasks instead of editing it",
                            "isRepeatable": false
                        }
                    ],
                    "args": [
                        {
                            "name": "task",
                            "description": "Tasks to edit",
                            "isOptional": false,
                            "isVariadic": false,
                            "generators": simpleTaskGenerator
                        }
                    ]
                },
                {
                    "name": [
                        "info"
                    ],
                    "description": "Get information about a task",
                    "options": [
                        {
                            "name": [
                                "-J",
                                "--json"
                            ],
                            "description": "Output in JSON format",
                            "isRepeatable": false
                        }
                    ],
                    "args": [
                        {
                            "name": "task",
                            "description": "Name of the task to get information about",
                            "isOptional": false,
                            "isVariadic": false,
                            "generators": simpleTaskGenerator
                        }
                    ]
                },
                {
                    "name": [
                        "ls"
                    ],
                    "description": "List available tasks to execute\nThese may be included from the config file or from the project's .mise/tasks directory\nmise will merge all tasks from all parent directories into this list.",
                    "options": [
                        {
                            "name": [
                                "--no-header"
                            ],
                            "description": "Do not print table header",
                            "isRepeatable": false
                        },
                        {
                            "name": [
                                "-x",
                                "--extended"
                            ],
                            "description": "Show all columns",
                            "isRepeatable": false
                        },
                        {
                            "name": [
                                "--hidden"
                            ],
                            "description": "Show hidden tasks",
                            "isRepeatable": false
                        },
                        {
                            "name": [
                                "--sort"
                            ],
                            "description": "Sort by column. Default is name.",
                            "isRepeatable": false,
                            "args": {
                                "name": "column",
                                "isOptional": false,
                                "isVariadic": false,
                                "suggestions": [
                                    "name",
                                    "alias",
                                    "description",
                                    "source"
                                ]
                            }
                        },
                        {
                            "name": [
                                "--sort-order"
                            ],
                            "description": "Sort order. Default is asc.",
                            "isRepeatable": false,
                            "args": {
                                "name": "sort_order",
                                "isOptional": false,
                                "isVariadic": false,
                                "suggestions": [
                                    "asc",
                                    "desc"
                                ]
                            }
                        },
                        {
                            "name": [
                                "-J",
                                "--json"
                            ],
                            "description": "Output in JSON format",
                            "isRepeatable": false
                        }
                    ]
                },
                {
                    "name": [
                        "run",
                        "r"
                    ],
                    "description": "Run task(s)",
                    "options": [
                        {
                            "name": [
                                "-C",
                                "--cd"
                            ],
                            "description": "Change to this directory before executing the command",
                            "isRepeatable": false,
                            "args": {
                                "name": "cd",
                                "isOptional": false,
                                "isVariadic": false
                            }
                        },
                        {
                            "name": [
                                "-n",
                                "--dry-run"
                            ],
                            "description": "Don't actually run the tasks(s), just print them in order of execution",
                            "isRepeatable": false
                        },
                        {
                            "name": [
                                "-f",
                                "--force"
                            ],
                            "description": "Force the tasks to run even if outputs are up to date",
                            "isRepeatable": false
                        },
                        {
                            "name": [
                                "-p",
                                "--prefix"
                            ],
                            "description": "Print stdout/stderr by line, prefixed with the tasks's label\nDefaults to true if --jobs > 1\nConfigure with `task_output` config or `MISE_TASK_OUTPUT` env var",
                            "isRepeatable": false
                        },
                        {
                            "name": [
                                "-i",
                                "--interleave"
                            ],
                            "description": "Print directly to stdout/stderr instead of by line\nDefaults to true if --jobs == 1\nConfigure with `task_output` config or `MISE_TASK_OUTPUT` env var",
                            "isRepeatable": false
                        },
                        {
                            "name": [
                                "-s",
                                "--shell"
                            ],
                            "description": "Shell to use to run toml tasks",
                            "isRepeatable": false,
                            "args": {
                                "name": "shell",
                                "isOptional": false,
                                "isVariadic": false
                            }
                        },
                        {
                            "name": [
                                "-t",
                                "--tool"
                            ],
                            "description": "Tool(s) to run in addition to what is in mise.toml files e.g.: node@20 python@3.10",
                            "isRepeatable": true,
                            "args": {
                                "name": "tool@version",
                                "isOptional": false,
                                "isVariadic": false,
                                "generators": toolVersionGenerator
                            }
                        },
                        {
                            "name": [
                                "-j",
                                "--jobs"
                            ],
                            "description": "Number of tasks to run in parallel\n[default: 4]\nConfigure with `jobs` config or `MISE_JOBS` env var",
                            "isRepeatable": false,
                            "args": {
                                "name": "jobs",
                                "isOptional": false,
                                "isVariadic": false
                            }
                        },
                        {
                            "name": [
                                "-r",
                                "--raw"
                            ],
                            "description": "Read/write directly to stdin/stdout/stderr instead of by line\nConfigure with `raw` config or `MISE_RAW` env var",
                            "isRepeatable": false
                        },
                        {
                            "name": [
                                "--no-timings"
                            ],
                            "description": "Hides elapsed time after each task completes",
                            "isRepeatable": false
                        },
                        {
                            "name": [
                                "-q",
                                "--quiet"
                            ],
                            "description": "Don't show extra output",
                            "isRepeatable": false
                        }
                    ],
                    "args": [
                        {
                            "name": "task",
                            "description": "Tasks to run\nCan specify multiple tasks by separating with `:::`\ne.g.: mise run task1 arg1 arg2 ::: task2 arg1 arg2",
                            "isOptional": true,
                            "isVariadic": false,
                            "generators": simpleTaskGenerator
                        },
                        {
                            "name": "args",
                            "description": "Arguments to pass to the tasks. Use \":::\" to separate tasks",
                            "isOptional": true,
                            "isVariadic": true
                        }
                    ],
                    "generateSpec": usageGenerateSpec(["mise tasks --usage"]),
                    "cache": false
                }
            ],
            "options": [
                {
                    "name": [
                        "--no-header"
                    ],
                    "description": "Do not print table header",
                    "isRepeatable": false
                },
                {
                    "name": [
                        "-x",
                        "--extended"
                    ],
                    "description": "Show all columns",
                    "isRepeatable": false
                },
                {
                    "name": [
                        "--hidden"
                    ],
                    "description": "Show hidden tasks",
                    "isRepeatable": false
                },
                {
                    "name": [
                        "--sort"
                    ],
                    "description": "Sort by column. Default is name.",
                    "isRepeatable": false,
                    "args": {
                        "name": "column",
                        "isOptional": false,
                        "isVariadic": false,
                        "suggestions": [
                            "name",
                            "alias",
                            "description",
                            "source"
                        ]
                    }
                },
                {
                    "name": [
                        "--sort-order"
                    ],
                    "description": "Sort order. Default is asc.",
                    "isRepeatable": false,
                    "args": {
                        "name": "sort_order",
                        "isOptional": false,
                        "isVariadic": false,
                        "suggestions": [
                            "asc",
                            "desc"
                        ]
                    }
                },
                {
                    "name": [
                        "-J",
                        "--json"
                    ],
                    "description": "Output in JSON format",
                    "isRepeatable": false
                }
            ],
            "args": [
                {
                    "name": "task",
                    "description": "Task name to get info of",
                    "isOptional": true,
                    "isVariadic": false,
                    "generators": simpleTaskGenerator
                }
            ]
        },
        {
            "name": [
                "tool"
            ],
            "description": "Gets information about a tool",
            "options": [
                {
                    "name": [
                        "-J",
                        "--json"
                    ],
                    "description": "Output in JSON format",
                    "isRepeatable": false
                },
                {
                    "name": [
                        "--backend"
                    ],
                    "description": "Only show backend field",
                    "isRepeatable": false
                },
                {
                    "name": [
                        "--installed"
                    ],
                    "description": "Only show installed versions",
                    "isRepeatable": false
                },
                {
                    "name": [
                        "--active"
                    ],
                    "description": "Only show active versions",
                    "isRepeatable": false
                },
                {
                    "name": [
                        "--requested"
                    ],
                    "description": "Only show requested versions",
                    "isRepeatable": false
                },
                {
                    "name": [
                        "--config-source"
                    ],
                    "description": "Only show config source",
                    "isRepeatable": false
                },
                {
                    "name": [
                        "--tool-options"
                    ],
                    "description": "Only show tool options",
                    "isRepeatable": false
                }
            ],
            "args": [
                {
                    "name": "backend",
                    "description": "Tool name to get information about",
                    "isOptional": false,
                    "isVariadic": false
                }
            ]
        },
        {
            "name": [
                "trust"
            ],
            "description": "Marks a config file as trusted",
            "options": [
                {
                    "name": [
                        "-a",
                        "--all"
                    ],
                    "description": "Trust all config files in the current directory and its parents",
                    "isRepeatable": false
                },
                {
                    "name": [
                        "--ignore"
                    ],
                    "description": "Do not trust this config and ignore it in the future",
                    "isRepeatable": false
                },
                {
                    "name": [
                        "--untrust"
                    ],
                    "description": "No longer trust this config, will prompt in the future",
                    "isRepeatable": false
                },
                {
                    "name": [
                        "--show"
                    ],
                    "description": "Show the trusted status of config files from the current directory and its parents.\nDoes not trust or untrust any files.",
                    "isRepeatable": false
                }
            ],
            "args": [
                {
                    "name": "config_file",
                    "description": "The config file to trust",
                    "isOptional": true,
                    "isVariadic": false,
                    "template": "filepaths",
                    "generators": configPathGenerator
                }
            ]
        },
        {
            "name": [
                "uninstall",
                "remove",
                "rm"
            ],
            "description": "Removes installed tool versions",
            "options": [
                {
                    "name": [
                        "-a",
                        "--all"
                    ],
                    "description": "Delete all installed versions",
                    "isRepeatable": false
                },
                {
                    "name": [
                        "-n",
                        "--dry-run"
                    ],
                    "description": "Do not actually delete anything",
                    "isRepeatable": false
                }
            ],
            "args": [
                {
                    "name": "installed_tool@version",
                    "description": "Tool(s) to remove",
                    "isOptional": true,
                    "isVariadic": true,
                    "generators": installedToolVersionGenerator
                }
            ]
        },
        {
            "name": [
                "unset"
            ],
            "description": "Remove environment variable(s) from the config file.",
            "options": [
                {
                    "name": [
                        "-f",
                        "--file"
                    ],
                    "description": "Specify a file to use instead of `mise.toml`",
                    "isRepeatable": false,
                    "args": {
                        "name": "file",
                        "isOptional": false,
                        "isVariadic": false,
                        "template": "filepaths"
                    }
                },
                {
                    "name": [
                        "-g",
                        "--global"
                    ],
                    "description": "Use the global config file",
                    "isRepeatable": false
                }
            ],
            "args": [
                {
                    "name": "keys",
                    "description": "Environment variable(s) to remove\ne.g.: NODE_ENV",
                    "isOptional": true,
                    "isVariadic": true
                }
            ]
        },
        {
            "name": [
                "upgrade",
                "up"
            ],
            "description": "Upgrades outdated tools",
            "options": [
                {
                    "name": [
                        "-n",
                        "--dry-run"
                    ],
                    "description": "Just print what would be done, don't actually do it",
                    "isRepeatable": false
                },
                {
                    "name": [
                        "-i",
                        "--interactive"
                    ],
                    "description": "Display multiselect menu to choose which tools to upgrade",
                    "isRepeatable": false
                },
                {
                    "name": [
                        "-j",
                        "--jobs"
                    ],
                    "description": "Number of jobs to run in parallel\n[default: 4]",
                    "isRepeatable": false,
                    "args": {
                        "name": "jobs",
                        "isOptional": false,
                        "isVariadic": false
                    }
                },
                {
                    "name": [
                        "-l",
                        "--bump"
                    ],
                    "description": "Upgrades to the latest version available, bumping the version in mise.toml",
                    "isRepeatable": false
                },
                {
                    "name": [
                        "--raw"
                    ],
                    "description": "Directly pipe stdin/stdout/stderr from plugin to user Sets --jobs=1",
                    "isRepeatable": false
                }
            ],
            "args": [
                {
                    "name": "tool@version",
                    "description": "Tool(s) to upgrade\ne.g.: node@20 python@3.10\nIf not specified, all current tools will be upgraded",
                    "isOptional": true,
                    "isVariadic": true,
                    "generators": toolVersionGenerator
                }
            ]
        },
        {
            "name": [
                "use",
                "u"
            ],
            "description": "Installs a tool and adds the version to mise.toml.",
            "options": [
                {
                    "name": [
                        "-f",
                        "--force"
                    ],
                    "description": "Force reinstall even if already installed",
                    "isRepeatable": false
                },
                {
                    "name": [
                        "--fuzzy"
                    ],
                    "description": "Save fuzzy version to config file",
                    "isRepeatable": false
                },
                {
                    "name": [
                        "-g",
                        "--global"
                    ],
                    "description": "Use the global config file (`~/.config/mise/config.toml`) instead of the local one",
                    "isRepeatable": false
                },
                {
                    "name": [
                        "-e",
                        "--env"
                    ],
                    "description": "Create/modify an environment-specific config file like .mise.<env>.toml",
                    "isRepeatable": false,
                    "args": {
                        "name": "env",
                        "isOptional": false,
                        "isVariadic": false
                    }
                },
                {
                    "name": [
                        "-j",
                        "--jobs"
                    ],
                    "description": "Number of jobs to run in parallel\n[default: 4]",
                    "isRepeatable": false,
                    "args": {
                        "name": "jobs",
                        "isOptional": false,
                        "isVariadic": false
                    }
                },
                {
                    "name": [
                        "--raw"
                    ],
                    "description": "Directly pipe stdin/stdout/stderr from plugin to user Sets `--jobs=1`",
                    "isRepeatable": false
                },
                {
                    "name": [
                        "--remove"
                    ],
                    "description": "Remove the plugin(s) from config file",
                    "isRepeatable": true,
                    "args": {
                        "name": "plugin",
                        "isOptional": false,
                        "isVariadic": false,
                        "generators": pluginGenerator
                    }
                },
                {
                    "name": [
                        "-p",
                        "--path"
                    ],
                    "description": "Specify a path to a config file or directory",
                    "isRepeatable": false,
                    "args": {
                        "name": "path",
                        "isOptional": false,
                        "isVariadic": false,
                        "template": "filepaths"
                    }
                },
                {
                    "name": [
                        "--pin"
                    ],
                    "description": "Save exact version to config file\ne.g.: `mise use --pin node@20` will save 20.0.0 as the version\nSet `MISE_PIN=1` to make this the default behavior",
                    "isRepeatable": false
                }
            ],
            "args": [
                {
                    "name": "tool@version",
                    "description": "Tool(s) to add to config file",
                    "isOptional": true,
                    "isVariadic": true,
                    "generators": toolVersionGenerator
                }
            ]
        },
        {
            "name": [
                "version",
                "v"
            ],
            "description": "Display the version of mise"
        },
        {
            "name": [
                "watch",
                "w"
            ],
            "description": "Run task(s) and watch for changes to rerun it",
            "options": [
                {
                    "name": [
                        "-t",
                        "--task"
                    ],
                    "description": "Tasks to run",
                    "isRepeatable": true,
                    "args": {
                        "name": "task",
                        "isOptional": false,
                        "isVariadic": false,
                        "generators": simpleTaskGenerator
                    }
                },
                {
                    "name": [
                        "-g",
                        "--glob"
                    ],
                    "description": "Files to watch\nDefaults to sources from the tasks(s)",
                    "isRepeatable": true,
                    "args": {
                        "name": "glob",
                        "isOptional": false,
                        "isVariadic": false
                    }
                }
            ],
            "args": [
                {
                    "name": "args",
                    "description": "Extra arguments",
                    "isOptional": true,
                    "isVariadic": true
                }
            ]
        },
        {
            "name": [
                "where"
            ],
            "description": "Display the installation path for a tool",
            "args": [
                {
                    "name": "tool@version",
                    "description": "Tool(s) to look up\ne.g.: ruby@3\nif \"@<PREFIX>\" is specified, it will show the latest installed version\nthat matches the prefix\notherwise, it will show the current, active installed version",
                    "isOptional": false,
                    "isVariadic": false,
                    "generators": toolVersionGenerator
                }
            ]
        },
        {
            "name": [
                "which"
            ],
            "description": "Shows the path that a tool's bin points to.",
            "options": [
                {
                    "name": [
                        "--plugin"
                    ],
                    "description": "Show the plugin name instead of the path",
                    "isRepeatable": false
                },
                {
                    "name": [
                        "--version"
                    ],
                    "description": "Show the version instead of the path",
                    "isRepeatable": false
                },
                {
                    "name": [
                        "-t",
                        "--tool"
                    ],
                    "description": "Use a specific tool@version\ne.g.: `mise which npm --tool=node@20`",
                    "isRepeatable": false,
                    "args": {
                        "name": "tool@version",
                        "isOptional": false,
                        "isVariadic": false,
                        "generators": toolVersionGenerator
                    }
                }
            ],
            "args": [
                {
                    "name": "bin_name",
                    "description": "The bin to look up",
                    "isOptional": false,
                    "isVariadic": false
                }
            ]
        }
    ],
    "options": [
        {
            "name": [
                "-C",
                "--cd"
            ],
            "description": "Change directory before running command",
            "isRepeatable": false,
            "args": {
                "name": "dir",
                "isOptional": false,
                "isVariadic": false,
                "template": "folders"
            }
        },
        {
            "name": [
                "-E",
                "--env"
            ],
            "description": "Set the environment for loading `mise.<ENV>.toml`",
            "isRepeatable": true,
            "args": {
                "name": "env",
                "isOptional": false,
                "isVariadic": false
            }
        },
        {
            "name": [
                "-j",
                "--jobs"
            ],
            "description": "How many jobs to run in parallel [default: 8]",
            "isRepeatable": false,
            "args": {
                "name": "jobs",
                "isOptional": false,
                "isVariadic": false
            }
        },
        {
            "name": [
                "-q",
                "--quiet"
            ],
            "description": "Suppress non-error messages",
            "isRepeatable": false
        },
        {
            "name": [
                "--raw"
            ],
            "description": "Read/write directly to stdin/stdout/stderr instead of by line",
            "isRepeatable": false
        },
        {
            "name": [
                "-v",
                "--verbose"
            ],
            "description": "Show extra output (use -vv for even more)",
            "isRepeatable": true
        },
        {
            "name": [
                "-y",
                "--yes"
            ],
            "description": "Answer yes to all confirmation prompts",
            "isRepeatable": false
        }
    ],
    "args": [
        {
            "name": "task",
            "description": "Task to run",
            "isOptional": true,
            "isVariadic": false,
            "generators": simpleTaskGenerator
        }
    ]
};
export default completionSpec;
