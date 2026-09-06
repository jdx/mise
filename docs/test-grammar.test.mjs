import { createHighlighter } from "shiki";
import { strict as assert } from "assert";
import miseTomlGrammar from "./.vitepress/grammars/mise-toml.tmLanguage.json" with { type: "json" };
import kdlGrammar from "./.vitepress/grammars/kdl.tmLanguage.json" with { type: "json" };

const code = `[tasks.deploy]
usage = '''
arg "<environment>" help="Target environment"
flag "-v --verbose"
'''
run = '''
echo "Deploying"
./deploy.sh
'''`;

console.log("Testing mise-toml grammar...");

try {
  const highlighter = await createHighlighter({
    themes: ["github-dark"],
    langs: [
      "shell",
      "bash",
      "toml",
      {
        ...kdlGrammar,
        name: "kdl",
        scopeName: "source.kdl",
      },
      {
        ...miseTomlGrammar,
        name: "mise-toml",
        aliases: ["mise.toml"],
        scopeName: "source.mise-toml",
      },
    ],
  });

  const html = highlighter.codeToHtml(code, {
    lang: "mise-toml",
    theme: "github-dark",
  });

  // Test that KDL keywords are highlighted (green color)
  assert.ok(
    html.includes("color:#85E89D"),
    "KDL keywords (arg/flag) should be highlighted green",
  );

  // Test that shell commands are highlighted (blue color)
  assert.ok(
    html.includes("color:#79B8FF"),
    "Shell commands (echo) should be highlighted blue",
  );

  // Test that strings are highlighted (blue color)
  assert.ok(html.includes("color:#9ECBFF"), "Strings should be highlighted");

  // Test that TOML structure is present (may be HTML escaped)
  assert.ok(
    html.includes("tasks") && html.includes("deploy"),
    "TOML structure should be preserved",
  );

  console.log("✓ All tests passed!");
  console.log("✓ KDL syntax highlighting working in usage fields");
  console.log("✓ Bash syntax highlighting working in run fields");
  console.log("✓ TOML structure properly parsed");

  process.exit(0);
} catch (error) {
  console.error("✗ Test failed:", error.message);
  process.exit(1);
}
