// If not being published, these need to manually downloaded from https://github.com/withfig/autocomplete/tree/master/src
/* eslint-disable @withfig/fig-linter/conventional-descriptions */
import { createNpmSearchHandler } from "./npm";
import { searchGenerator as createCargoSearchGenerator } from "./cargo";

const singleCmdNewLineGenerator = (completion_cmd: string): Fig.Generator => ({
  script: completion_cmd.split(" "),
  splitOn: "\n",
});

const singleCmdJsonGenerator = (cmd: string): Fig.Generator => ({
  script: cmd.split(" "),
  postProcess: (out) =>
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
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
  executeShellCommand: Fig.ExecuteCommandFunction
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
      })
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
  key: keyof T
): Record<T[keyof T], T[]> {
  return array.reduce(
    (result, currentItem) => {
      (result[currentItem[key] as ObjectKeyType] =
        result[currentItem[key] as ObjectKeyType] || []).push(currentItem);
      return result;
    },
    {} as Record<ObjectKeyType, T[]>
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
        })
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

type GitHubRepoInfo = {
  name: string;
  full_name: string;
  description: string;
};

type GitHubAssetInfo = {
  url: string;
  uploader: object;
  download_count: number;
  state: string;
};
type GitHubVersionInfo = {
  assets: string[];
  tag_name: string;
  draft: boolean;
  body: string; // Markdown
};

const searchGitHub = async (
  package_name: string,
  executeShellCommand: Fig.ExecuteCommandFunction,
  shellContext: Fig.GeneratorContext
): Promise<Fig.Suggestion[]> => {
  const query = [
    "-H",
    "Accept: application/vnd.github+json",
    "-H",
    "X-GitHub-Api-Version: 2022-11-28",
  ];

  const generalUrl =
    "https://api.github.com/search/repositories?q=$NAME$+in:name";
  const versionsUrl = "https://api.github.com/repos/$FULL_NAME$/releases";

  try {
    const envs = (
      await executeShellCommand({
        command: envVarGenerator.script[0],
        args: envVarGenerator.script.slice(1),
      })
    ).stdout
      .split("\n")
      .map((l) => ({
        name: l.split("=")[0].trim(),
        value: l.split("=")[1].trim(),
      }));

    const gh_token = envs.find((v) => v.name == "GITHUB_TOKEN");
    if (gh_token) {
      query.push("-H");
      query.push("Authorization: Bearer $TOKEN$");
      query[query.length - 1] = query[query.length - 1].replace(
        "$TOKEN$",
        gh_token.value
      );
    }

    const url =
      package_name[package_name.length - 1] === "@" ? versionsUrl : generalUrl;
    query.push(url);
    query[query.length - 1] = query[query.length - 1].replace(
      "$NAME$",
      package_name
    );
    query[query.length - 1] = query[query.length - 1].replace(
      "$FULL_NAME$",
      package_name.slice(0, package_name.length - 1)
    );

    const { stdout } = await executeShellCommand({
      command: "curl",
      args: query,
    });

    if (package_name[package_name.length - 1] === "@") {
      const package_real_name = package_name.slice(0, package_name.length - 1);
      return [
        ...new Set(
          (JSON.parse(stdout) as GitHubVersionInfo[])
            .filter((e) => e.assets.length > 0)
            .slice(0, 200)
            .map((e) => ({
              name: `${package_real_name}@${e.tag_name}`,
              description: e.body,
            }))
        ),
      ];
    } else {
      return [
        ...new Set(
          (JSON.parse(stdout).items as GitHubRepoInfo[]).slice(0, 200).map(
            (entry) =>
              ({
                name: entry.full_name,
                displayName: entry.name,
                description: entry.description,
              }) as Fig.Suggestion
          )
        ),
      ];
    }
  } catch (error) {
    return [{ name: "error", description: error as string }];
  }
};

const searchBackend = async (
  backend: string,
  context: string[],
  executeShellCommand: Fig.ExecuteCommandFunction,
  shellContext: Fig.GeneratorContext
): Promise<Fig.Suggestion[]> => {
  const customContext = context;
  customContext[context.length - 1] = customContext[context.length - 1].replace(
    `${backend}:`,
    ""
  );
  switch (backend) {
    case "npm":
      return await createNpmSearchHandler()(
        context,
        executeShellCommand,
        shellContext
      );
    case "cargo":
      return await createCargoSearchGenerator.custom(
        customContext,
        executeShellCommand,
        shellContext
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
          })
        ),
      ];
    case "ubi":
      return await searchGitHub(
        customContext[customContext.length - 1],
        executeShellCommand,
        shellContext
      );
    default:
      return [];
  }
};

const compareVersions = (a: string, b: string): number => {
  const result = [a, b].sort(); // Unless we can add semversort
  if (result[0] != a) return 1;
  return -1;
};

const getBackends = async (
  executeShellCommand: Fig.ExecuteCommandFunction
): Promise<string[]> => {
  const { stdout, stderr, status } = await executeShellCommand({
    command: "sh",
    args: ["-c", "mise backends ls"],
  });
  if (status != 0) {
    return [stderr];
  }
  return [stdout];
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
    shellContext: Fig.GeneratorContext
  ): Promise<Fig.Suggestion[]> => {
    const currentWord = context[context.length - 1];
    if (backendSepInStr(currentWord)) {
      // Let's handle backends
      const backend = currentWord.slice(0, currentWord.lastIndexOf(":"));

      return (
        await searchBackend(backend, context, executeShellCommand, shellContext)
      ).map((s) => ({
        ...s,
        name: `${backend}:${s.name}`,
        displayName: s.name as string,
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

    const { stdout: registryStdout } = await executeShellCommand({
      command: "sh",
      args: ["-c", "mise registry"],
    });
    const registrySuggestions = [
      ...new Set(
        registryStdout.split("\n").map((l) => {
          const tokens = l.split(/\s+/);
          return { name: tokens[0], description: tokens[1] };
        })
      ),
    ];

    const backendSuggestions = (await getBackends(executeShellCommand)).map(
      (backend) => ({ name: backend, description: "Backend" })
    );
    return [...backendSuggestions, ...registrySuggestions];
  },
};

const installedToolVersionGenerator: Fig.Generator = {
  trigger: "@",
  getQueryTerm: "@",
  custom: async (
    context: string[],
    executeShellCommand: Fig.ExecuteCommandFunction
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

const envVarGenerator = {
  script: ["sh", "-c", "env"],
  postProcess: (output: string) => {
    return output.split("\n").map((l) => ({ name: l.split("=")[0] }));
  },
};
const usageGenerateSpec = (cmds: string[]) => {
  return async (
    context: string[],
    executeCommand: Fig.ExecuteCommandFunction
  ): Promise<Fig.Spec> => {
    const promises = cmds.map(async (cmd): Promise<Fig.Subcommand[]> => {
      try {
        const args = cmd.split(" ");
        const {
          stdout,
          stderr: cmdStderr,
          status: cmdStatus,
        } = await executeCommand({
          command: args[0],
          args: args.splice(1),
        });
        if (cmdStatus !== 0) {
          return [{ name: "error", description: cmdStderr }];
        }
        const {
          stdout: figSpecOut,
          stderr: figSpecStderr,
          status: usageFigStatus,
        } = await executeCommand({
          command: "usage",
          args: ["g", "fig", "--spec", stdout],
        });
        if (usageFigStatus !== 0) {
          return [{ name: "error", description: figSpecStderr }];
        }
        const start_of_json = figSpecOut.indexOf("{");
        const j = figSpecOut.slice(start_of_json);
        return JSON.parse(j).subcommands as Fig.Subcommand[];
      } catch (e) {
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
const completionGeneratorTemplate = (
  argSuggestionBash: string
): Fig.Generator => {
  return {
    custom: async (tokens: string[], executeCommand) => {
      let arg = argSuggestionBash;
      if (tokens.length >= 1) {
        arg = argSuggestionBash.replace(
          "{{words[CURRENT]}}",
          tokens[tokens.length - 1]
        );
      }
      if (tokens.length >= 2) {
        arg = arg.replace(`{{words[PREV]}}`, tokens[tokens.length - 2]);
      }
      const { stdout: text } = await executeCommand({
        command: "sh",
        args: ["-c", arg],
      });
      if (text.trim().length == 0) return [];
      return text.split("\n").map((elm) => ({ name: elm }));
    },
  };
};
const completionSpec: Fig.Spec = {
  name: "mise",
  subcommands: [
    {
      name: "activate",
      description: "Initializes mise in the current shell session",
      options: [
        {
          name: "--shims",
          description:
            "Use shims instead of modifying PATH\nEffectively the same as:",
          isRepeatable: false,
        },
        {
          name: ["-q", "--quiet"],
          description: "Suppress non-error messages",
          isRepeatable: false,
        },
        {
          name: "--no-hook-env",
          description: "Do not automatically call hook-env",
          isRepeatable: false,
        },
      ],
      args: {
        name: "shell_type",
        description: "Shell type to generate the script for",
        isOptional: true,
        suggestions: ["bash", "elvish", "fish", "nu", "xonsh", "zsh", "pwsh"],
      },
    },
    {
      name: ["alias", "a"],
      description: "Manage version aliases.",
      subcommands: [
        {
          name: "get",
          description: "Show an alias for a plugin",
          args: [
            {
              name: "plugin",
              description: "The plugin to show the alias for",
              generators: pluginGenerator,
              debounce: true,
            },
            {
              name: "alias",
              description: "The alias to show",
              generators: aliasGenerator,
              debounce: true,
            },
          ],
        },
        {
          name: ["ls", "list"],
          description:
            "List aliases\nShows the aliases that can be specified.\nThese can come from user config or from plugins in `bin/list-aliases`.",
          options: [
            {
              name: "--no-header",
              description: "Don't show table header",
              isRepeatable: false,
            },
          ],
          args: {
            name: "tool",
            description: "Show aliases for <TOOL>",
            isOptional: true,
            generators: completionGeneratorTemplate(`mise registry --complete`),
            debounce: true,
          },
        },
        {
          name: ["set", "add", "create"],
          description: "Add/update an alias for a plugin",
          args: [
            {
              name: "plugin",
              description: "The plugin to set the alias for",
              generators: pluginGenerator,
              debounce: true,
            },
            {
              name: "alias",
              description: "The alias to set",
              generators: aliasGenerator,
              debounce: true,
            },
            {
              name: "value",
              description: "The value to set the alias to",
            },
          ],
        },
        {
          name: ["unset", "rm", "remove", "delete", "del"],
          description: "Clears an alias for a plugin",
          args: [
            {
              name: "plugin",
              description: "The plugin to remove the alias from",
              generators: pluginGenerator,
              debounce: true,
            },
            {
              name: "alias",
              description: "The alias to remove",
              generators: aliasGenerator,
              debounce: true,
            },
          ],
        },
      ],
      options: [
        {
          name: ["-p", "--plugin"],
          description: "Filter aliases by plugin",
          isRepeatable: false,
          args: {
            name: "plugin",
            generators: pluginGenerator,
            debounce: true,
          },
        },
        {
          name: "--no-header",
          description: "Don't show table header",
          isRepeatable: false,
        },
      ],
    },
    {
      name: ["backends", "b"],
      description: "Manage backends",
      subcommands: [
        {
          name: ["ls", "list"],
          description: "List built-in backends",
        },
      ],
    },
    {
      name: "bin-paths",
      description: "List all the active runtime bin paths",
      args: {
        name: "tool@version",
        description: "Tool(s) to look up\ne.g.: ruby@3",
        isOptional: true,
        isVariadic: true,
        generators: toolVersionGenerator,
        debounce: true,
      },
    },
    {
      name: "cache",
      description: "Manage the mise cache",
      subcommands: [
        {
          name: ["clear", "c"],
          description: "Deletes all cache files in mise",
          args: {
            name: "plugin",
            description: "Plugin(s) to clear cache for e.g.: node, python",
            isOptional: true,
            isVariadic: true,
            generators: pluginGenerator,
            debounce: true,
          },
        },
        {
          name: ["prune", "p"],
          description: "Removes stale mise cache files",
          options: [
            {
              name: "--dry-run",
              description: "Just show what would be pruned",
              isRepeatable: false,
            },
            {
              name: ["-v", "--verbose"],
              description: "Show pruned files",
              isRepeatable: true,
            },
          ],
          args: {
            name: "plugin",
            description: "Plugin(s) to clear cache for e.g.: node, python",
            isOptional: true,
            isVariadic: true,
            generators: pluginGenerator,
            debounce: true,
          },
        },
      ],
    },
    {
      name: "completion",
      description: "Generate shell completions",
      options: [
        {
          name: "--include-bash-completion-lib",
          description:
            "Include the bash completion library in the bash completion script",
          isRepeatable: false,
        },
      ],
      args: {
        name: "shell",
        description: "Shell type to generate completions for",
        isOptional: true,
        suggestions: ["bash", "fish", "zsh"],
      },
    },
    {
      name: ["config", "cfg"],
      description: "Manage config files",
      subcommands: [
        {
          name: ["generate", "g"],
          description: "[experimental] Generate a mise.toml file",
          options: [
            {
              name: ["-t", "--tool-versions"],
              description: "Path to a .tool-versions file to import tools from",
              isRepeatable: false,
              args: {
                name: "tool_versions",
              },
            },
            {
              name: ["-o", "--output"],
              description: "Output to file instead of stdout",
              isRepeatable: false,
              args: {
                name: "output",
              },
            },
          ],
        },
        {
          name: "get",
          description: "Display the value of a setting in a mise.toml file",
          options: [
            {
              name: ["-f", "--file"],
              description: "The path to the mise.toml file to edit",
              isRepeatable: false,
              args: {
                name: "file",
                template: "filepaths",
              },
            },
          ],
          args: {
            name: "key",
            description: "The path of the config to display",
            isOptional: true,
          },
        },
        {
          name: ["ls", "list"],
          description: "List config files currently in use",
          options: [
            {
              name: "--no-header",
              description: "Do not print table header",
              isRepeatable: false,
            },
            {
              name: "--tracked-configs",
              description: "List all tracked config files",
              isRepeatable: false,
            },
            {
              name: ["-J", "--json"],
              description: "Output in JSON format",
              isRepeatable: false,
            },
          ],
        },
        {
          name: "set",
          description: "Set the value of a setting in a mise.toml file",
          options: [
            {
              name: ["-f", "--file"],
              description: "The path to the mise.toml file to edit",
              isRepeatable: false,
              args: {
                name: "file",
                template: "filepaths",
              },
            },
            {
              name: ["-t", "--type"],
              isRepeatable: false,
              args: {
                name: "type",
                suggestions: [
                  "infer",
                  "string",
                  "integer",
                  "float",
                  "bool",
                  "list",
                  "set",
                ],
              },
            },
          ],
          args: [
            {
              name: "key",
              description: "The path of the config to display",
            },
            {
              name: "value",
              description: "The value to set the key to",
            },
          ],
        },
      ],
      options: [
        {
          name: "--no-header",
          description: "Do not print table header",
          isRepeatable: false,
        },
        {
          name: "--tracked-configs",
          description: "List all tracked config files",
          isRepeatable: false,
        },
        {
          name: ["-J", "--json"],
          description: "Output in JSON format",
          isRepeatable: false,
        },
      ],
    },
    {
      name: "deactivate",
      description: "Disable mise for current shell session",
    },
    {
      name: ["doctor", "dr"],
      description: "Check mise installation for possible problems",
      subcommands: [
        {
          name: "path",
          description: "Print the current PATH entries mise is providing",
          options: [
            {
              name: ["-f", "--full"],
              description:
                "Print all entries including those not provided by mise",
              isRepeatable: false,
            },
          ],
        },
      ],
      options: [
        {
          name: ["-J", "--json"],
          isRepeatable: false,
        },
      ],
    },
    {
      name: "en",
      description:
        "[experimental] starts a new shell with the mise environment built from the current configuration",
      options: [
        {
          name: ["-s", "--shell"],
          description: "Shell to start",
          isRepeatable: false,
          args: {
            name: "shell",
          },
        },
      ],
      args: {
        name: "dir",
        description: "Directory to start the shell in",
        isOptional: true,
        template: "folders",
      },
    },
    {
      name: ["env", "e"],
      description: "Exports env vars to activate mise a single time",
      options: [
        {
          name: ["-J", "--json"],
          description: "Output in JSON format",
          isRepeatable: false,
        },
        {
          name: "--json-extended",
          description:
            "Output in JSON format with additional information (source, tool)",
          isRepeatable: false,
        },
        {
          name: ["-D", "--dotenv"],
          description: "Output in dotenv format",
          isRepeatable: false,
        },
        {
          name: ["-s", "--shell"],
          description: "Shell type to generate environment variables for",
          isRepeatable: false,
          args: {
            name: "shell",
            suggestions: [
              "bash",
              "elvish",
              "fish",
              "nu",
              "xonsh",
              "zsh",
              "pwsh",
            ],
          },
        },
      ],
      args: {
        name: "tool@version",
        description: "Tool(s) to use",
        isOptional: true,
        isVariadic: true,
        generators: toolVersionGenerator,
        debounce: true,
      },
    },
    {
      name: ["exec", "x"],
      description: "Execute a command with tool(s) set",
      options: [
        {
          name: ["-c", "--command"],
          description: "Command string to execute",
          isRepeatable: false,
          args: {
            name: "c",
          },
        },
        {
          name: ["-j", "--jobs"],
          description: "Number of jobs to run in parallel\n[default: 4]",
          isRepeatable: false,
          args: {
            name: "jobs",
          },
        },
        {
          name: "--raw",
          description:
            "Directly pipe stdin/stdout/stderr from plugin to user Sets --jobs=1",
          isRepeatable: false,
        },
      ],
      args: [
        {
          name: "tool@version",
          description: "Tool(s) to start e.g.: node@20 python@3.10",
          isOptional: true,
          isVariadic: true,
          generators: toolVersionGenerator,
          debounce: true,
        },
        {
          name: "command",
          description: "Command string to execute (same as --command)",
          isOptional: true,
          isVariadic: true,
        },
      ],
    },
    {
      name: "fmt",
      description: "Formats mise.toml",
      options: [
        {
          name: ["-a", "--all"],
          description: "Format all files from the current directory",
          isRepeatable: false,
        },
        {
          name: ["-c", "--check"],
          description:
            "Check if the configs are formatted, no formatting is done",
          isRepeatable: false,
        },
        {
          name: ["-s", "--stdin"],
          description:
            "Read config from stdin and write its formatted version into stdout",
          isRepeatable: false,
        },
      ],
    },
    {
      name: ["generate", "gen"],
      description: "[experimental] Generate files for various tools/services",
      subcommands: [
        {
          name: "bootstrap",
          description:
            "[experimental] Generate a script to download+execute mise",
          options: [
            {
              name: ["-l", "--localize"],
              description:
                "Sandboxes mise internal directories like MISE_DATA_DIR and MISE_CACHE_DIR into a `.mise` directory in the project",
              isRepeatable: false,
            },
            {
              name: "--localized-dir",
              description: "Directory to put localized data into",
              isRepeatable: false,
              args: {
                name: "localized_dir",
                template: "folders",
              },
            },
            {
              name: ["-V", "--version"],
              description: "Specify mise version to fetch",
              isRepeatable: false,
              args: {
                name: "version",
              },
            },
            {
              name: ["-w", "--write"],
              description:
                "Instead of outputting the script to stdout, write to a file and make it executable",
              isRepeatable: false,
              args: {
                name: "write",
              },
            },
          ],
        },
        {
          name: ["config", "g"],
          description: "[experimental] Generate a mise.toml file",
          options: [
            {
              name: ["-t", "--tool-versions"],
              description: "Path to a .tool-versions file to import tools from",
              isRepeatable: false,
              args: {
                name: "tool_versions",
              },
            },
            {
              name: ["-o", "--output"],
              description: "Output to file instead of stdout",
              isRepeatable: false,
              args: {
                name: "output",
              },
            },
          ],
        },
        {
          name: "devcontainer",
          description: "[experimental] Generate a devcontainer to execute mise",
          options: [
            {
              name: ["-n", "--name"],
              description: "The name of the devcontainer",
              isRepeatable: false,
              args: {
                name: "name",
              },
            },
            {
              name: ["-i", "--image"],
              description: "The image to use for the devcontainer",
              isRepeatable: false,
              args: {
                name: "image",
              },
            },
            {
              name: ["-m", "--mount-mise-data"],
              description: "Bind the mise-data-volume to the devcontainer",
              isRepeatable: false,
            },
            {
              name: ["-w", "--write"],
              description: "Write to .devcontainer/devcontainer.json",
              isRepeatable: false,
            },
          ],
        },
        {
          name: ["git-pre-commit", "pre-commit"],
          description: "[experimental] Generate a git pre-commit hook",
          options: [
            {
              name: "--hook",
              description: "Which hook to generate (saves to .git/hooks/$hook)",
              isRepeatable: false,
              args: {
                name: "hook",
              },
            },
            {
              name: ["-t", "--task"],
              description:
                "The task to run when the pre-commit hook is triggered",
              isRepeatable: false,
              args: {
                name: "task",
                generators: simpleTaskGenerator,
                debounce: true,
              },
            },
            {
              name: ["-w", "--write"],
              description:
                "Write to .git/hooks/pre-commit and make it executable",
              isRepeatable: false,
            },
          ],
        },
        {
          name: "github-action",
          description: "[experimental] Generate a GitHub Action workflow file",
          options: [
            {
              name: "--name",
              description: "The name of the workflow to generate",
              isRepeatable: false,
              args: {
                name: "name",
              },
            },
            {
              name: ["-t", "--task"],
              description: "The task to run when the workflow is triggered",
              isRepeatable: false,
              args: {
                name: "task",
                generators: simpleTaskGenerator,
                debounce: true,
              },
            },
            {
              name: ["-w", "--write"],
              description: "Write to .github/workflows/$name.yml",
              isRepeatable: false,
            },
          ],
        },
        {
          name: "task-docs",
          description: "Generate documentation for tasks in a project",
          options: [
            {
              name: ["-I", "--index"],
              description:
                "Write only an index of tasks, intended for use with `--multi`",
              isRepeatable: false,
            },
            {
              name: ["-i", "--inject"],
              description: "Inserts the documentation into an existing file",
              isRepeatable: false,
            },
            {
              name: ["-m", "--multi"],
              description:
                "Render each task as a separate document, requires `--output` to be a directory",
              isRepeatable: false,
            },
            {
              name: ["-o", "--output"],
              description: "Writes the generated docs to a file/directory",
              isRepeatable: false,
              args: {
                name: "output",
              },
            },
            {
              name: ["-r", "--root"],
              description: "Root directory to search for tasks",
              isRepeatable: false,
              args: {
                name: "root",
              },
            },
            {
              name: ["-s", "--style"],
              isRepeatable: false,
              args: {
                name: "style",
                suggestions: ["simple", "detailed"],
              },
            },
          ],
        },
        {
          name: "task-stubs",
          description: "[experimental] Generates shims to run mise tasks",
          options: [
            {
              name: ["-m", "--mise-bin"],
              description:
                "Path to a mise bin to use when running the task stub.",
              isRepeatable: false,
              args: {
                name: "mise_bin",
              },
            },
            {
              name: ["-d", "--dir"],
              description: "Directory to create task stubs inside of",
              isRepeatable: false,
              args: {
                name: "dir",
                template: "folders",
              },
            },
          ],
        },
      ],
    },
    {
      name: "implode",
      description: "Removes mise CLI and all related data",
      options: [
        {
          name: "--config",
          description: "Also remove config directory",
          isRepeatable: false,
        },
        {
          name: ["-n", "--dry-run"],
          description:
            "List directories that would be removed without actually removing them",
          isRepeatable: false,
        },
      ],
    },
    {
      name: ["install", "i"],
      description: "Install a tool version",
      options: [
        {
          name: ["-f", "--force"],
          description: "Force reinstall even if already installed",
          isRepeatable: false,
        },
        {
          name: ["-j", "--jobs"],
          description: "Number of jobs to run in parallel\n[default: 4]",
          isRepeatable: false,
          args: {
            name: "jobs",
          },
        },
        {
          name: "--raw",
          description:
            "Directly pipe stdin/stdout/stderr from plugin to user Sets --jobs=1",
          isRepeatable: false,
        },
        {
          name: ["-v", "--verbose"],
          description: "Show installation output",
          isRepeatable: true,
        },
      ],
      args: {
        name: "tool@version",
        description: "Tool(s) to install e.g.: node@20",
        isOptional: true,
        isVariadic: true,
        generators: toolVersionGenerator,
        debounce: true,
      },
    },
    {
      name: "install-into",
      description: "Install a tool version to a specific path",
      args: [
        {
          name: "tool@version",
          description: "Tool to install e.g.: node@20",
          generators: toolVersionGenerator,
          debounce: true,
        },
        {
          name: "path",
          description: "Path to install the tool into",
          template: "filepaths",
        },
      ],
    },
    {
      name: "latest",
      description: "Gets the latest available version for a plugin",
      options: [
        {
          name: ["-i", "--installed"],
          description: "Show latest installed instead of available version",
          isRepeatable: false,
        },
      ],
      args: {
        name: "tool@version",
        description: "Tool to get the latest version of",
        generators: toolVersionGenerator,
        debounce: true,
      },
    },
    {
      name: ["link", "ln"],
      description: "Symlinks a tool version into mise",
      options: [
        {
          name: ["-f", "--force"],
          description: "Overwrite an existing tool version if it exists",
          isRepeatable: false,
        },
      ],
      args: [
        {
          name: "tool@version",
          description: "Tool name and version to create a symlink for",
          generators: toolVersionGenerator,
          debounce: true,
        },
        {
          name: "path",
          description:
            "The local path to the tool version\ne.g.: ~/.nvm/versions/node/v20.0.0",
          template: "filepaths",
        },
      ],
    },
    {
      name: ["ls", "list"],
      description: "List installed and active tool versions",
      options: [
        {
          name: ["-c", "--current"],
          description:
            "Only show tool versions currently specified in a mise.toml",
          isRepeatable: false,
        },
        {
          name: ["-g", "--global"],
          description:
            "Only show tool versions currently specified in the global mise.toml",
          isRepeatable: false,
        },
        {
          name: ["-l", "--local"],
          description:
            "Only show tool versions currently specified in the local mise.toml",
          isRepeatable: false,
        },
        {
          name: ["-i", "--installed"],
          description:
            "Only show tool versions that are installed (Hides tools defined in mise.toml but not installed)",
          isRepeatable: false,
        },
        {
          name: "--outdated",
          description: "Display whether a version is outdated",
          isRepeatable: false,
        },
        {
          name: ["-J", "--json"],
          description: "Output in JSON format",
          isRepeatable: false,
        },
        {
          name: ["-m", "--missing"],
          description: "Display missing tool versions",
          isRepeatable: false,
        },
        {
          name: "--prefix",
          description: "Display versions matching this prefix",
          isRepeatable: false,
          args: {
            name: "prefix",
            generators: completionGeneratorTemplate(
              `mise ls-remote {{words[PREV]}}`
            ),
            debounce: true,
          },
        },
        {
          name: "--prunable",
          description: "List only tools that can be pruned with `mise prune`",
          isRepeatable: false,
        },
        {
          name: "--no-header",
          description: "Don't display headers",
          isRepeatable: false,
        },
      ],
      args: {
        name: "installed_tool",
        description: "Only show tool versions from [TOOL]",
        isOptional: true,
        isVariadic: true,
        generators: completionGeneratorTemplate(
          `mise ls -i | awk '{print $1}' | uniq`
        ),
        debounce: true,
      },
    },
    {
      name: "ls-remote",
      description: "List runtime versions available for install.",
      options: [
        {
          name: "--all",
          description: "Show all installed plugins and versions",
          isRepeatable: false,
        },
      ],
      args: [
        {
          name: "tool@version",
          description: "Tool to get versions for",
          isOptional: true,
          generators: toolVersionGenerator,
          debounce: true,
        },
        {
          name: "prefix",
          description:
            'The version prefix to use when querying the latest version\nsame as the first argument after the "@"',
          isOptional: true,
          generators: completionGeneratorTemplate(
            `mise ls-remote {{words[PREV]}}`
          ),
          debounce: true,
        },
      ],
    },
    {
      name: "outdated",
      description: "Shows outdated tool versions",
      options: [
        {
          name: ["-l", "--bump"],
          description:
            "Compares against the latest versions available, not what matches the current config",
          isRepeatable: false,
        },
        {
          name: ["-J", "--json"],
          description: "Output in JSON format",
          isRepeatable: false,
        },
        {
          name: "--no-header",
          description: "Don't show table header",
          isRepeatable: false,
        },
      ],
      args: {
        name: "tool@version",
        description:
          "Tool(s) to show outdated versions for\ne.g.: node@20 python@3.10\nIf not specified, all tools in global and local configs will be shown",
        isOptional: true,
        isVariadic: true,
        generators: toolVersionGenerator,
        debounce: true,
      },
    },
    {
      name: ["plugins", "p"],
      description: "Manage plugins",
      subcommands: [
        {
          name: ["install", "i", "a", "add"],
          description: "Install a plugin",
          options: [
            {
              name: ["-f", "--force"],
              description: "Reinstall even if plugin exists",
              isRepeatable: false,
            },
            {
              name: ["-a", "--all"],
              description:
                "Install all missing plugins\nThis will only install plugins that have matching shorthands.\ni.e.: they don't need the full git repo url",
              isRepeatable: false,
            },
            {
              name: ["-v", "--verbose"],
              description: "Show installation output",
              isRepeatable: true,
            },
            {
              name: ["-j", "--jobs"],
              description: "Number of jobs to run in parallel",
              isRepeatable: false,
              args: {
                name: "jobs",
              },
            },
          ],
          args: [
            {
              name: "new_plugin",
              description:
                "The name of the plugin to install\ne.g.: node, ruby\nCan specify multiple plugins: `mise plugins install node ruby python`",
              isOptional: true,
              generators: completionGeneratorTemplate(`mise plugins --all`),
              debounce: true,
            },
            {
              name: "git_url",
              description: "The git url of the plugin",
              isOptional: true,
            },
          ],
        },
        {
          name: ["link", "ln"],
          description: "Symlinks a plugin into mise",
          options: [
            {
              name: ["-f", "--force"],
              description: "Overwrite existing plugin",
              isRepeatable: false,
            },
          ],
          args: [
            {
              name: "name",
              description: "The name of the plugin\ne.g.: node, ruby",
            },
            {
              name: "dir",
              description: "The local path to the plugin\ne.g.: ./mise-node",
              isOptional: true,
              template: "folders",
            },
          ],
        },
        {
          name: ["ls", "list"],
          description: "List installed plugins",
          options: [
            {
              name: ["-u", "--urls"],
              description:
                "Show the git url for each plugin\ne.g.: https://github.com/asdf-vm/asdf-nodejs.git",
              isRepeatable: false,
            },
          ],
        },
        {
          name: ["ls-remote", "list-remote", "list-all"],
          description: "List all available remote plugins",
          options: [
            {
              name: ["-u", "--urls"],
              description:
                "Show the git url for each plugin e.g.: https://github.com/mise-plugins/mise-poetry.git",
              isRepeatable: false,
            },
            {
              name: "--only-names",
              description:
                'Only show the name of each plugin by default it will show a "*" next to installed plugins',
              isRepeatable: false,
            },
          ],
        },
        {
          name: ["uninstall", "remove", "rm"],
          description: "Removes a plugin",
          options: [
            {
              name: ["-p", "--purge"],
              description:
                "Also remove the plugin's installs, downloads, and cache",
              isRepeatable: false,
            },
            {
              name: ["-a", "--all"],
              description: "Remove all plugins",
              isRepeatable: false,
            },
          ],
          args: {
            name: "plugin",
            description: "Plugin(s) to remove",
            isOptional: true,
            isVariadic: true,
            generators: pluginGenerator,
            debounce: true,
          },
        },
        {
          name: ["update", "up", "upgrade"],
          description: "Updates a plugin to the latest version",
          options: [
            {
              name: ["-j", "--jobs"],
              description: "Number of jobs to run in parallel\nDefault: 4",
              isRepeatable: false,
              args: {
                name: "jobs",
              },
            },
          ],
          args: {
            name: "plugin",
            description: "Plugin(s) to update",
            isOptional: true,
            isVariadic: true,
            generators: pluginGenerator,
            debounce: true,
          },
        },
      ],
      options: [
        {
          name: ["-c", "--core"],
          description:
            "The built-in plugins only\nNormally these are not shown",
          isRepeatable: false,
        },
        {
          name: "--user",
          description: "List installed plugins",
          isRepeatable: false,
        },
        {
          name: ["-u", "--urls"],
          description:
            "Show the git url for each plugin\ne.g.: https://github.com/asdf-vm/asdf-nodejs.git",
          isRepeatable: false,
        },
      ],
    },
    {
      name: "prune",
      description: "Delete unused versions of tools",
      options: [
        {
          name: ["-n", "--dry-run"],
          description: "Do not actually delete anything",
          isRepeatable: false,
        },
        {
          name: "--configs",
          description:
            "Prune only tracked and trusted configuration links that point to non-existent configurations",
          isRepeatable: false,
        },
        {
          name: "--tools",
          description: "Prune only unused versions of tools",
          isRepeatable: false,
        },
      ],
      args: {
        name: "installed_tool",
        description: "Prune only these tools",
        isOptional: true,
        isVariadic: true,
        generators: completionGeneratorTemplate(
          `mise ls -i | awk '{print $1}' | uniq`
        ),
        debounce: true,
      },
    },
    {
      name: "registry",
      description: "List available tools to install",
      options: [
        {
          name: ["-b", "--backend"],
          description: "Show only tools for this backend",
          isRepeatable: false,
          args: {
            name: "backend",
            generators: completionGeneratorTemplate(`mise backends`),
            debounce: true,
          },
        },
        {
          name: "--hide-aliased",
          description: "Hide aliased tools",
          isRepeatable: false,
        },
      ],
      args: {
        name: "name",
        description: "Show only the specified tool's full name",
        isOptional: true,
      },
    },
    {
      name: "reshim",
      description:
        "Creates new shims based on bin paths from currently installed tools.",
      options: [
        {
          name: ["-f", "--force"],
          description: "Removes all shims before reshimming",
          isRepeatable: false,
        },
      ],
    },
    {
      name: ["run", "r"],
      description: "Run task(s)",
      options: [
        {
          name: ["-C", "--cd"],
          description: "Change to this directory before executing the command",
          isRepeatable: false,
          args: {
            name: "cd",
          },
        },
        {
          name: ["-c", "--continue-on-error"],
          description: "Continue running tasks even if one fails",
          isRepeatable: false,
        },
        {
          name: ["-n", "--dry-run"],
          description:
            "Don't actually run the tasks(s), just print them in order of execution",
          isRepeatable: false,
        },
        {
          name: ["-f", "--force"],
          description: "Force the tasks to run even if outputs are up to date",
          isRepeatable: false,
        },
        {
          name: ["-s", "--shell"],
          description: "Shell to use to run toml tasks",
          isRepeatable: false,
          args: {
            name: "shell",
          },
        },
        {
          name: ["-t", "--tool"],
          description:
            "Tool(s) to run in addition to what is in mise.toml files e.g.: node@20 python@3.10",
          isRepeatable: true,
          args: {
            name: "tool@version",
            generators: toolVersionGenerator,
            debounce: true,
          },
        },
        {
          name: ["-j", "--jobs"],
          description:
            "Number of tasks to run in parallel\n[default: 4]\nConfigure with `jobs` config or `MISE_JOBS` env var",
          isRepeatable: false,
          args: {
            name: "jobs",
          },
        },
        {
          name: ["-r", "--raw"],
          description:
            "Read/write directly to stdin/stdout/stderr instead of by line\nRedactions are not applied with this option\nConfigure with `raw` config or `MISE_RAW` env var",
          isRepeatable: false,
        },
        {
          name: "--no-timings",
          description: "Hides elapsed time after each task completes",
          isRepeatable: false,
        },
        {
          name: ["-q", "--quiet"],
          description: "Don't show extra output",
          isRepeatable: false,
        },
        {
          name: ["-S", "--silent"],
          description: "Don't show any output except for errors",
          isRepeatable: false,
        },
        {
          name: ["-o", "--output"],
          description:
            "Change how tasks information is output when running tasks",
          isRepeatable: false,
          args: {
            name: "output",
          },
        },
        {
          name: "--no-cache",
          isRepeatable: false,
        },
      ],
      generateSpec: usageGenerateSpec(["mise tasks --usage"]),
      cache: false,
    },
    {
      name: "search",
      description: "Search for tools in the registry",
      options: [
        {
          name: ["-i", "--interactive"],
          description: "Show interactive search",
          isRepeatable: false,
        },
        {
          name: ["-m", "--match-type"],
          description: "Match type: equal, contains, or fuzzy",
          isRepeatable: false,
          args: {
            name: "match_type",
            suggestions: ["equal", "contains", "fuzzy"],
          },
        },
        {
          name: "--no-header",
          description: "Don't display headers",
          isRepeatable: false,
        },
      ],
      args: {
        name: "name",
        description: "The tool to search for",
        isOptional: true,
      },
    },
    {
      name: "self-update",
      description: "Updates mise itself.",
      options: [
        {
          name: ["-f", "--force"],
          description: "Update even if already up to date",
          isRepeatable: false,
        },
        {
          name: "--no-plugins",
          description: "Disable auto-updating plugins",
          isRepeatable: false,
        },
        {
          name: ["-y", "--yes"],
          description: "Skip confirmation prompt",
          isRepeatable: false,
        },
      ],
      args: {
        name: "version",
        description: "Update to a specific version",
        isOptional: true,
      },
    },
    {
      name: "set",
      description: "Set environment variables in mise.toml",
      options: [
        {
          name: "--file",
          description: "The TOML file to update",
          isRepeatable: false,
          args: {
            name: "file",
            template: "filepaths",
          },
        },
        {
          name: ["-g", "--global"],
          description: "Set the environment variable in the global config file",
          isRepeatable: false,
        },
      ],
      args: {
        name: "env_var",
        description:
          "Environment variable(s) to set\ne.g.: NODE_ENV=production",
        isOptional: true,
        isVariadic: true,
        generators: envVarGenerator,
        debounce: true,
      },
    },
    {
      name: "settings",
      description: "Manage settings",
      subcommands: [
        {
          name: "add",
          description: "Adds a setting to the configuration file",
          options: [
            {
              name: ["-l", "--local"],
              description:
                "Use the local config file instead of the global one",
              isRepeatable: false,
            },
          ],
          args: [
            {
              name: "setting",
              description: "The setting to set",
              generators: settingsGenerator,
              debounce: true,
            },
            {
              name: "value",
              description: "The value to set",
            },
          ],
        },
        {
          name: "get",
          description: "Show a current setting",
          options: [
            {
              name: ["-l", "--local"],
              description:
                "Use the local config file instead of the global one",
              isRepeatable: false,
            },
          ],
          args: {
            name: "setting",
            description: "The setting to show",
            generators: settingsGenerator,
            debounce: true,
          },
        },
        {
          name: ["ls", "list"],
          description: "Show current settings",
          options: [
            {
              name: ["-a", "--all"],
              description: "List all settings",
              isRepeatable: false,
            },
            {
              name: ["-l", "--local"],
              description:
                "Use the local config file instead of the global one",
              isRepeatable: false,
            },
            {
              name: ["-J", "--json"],
              description: "Output in JSON format",
              isRepeatable: false,
            },
            {
              name: "--json-extended",
              description: "Output in JSON format with sources",
              isRepeatable: false,
            },
            {
              name: ["-T", "--toml"],
              description: "Output in TOML format",
              isRepeatable: false,
            },
          ],
          args: {
            name: "setting",
            description: "Name of setting",
            isOptional: true,
            generators: settingsGenerator,
            debounce: true,
          },
        },
        {
          name: ["set", "create"],
          description: "Add/update a setting",
          options: [
            {
              name: ["-l", "--local"],
              description:
                "Use the local config file instead of the global one",
              isRepeatable: false,
            },
          ],
          args: [
            {
              name: "setting",
              description: "The setting to set",
              generators: settingsGenerator,
              debounce: true,
            },
            {
              name: "value",
              description: "The value to set",
            },
          ],
        },
        {
          name: ["unset", "rm", "remove", "delete", "del"],
          description: "Clears a setting",
          options: [
            {
              name: ["-l", "--local"],
              description:
                "Use the local config file instead of the global one",
              isRepeatable: false,
            },
          ],
          args: {
            name: "key",
            description: "The setting to remove",
          },
        },
      ],
      options: [
        {
          name: ["-a", "--all"],
          description: "List all settings",
          isRepeatable: false,
        },
        {
          name: ["-l", "--local"],
          description: "Use the local config file instead of the global one",
          isRepeatable: false,
        },
        {
          name: ["-J", "--json"],
          description: "Output in JSON format",
          isRepeatable: false,
        },
        {
          name: "--json-extended",
          description: "Output in JSON format with sources",
          isRepeatable: false,
        },
        {
          name: ["-T", "--toml"],
          description: "Output in TOML format",
          isRepeatable: false,
        },
      ],
      args: [
        {
          name: "setting",
          description: "Name of setting",
          isOptional: true,
          generators: settingsGenerator,
          debounce: true,
        },
        {
          name: "value",
          description: "Setting value to set",
          isOptional: true,
        },
      ],
    },
    {
      name: ["shell", "sh"],
      description: "Sets a tool version for the current session.",
      options: [
        {
          name: ["-j", "--jobs"],
          description: "Number of jobs to run in parallel\n[default: 4]",
          isRepeatable: false,
          args: {
            name: "jobs",
          },
        },
        {
          name: "--raw",
          description:
            "Directly pipe stdin/stdout/stderr from plugin to user Sets --jobs=1",
          isRepeatable: false,
        },
        {
          name: ["-u", "--unset"],
          description: "Removes a previously set version",
          isRepeatable: false,
        },
      ],
      args: {
        name: "tool@version",
        description: "Tool(s) to use",
        isVariadic: true,
        generators: toolVersionGenerator,
        debounce: true,
      },
    },
    {
      name: "sync",
      description: "Synchronize tools from other version managers with mise",
      subcommands: [
        {
          name: "node",
          description:
            "Symlinks all tool versions from an external tool into mise",
          options: [
            {
              name: "--brew",
              description: "Get tool versions from Homebrew",
              isRepeatable: false,
            },
            {
              name: "--nvm",
              description: "Get tool versions from nvm",
              isRepeatable: false,
            },
            {
              name: "--nodenv",
              description: "Get tool versions from nodenv",
              isRepeatable: false,
            },
          ],
        },
        {
          name: "python",
          description:
            "Symlinks all tool versions from an external tool into mise",
          options: [
            {
              name: "--pyenv",
              description: "Get tool versions from pyenv",
              isRepeatable: false,
            },
            {
              name: "--uv",
              description: "Sync tool versions with uv (2-way sync)",
              isRepeatable: false,
            },
          ],
        },
        {
          name: "ruby",
          description:
            "Symlinks all ruby tool versions from an external tool into mise",
          options: [
            {
              name: "--brew",
              description: "Get tool versions from Homebrew",
              isRepeatable: false,
            },
          ],
        },
      ],
    },
    {
      name: ["tasks", "t"],
      description: "Manage tasks",
      subcommands: [
        {
          name: "add",
          description: "Create a new task",
          options: [
            {
              name: "--description",
              description: "Description of the task",
              isRepeatable: false,
              args: {
                name: "description",
              },
            },
            {
              name: ["-a", "--alias"],
              description: "Other names for the task",
              isRepeatable: true,
              args: {
                name: "alias",
                generators: aliasGenerator,
                debounce: true,
              },
            },
            {
              name: "--depends-post",
              description: "Dependencies to run after the task runs",
              isRepeatable: true,
              args: {
                name: "depends_post",
              },
            },
            {
              name: ["-w", "--wait-for"],
              description:
                "Wait for these tasks to complete if they are to run",
              isRepeatable: true,
              args: {
                name: "wait_for",
              },
            },
            {
              name: ["-D", "--dir"],
              description: "Run the task in a specific directory",
              isRepeatable: false,
              args: {
                name: "dir",
                template: "folders",
              },
            },
            {
              name: ["-H", "--hide"],
              description: "Hide the task from `mise task` and completions",
              isRepeatable: false,
            },
            {
              name: ["-r", "--raw"],
              description: "Directly connect stdin/stdout/stderr",
              isRepeatable: false,
            },
            {
              name: ["-s", "--sources"],
              description: "Glob patterns of files this task uses as input",
              isRepeatable: true,
              args: {
                name: "sources",
              },
            },
            {
              name: "--outputs",
              description:
                "Glob patterns of files this task creates, to skip if they are not modified",
              isRepeatable: true,
              args: {
                name: "outputs",
              },
            },
            {
              name: "--shell",
              description: "Run the task in a specific shell",
              isRepeatable: false,
              args: {
                name: "shell",
              },
            },
            {
              name: ["-q", "--quiet"],
              description: "Do not print the command before running",
              isRepeatable: false,
            },
            {
              name: "--silent",
              description: "Do not print the command or its output",
              isRepeatable: false,
            },
            {
              name: ["-d", "--depends"],
              description: "Add dependencies to the task",
              isRepeatable: true,
              args: {
                name: "depends",
              },
            },
            {
              name: "--run-windows",
              description: "Command to run on windows",
              isRepeatable: false,
              args: {
                name: "run_windows",
              },
            },
            {
              name: ["-f", "--file"],
              description: "Create a file task instead of a toml task",
              isRepeatable: false,
            },
          ],
          args: [
            {
              name: "task",
              description: "Tasks name to add",
              generators: simpleTaskGenerator,
              debounce: true,
            },
            {
              name: "run",
              isOptional: true,
              isVariadic: true,
            },
          ],
        },
        {
          name: "deps",
          description: "Display a tree visualization of a dependency graph",
          options: [
            {
              name: "--hidden",
              description: "Show hidden tasks",
              isRepeatable: false,
            },
            {
              name: "--dot",
              description: "Display dependencies in DOT format",
              isRepeatable: false,
            },
          ],
          args: {
            name: "tasks",
            description:
              "Tasks to show dependencies for\nCan specify multiple tasks by separating with spaces\ne.g.: mise tasks deps lint test check",
            isOptional: true,
            isVariadic: true,
          },
        },
        {
          name: "edit",
          description: "Edit a tasks with $EDITOR",
          options: [
            {
              name: ["-p", "--path"],
              description:
                "Display the path to the tasks instead of editing it",
              isRepeatable: false,
            },
          ],
          args: {
            name: "task",
            description: "Tasks to edit",
            generators: simpleTaskGenerator,
            debounce: true,
          },
        },
        {
          name: "info",
          description: "Get information about a task",
          options: [
            {
              name: ["-J", "--json"],
              description: "Output in JSON format",
              isRepeatable: false,
            },
          ],
          args: {
            name: "task",
            description: "Name of the task to get information about",
            generators: simpleTaskGenerator,
            debounce: true,
          },
        },
        {
          name: "ls",
          description:
            "List available tasks to execute\nThese may be included from the config file or from the project's .mise/tasks directory\nmise will merge all tasks from all parent directories into this list.",
          options: [
            {
              name: ["-x", "--extended"],
              description: "Show all columns",
              isRepeatable: false,
            },
            {
              name: "--no-header",
              description: "Do not print table header",
              isRepeatable: false,
            },
            {
              name: "--hidden",
              description: "Show hidden tasks",
              isRepeatable: false,
            },
            {
              name: ["-g", "--global"],
              description: "Only show global tasks",
              isRepeatable: false,
            },
            {
              name: ["-J", "--json"],
              description: "Output in JSON format",
              isRepeatable: false,
            },
            {
              name: ["-l", "--local"],
              description: "Only show non-global tasks",
              isRepeatable: false,
            },
            {
              name: "--sort",
              description: "Sort by column. Default is name.",
              isRepeatable: false,
              args: {
                name: "column",
                suggestions: ["name", "alias", "description", "source"],
              },
            },
            {
              name: "--sort-order",
              description: "Sort order. Default is asc.",
              isRepeatable: false,
              args: {
                name: "sort_order",
                suggestions: ["asc", "desc"],
              },
            },
          ],
        },
        {
          name: ["run", "r"],
          description: "Run task(s)",
          options: [
            {
              name: ["-C", "--cd"],
              description:
                "Change to this directory before executing the command",
              isRepeatable: false,
              args: {
                name: "cd",
              },
            },
            {
              name: ["-c", "--continue-on-error"],
              description: "Continue running tasks even if one fails",
              isRepeatable: false,
            },
            {
              name: ["-n", "--dry-run"],
              description:
                "Don't actually run the tasks(s), just print them in order of execution",
              isRepeatable: false,
            },
            {
              name: ["-f", "--force"],
              description:
                "Force the tasks to run even if outputs are up to date",
              isRepeatable: false,
            },
            {
              name: ["-s", "--shell"],
              description: "Shell to use to run toml tasks",
              isRepeatable: false,
              args: {
                name: "shell",
              },
            },
            {
              name: ["-t", "--tool"],
              description:
                "Tool(s) to run in addition to what is in mise.toml files e.g.: node@20 python@3.10",
              isRepeatable: true,
              args: {
                name: "tool@version",
                generators: toolVersionGenerator,
                debounce: true,
              },
            },
            {
              name: ["-j", "--jobs"],
              description:
                "Number of tasks to run in parallel\n[default: 4]\nConfigure with `jobs` config or `MISE_JOBS` env var",
              isRepeatable: false,
              args: {
                name: "jobs",
              },
            },
            {
              name: ["-r", "--raw"],
              description:
                "Read/write directly to stdin/stdout/stderr instead of by line\nRedactions are not applied with this option\nConfigure with `raw` config or `MISE_RAW` env var",
              isRepeatable: false,
            },
            {
              name: "--no-timings",
              description: "Hides elapsed time after each task completes",
              isRepeatable: false,
            },
            {
              name: ["-q", "--quiet"],
              description: "Don't show extra output",
              isRepeatable: false,
            },
            {
              name: ["-S", "--silent"],
              description: "Don't show any output except for errors",
              isRepeatable: false,
            },
            {
              name: ["-o", "--output"],
              description:
                "Change how tasks information is output when running tasks",
              isRepeatable: false,
              args: {
                name: "output",
              },
            },
            {
              name: "--no-cache",
              isRepeatable: false,
            },
          ],
          args: [
            {
              name: "task",
              description:
                "Tasks to run\nCan specify multiple tasks by separating with `:::`\ne.g.: mise run task1 arg1 arg2 ::: task2 arg1 arg2",
              isOptional: true,
              generators: simpleTaskGenerator,
              debounce: true,
            },
            {
              name: "args",
              description:
                'Arguments to pass to the tasks. Use ":::" to separate tasks',
              isOptional: true,
              isVariadic: true,
            },
          ],
          generateSpec: usageGenerateSpec(["mise tasks --usage"]),
          cache: false,
        },
      ],
      options: [
        {
          name: ["-x", "--extended"],
          description: "Show all columns",
          isRepeatable: false,
        },
        {
          name: "--no-header",
          description: "Do not print table header",
          isRepeatable: false,
        },
        {
          name: "--hidden",
          description: "Show hidden tasks",
          isRepeatable: false,
        },
        {
          name: ["-g", "--global"],
          description: "Only show global tasks",
          isRepeatable: false,
        },
        {
          name: ["-J", "--json"],
          description: "Output in JSON format",
          isRepeatable: false,
        },
        {
          name: ["-l", "--local"],
          description: "Only show non-global tasks",
          isRepeatable: false,
        },
        {
          name: "--sort",
          description: "Sort by column. Default is name.",
          isRepeatable: false,
          args: {
            name: "column",
            suggestions: ["name", "alias", "description", "source"],
          },
        },
        {
          name: "--sort-order",
          description: "Sort order. Default is asc.",
          isRepeatable: false,
          args: {
            name: "sort_order",
            suggestions: ["asc", "desc"],
          },
        },
      ],
      args: {
        name: "task",
        description: "Task name to get info of",
        isOptional: true,
        generators: simpleTaskGenerator,
        debounce: true,
      },
    },
    {
      name: "test-tool",
      description: "Test a tool installs and executes",
      options: [
        {
          name: ["-a", "--all"],
          description: "Test every tool specified in registry.toml",
          isRepeatable: false,
        },
        {
          name: "--all-config",
          description: "Test all tools specified in config files",
          isRepeatable: false,
        },
        {
          name: "--include-non-defined",
          description:
            "Also test tools not defined in registry.toml, guessing how to test it",
          isRepeatable: false,
        },
        {
          name: ["-j", "--jobs"],
          description: "Number of jobs to run in parallel\n[default: 4]",
          isRepeatable: false,
          args: {
            name: "jobs",
          },
        },
        {
          name: "--raw",
          description:
            "Directly pipe stdin/stdout/stderr from plugin to user Sets --jobs=1",
          isRepeatable: false,
        },
      ],
      args: {
        name: "tool",
        description: "Tool name to test",
        isOptional: true,
        generators: completionGeneratorTemplate(`mise registry --complete`),
        debounce: true,
      },
    },
    {
      name: "tool",
      description: "Gets information about a tool",
      options: [
        {
          name: ["-J", "--json"],
          description: "Output in JSON format",
          isRepeatable: false,
        },
        {
          name: "--backend",
          description: "Only show backend field",
          isRepeatable: false,
        },
        {
          name: "--description",
          description: "Only show description field",
          isRepeatable: false,
        },
        {
          name: "--installed",
          description: "Only show installed versions",
          isRepeatable: false,
        },
        {
          name: "--active",
          description: "Only show active versions",
          isRepeatable: false,
        },
        {
          name: "--requested",
          description: "Only show requested versions",
          isRepeatable: false,
        },
        {
          name: "--config-source",
          description: "Only show config source",
          isRepeatable: false,
        },
        {
          name: "--tool-options",
          description: "Only show tool options",
          isRepeatable: false,
        },
      ],
      args: {
        name: "tool",
        description: "Tool name to get information about",
        generators: completionGeneratorTemplate(`mise registry --complete`),
        debounce: true,
      },
    },
    {
      name: "trust",
      description: "Marks a config file as trusted",
      options: [
        {
          name: ["-a", "--all"],
          description:
            "Trust all config files in the current directory and its parents",
          isRepeatable: false,
        },
        {
          name: "--ignore",
          description: "Do not trust this config and ignore it in the future",
          isRepeatable: false,
        },
        {
          name: "--untrust",
          description: "No longer trust this config, will prompt in the future",
          isRepeatable: false,
        },
        {
          name: "--show",
          description:
            "Show the trusted status of config files from the current directory and its parents.\nDoes not trust or untrust any files.",
          isRepeatable: false,
        },
      ],
      args: {
        name: "config_file",
        description: "The config file to trust",
        isOptional: true,
        template: "filepaths",
        generators: configPathGenerator,
        debounce: true,
      },
    },
    {
      name: "uninstall",
      description: "Removes installed tool versions",
      options: [
        {
          name: ["-a", "--all"],
          description: "Delete all installed versions",
          isRepeatable: false,
        },
        {
          name: ["-n", "--dry-run"],
          description: "Do not actually delete anything",
          isRepeatable: false,
        },
      ],
      args: {
        name: "installed_tool@version",
        description: "Tool(s) to remove",
        isOptional: true,
        isVariadic: true,
        generators: installedToolVersionGenerator,
        debounce: true,
      },
    },
    {
      name: "unset",
      description: "Remove environment variable(s) from the config file.",
      options: [
        {
          name: ["-f", "--file"],
          description: "Specify a file to use instead of `mise.toml`",
          isRepeatable: false,
          args: {
            name: "file",
            template: "filepaths",
          },
        },
        {
          name: ["-g", "--global"],
          description: "Use the global config file",
          isRepeatable: false,
        },
      ],
      args: {
        name: "env_key",
        description: "Environment variable(s) to remove\ne.g.: NODE_ENV",
        isOptional: true,
        isVariadic: true,
        generators: completionGeneratorTemplate(`mise set --complete`),
        debounce: true,
      },
    },
    {
      name: ["unuse", "rm", "remove"],
      description: "Removes installed tool versions from mise.toml",
      options: [
        {
          name: ["-g", "--global"],
          description:
            "Use the global config file (`~/.config/mise/config.toml`) instead of the local one",
          isRepeatable: false,
        },
        {
          name: ["-e", "--env"],
          description:
            "Create/modify an environment-specific config file like .mise.<env>.toml",
          isRepeatable: false,
          args: {
            name: "env",
          },
        },
        {
          name: ["-p", "--path"],
          description: "Specify a path to a config file or directory",
          isRepeatable: false,
          args: {
            name: "path",
            template: "filepaths",
          },
        },
        {
          name: "--no-prune",
          description: "Do not also prune the installed version",
          isRepeatable: false,
        },
      ],
      args: {
        name: "installed_tool@version",
        description: "Tool(s) to remove",
        isVariadic: true,
        generators: installedToolVersionGenerator,
        debounce: true,
      },
    },
    {
      name: ["upgrade", "up"],
      description: "Upgrades outdated tools",
      options: [
        {
          name: ["-n", "--dry-run"],
          description: "Just print what would be done, don't actually do it",
          isRepeatable: false,
        },
        {
          name: ["-i", "--interactive"],
          description:
            "Display multiselect menu to choose which tools to upgrade",
          isRepeatable: false,
        },
        {
          name: ["-j", "--jobs"],
          description: "Number of jobs to run in parallel\n[default: 4]",
          isRepeatable: false,
          args: {
            name: "jobs",
          },
        },
        {
          name: ["-l", "--bump"],
          description:
            "Upgrades to the latest version available, bumping the version in mise.toml",
          isRepeatable: false,
        },
        {
          name: "--raw",
          description:
            "Directly pipe stdin/stdout/stderr from plugin to user Sets --jobs=1",
          isRepeatable: false,
        },
      ],
      args: {
        name: "tool@version",
        description:
          "Tool(s) to upgrade\ne.g.: node@20 python@3.10\nIf not specified, all current tools will be upgraded",
        isOptional: true,
        isVariadic: true,
        generators: toolVersionGenerator,
        debounce: true,
      },
    },
    {
      name: ["use", "u"],
      description: "Installs a tool and adds the version to mise.toml.",
      options: [
        {
          name: ["-f", "--force"],
          description: "Force reinstall even if already installed",
          isRepeatable: false,
        },
        {
          name: "--fuzzy",
          description: "Save fuzzy version to config file",
          isRepeatable: false,
        },
        {
          name: ["-g", "--global"],
          description:
            "Use the global config file (`~/.config/mise/config.toml`) instead of the local one",
          isRepeatable: false,
        },
        {
          name: ["-e", "--env"],
          description:
            "Create/modify an environment-specific config file like .mise.<env>.toml",
          isRepeatable: false,
          args: {
            name: "env",
          },
        },
        {
          name: ["-j", "--jobs"],
          description: "Number of jobs to run in parallel\n[default: 4]",
          isRepeatable: false,
          args: {
            name: "jobs",
          },
        },
        {
          name: "--raw",
          description:
            "Directly pipe stdin/stdout/stderr from plugin to user Sets `--jobs=1`",
          isRepeatable: false,
        },
        {
          name: "--remove",
          description: "Remove the plugin(s) from config file",
          isRepeatable: true,
          args: {
            name: "plugin",
            generators: pluginGenerator,
            debounce: true,
          },
        },
        {
          name: ["-p", "--path"],
          description: "Specify a path to a config file or directory",
          isRepeatable: false,
          args: {
            name: "path",
            template: "filepaths",
          },
        },
        {
          name: "--pin",
          description:
            "Save exact version to config file\ne.g.: `mise use --pin node@20` will save 20.0.0 as the version\nSet `MISE_PIN=1` to make this the default behavior",
          isRepeatable: false,
        },
      ],
      args: {
        name: "tool@version",
        description: "Tool(s) to add to config file",
        isOptional: true,
        isVariadic: true,
        generators: toolVersionGenerator,
        debounce: true,
      },
    },
    {
      name: ["version", "v"],
      description: "Display the version of mise",
      options: [
        {
          name: ["-J", "--json"],
          description: "Print the version information in JSON format",
          isRepeatable: false,
        },
      ],
    },
    {
      name: ["watch", "w"],
      description: "Run task(s) and watch for changes to rerun it",
      options: [
        {
          name: ["-w", "--watch"],
          description: "Watch a specific file or directory",
          isRepeatable: true,
          args: {
            name: "path",
            template: "filepaths",
          },
        },
        {
          name: ["-W", "--watch-non-recursive"],
          description: "Watch a specific directory, non-recursively",
          isRepeatable: true,
          args: {
            name: "path",
            template: "filepaths",
          },
        },
        {
          name: ["-F", "--watch-file"],
          description: "Watch files and directories from a file",
          isRepeatable: false,
          args: {
            name: "path",
            template: "filepaths",
          },
        },
        {
          name: ["-c", "--clear"],
          description: "Clear screen before running command",
          isRepeatable: false,
          args: {
            name: "mode",
            suggestions: ["clear", "reset"],
          },
        },
        {
          name: ["-o", "--on-busy-update"],
          description:
            "What to do when receiving events while the command is running",
          isRepeatable: false,
          args: {
            name: "mode",
            suggestions: ["queue", "do-nothing", "restart", "signal"],
          },
        },
        {
          name: ["-r", "--restart"],
          description: "Restart the process if it's still running",
          isRepeatable: false,
        },
        {
          name: ["-s", "--signal"],
          description: "Send a signal to the process when it's still running",
          isRepeatable: false,
          args: {
            name: "signal",
          },
        },
        {
          name: "--stop-signal",
          description: "Signal to send to stop the command",
          isRepeatable: false,
          args: {
            name: "signal",
          },
        },
        {
          name: "--stop-timeout",
          description: "Time to wait for the command to exit gracefully",
          isRepeatable: false,
          args: {
            name: "timeout",
          },
        },
        {
          name: "--map-signal",
          description:
            "Translate signals from the OS to signals to send to the command",
          isRepeatable: true,
          args: {
            name: "signal:signal",
          },
        },
        {
          name: ["-d", "--debounce"],
          description: "Time to wait for new events before taking action",
          isRepeatable: false,
          args: {
            name: "timeout",
          },
        },
        {
          name: "--stdin-quit",
          description: "Exit when stdin closes",
          isRepeatable: false,
        },
        {
          name: "--no-vcs-ignore",
          description: "Don't load gitignores",
          isRepeatable: false,
        },
        {
          name: "--no-project-ignore",
          description: "Don't load project-local ignores",
          isRepeatable: false,
        },
        {
          name: "--no-global-ignore",
          description: "Don't load global ignores",
          isRepeatable: false,
        },
        {
          name: "--no-default-ignore",
          description: "Don't use internal default ignores",
          isRepeatable: false,
        },
        {
          name: "--no-discover-ignore",
          description: "Don't discover ignore files at all",
          isRepeatable: false,
        },
        {
          name: "--ignore-nothing",
          description: "Don't ignore anything at all",
          isRepeatable: false,
        },
        {
          name: ["-p", "--postpone"],
          description: "Wait until first change before running command",
          isRepeatable: false,
        },
        {
          name: "--delay-run",
          description: "Sleep before running the command",
          isRepeatable: false,
          args: {
            name: "duration",
          },
        },
        {
          name: "--poll",
          description: "Poll for filesystem changes",
          isRepeatable: false,
          args: {
            name: "interval",
          },
        },
        {
          name: "--shell",
          description: "Use a different shell",
          isRepeatable: false,
          args: {
            name: "shell",
          },
        },
        {
          name: "-n",
          description: "Shorthand for '--shell=none'",
          isRepeatable: false,
        },
        {
          name: "--emit-events-to",
          description: "Configure event emission",
          isRepeatable: false,
          args: {
            name: "mode",
            suggestions: [
              "environment",
              "stdio",
              "file",
              "json-stdio",
              "json-file",
              "none",
            ],
          },
        },
        {
          name: "--only-emit-events",
          description: "Only emit events to stdout, run no commands",
          isRepeatable: false,
        },
        {
          name: ["-E", "--env"],
          description: "Add env vars to the command",
          isRepeatable: true,
          args: {
            name: "key=value",
          },
        },
        {
          name: "--wrap-process",
          description: "Configure how the process is wrapped",
          isRepeatable: false,
          args: {
            name: "mode",
            suggestions: ["group", "session", "none"],
          },
        },
        {
          name: ["-N", "--notify"],
          description: "Alert when commands start and end",
          isRepeatable: false,
        },
        {
          name: "--color",
          description: "When to use terminal colours",
          isRepeatable: false,
          args: {
            name: "mode",
            suggestions: ["auto", "always", "never"],
          },
        },
        {
          name: "--timings",
          description: "Print how long the command took to run",
          isRepeatable: false,
        },
        {
          name: ["-q", "--quiet"],
          description: "Don't print starting and stopping messages",
          isRepeatable: false,
        },
        {
          name: "--bell",
          description: "Ring the terminal bell on command completion",
          isRepeatable: false,
        },
        {
          name: "--project-origin",
          description: "Set the project origin",
          isRepeatable: false,
          args: {
            name: "directory",
            template: "folders",
          },
        },
        {
          name: "--workdir",
          description: "Set the working directory",
          isRepeatable: false,
          args: {
            name: "directory",
            template: "folders",
          },
        },
        {
          name: ["-e", "--exts"],
          description: "Filename extensions to filter to",
          isRepeatable: true,
          args: {
            name: "extensions",
          },
        },
        {
          name: ["-f", "--filter"],
          description: "Filename patterns to filter to",
          isRepeatable: true,
          args: {
            name: "pattern",
          },
        },
        {
          name: "--filter-file",
          description: "Files to load filters from",
          isRepeatable: true,
          args: {
            name: "path",
            template: "filepaths",
          },
        },
        {
          name: ["-J", "--filter-prog"],
          description: "[experimental] Filter programs",
          isRepeatable: true,
          args: {
            name: "expression",
          },
        },
        {
          name: ["-i", "--ignore"],
          description: "Filename patterns to filter out",
          isRepeatable: true,
          args: {
            name: "pattern",
          },
        },
        {
          name: "--ignore-file",
          description: "Files to load ignores from",
          isRepeatable: true,
          args: {
            name: "path",
            template: "filepaths",
          },
        },
        {
          name: "--fs-events",
          description: "Filesystem events to filter to",
          isRepeatable: true,
          args: {
            name: "events",
            suggestions: [
              "access",
              "create",
              "remove",
              "rename",
              "modify",
              "metadata",
            ],
          },
        },
        {
          name: "--no-meta",
          description: "Don't emit fs events for metadata changes",
          isRepeatable: false,
        },
        {
          name: "--print-events",
          description: "Print events that trigger actions",
          isRepeatable: false,
        },
        {
          name: "--manual",
          description: "Show the manual page",
          isRepeatable: false,
        },
      ],
      args: [
        {
          name: "task",
          description:
            "Tasks to run\nCan specify multiple tasks by separating with `:::`\ne.g.: `mise run task1 arg1 arg2 ::: task2 arg1 arg2`",
          isOptional: true,
          generators: simpleTaskGenerator,
          debounce: true,
        },
        {
          name: "args",
          description: "Task and arguments to run",
          isOptional: true,
          isVariadic: true,
        },
      ],
    },
    {
      name: "where",
      description: "Display the installation path for a tool",
      args: {
        name: "tool@version",
        description:
          'Tool(s) to look up\ne.g.: ruby@3\nif "@<PREFIX>" is specified, it will show the latest installed version\nthat matches the prefix\notherwise, it will show the current, active installed version',
        generators: toolVersionGenerator,
        debounce: true,
      },
    },
    {
      name: "which",
      description: "Shows the path that a tool's bin points to.",
      options: [
        {
          name: "--plugin",
          description: "Show the plugin name instead of the path",
          isRepeatable: false,
        },
        {
          name: "--version",
          description: "Show the version instead of the path",
          isRepeatable: false,
        },
        {
          name: ["-t", "--tool"],
          description:
            "Use a specific tool@version\ne.g.: `mise which npm --tool=node@20`",
          isRepeatable: false,
          args: {
            name: "tool@version",
            generators: toolVersionGenerator,
            debounce: true,
          },
        },
      ],
      args: {
        name: "bin_name",
        description: "The bin to look up",
        isOptional: true,
        generators: completionGeneratorTemplate(`mise which --complete`),
        debounce: true,
      },
    },
  ],
  options: [
    {
      name: ["-C", "--cd"],
      description: "Change directory before running command",
      isRepeatable: false,
      args: {
        name: "dir",
        template: "folders",
      },
    },
    {
      name: ["-E", "--env"],
      description: "Set the environment for loading `mise.<ENV>.toml`",
      isRepeatable: true,
      args: {
        name: "env",
      },
    },
    {
      name: ["-j", "--jobs"],
      description: "How many jobs to run in parallel [default: 8]",
      isRepeatable: false,
      args: {
        name: "jobs",
      },
    },
    {
      name: "--output",
      isRepeatable: false,
      args: {
        name: "output",
      },
    },
    {
      name: "--raw",
      description:
        "Read/write directly to stdin/stdout/stderr instead of by line",
      isRepeatable: false,
    },
    {
      name: "--no-config",
      description: "Do not load any config files",
      isRepeatable: false,
    },
    {
      name: ["-y", "--yes"],
      description: "Answer yes to all confirmation prompts",
      isRepeatable: false,
    },
    {
      name: ["-q", "--quiet"],
      description: "Suppress non-error messages",
      isRepeatable: false,
    },
    {
      name: "--silent",
      description: "Suppress all task output and mise non-error messages",
      isRepeatable: false,
    },
    {
      name: ["-v", "--verbose"],
      description: "Show extra output (use -vv for even more)",
      isRepeatable: true,
    },
  ],
  args: {
    name: "task",
    description: "Task to run",
    isOptional: true,
    generators: simpleTaskGenerator,
    debounce: true,
  },
};
export default completionSpec;
