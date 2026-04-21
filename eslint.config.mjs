import compat from "eslint-plugin-compat";
import figLinter from "@withfig/eslint-plugin-fig-linter";
import tseslint from "typescript-eslint";

export default tseslint.config(
  {
    files: ["xtasks/fig/**/*.ts"],
    extends: [
      ...tseslint.configs.recommended,
      compat.configs["flat/recommended"],
    ],
    plugins: {
      "@withfig/fig-linter": figLinter,
    },
    languageOptions: {
      ecmaVersion: 2020,
      sourceType: "module",
      globals: {
        Fig: "readonly",
      },
    },
    rules: {
      "@typescript-eslint/explicit-module-boundary-types": "off",
      "no-unused-vars": "off",
      "no-var": "off",
      "@typescript-eslint/no-unused-vars": "off",
      "@withfig/fig-linter/no-malicious-script": "error",
    },
  },
  {
    files: ["xtasks/fig/src/**/*.ts"],
    rules: {
      "@withfig/fig-linter/no-useless-insertvalue": "error",
      "@withfig/fig-linter/no-empty-array-values": "error",
      "@withfig/fig-linter/no-name-equals": "error",
    },
  },
);
