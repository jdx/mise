import { FlatCompat } from "@eslint/eslintrc";
import figLinter from "@withfig/eslint-plugin-fig-linter";

const compat = new FlatCompat({
  baseDirectory: import.meta.dirname,
});

const figSpecOverrides = figLinter.configs.recommended.overrides.map(
  (override) => ({
    ...override,
    files: [override.files].flat().map((file) => `xtasks/fig/${file}`),
  })
);

export default [...compat.extends("@fig/autocomplete"), ...figSpecOverrides];
