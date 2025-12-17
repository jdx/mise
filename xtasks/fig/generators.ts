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

const shellAliasGenerator: Fig.Generator = {
  script: ["sh", "-c", "mise shell-alias ls --no-header"],
  postProcess: (out) => {
    if (!out.trim()) return [];
    return out
      .split("\n")
      .filter((l) => l.trim().length > 0)
      .map((l) => {
        const tokens = l.split(/\s+/);
        return { name: tokens[0], description: tokens.slice(1).join(" ") };
      });
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
