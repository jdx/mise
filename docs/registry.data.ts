import * as fs from "node:fs";
import { load } from "js-toml";
import { parse as parseYaml } from "yaml";

type Registry = {
  tools: Record<
    string,
    {
      aliases?: string[];
      backends: (
        | string
        | {
            full: string;
            platforms?: string[];
            options?: Record<string, string>;
          }
      )[];
      os?: string[];
      url?: string;
    }
  >;
};

/** Verification a backend performs beyond checksums, as shown on the registry page. */
type Verification =
  "packslip" | "github-attestations" | "slsa" | "cosign" | "minisign";

type Tool = {
  short: string;
  url: string;
  backends: { name: string; url: string; verification: Verification[] }[];
  aliases: string[];
  os: string[];
};

type AquaEnabled = { enabled?: boolean };
type AquaCosign = AquaEnabled & { key?: unknown; bundle?: unknown };
type AquaChecksum = AquaEnabled & {
  cosign?: AquaCosign;
  minisign?: AquaEnabled;
  github_artifact_attestations?: AquaEnabled;
};
type AquaPackage = {
  name?: string;
  repo_owner?: string;
  repo_name?: string;
  path?: string;
  aliases?: { name: string }[];
  checksum?: AquaChecksum;
  cosign?: AquaCosign;
  minisign?: AquaEnabled;
  slsa_provenance?: AquaEnabled;
  github_artifact_attestations?: AquaEnabled;
  version_overrides?: Omit<AquaPackage, "version_overrides">[];
};

const isEnabled = (config?: AquaEnabled) =>
  config != null && config.enabled !== false;

// mise verifies Cosign natively only from a key or a signature bundle; it does
// not run arbitrary `opts`. Keep in sync with `AquaBackend::has_native_cosign`.
const hasNativeCosign = (cosign?: AquaCosign) =>
  isEnabled(cosign) && (cosign?.key != null || cosign?.bundle != null);

/**
 * Read the verification the vendored aqua registry configures for each package.
 *
 * This mirrors the registry-config half of `AquaBackend::security_info`, checking
 * the base package and every version override. It omits that function's
 * release-asset heuristics, so the page only claims checks mise performs.
 */
function loadAquaVerification(): Map<string, Verification[]> {
  const raw = fs.readFileSync("./vendor/aqua-registry/registry.yml", "utf-8");
  const { packages } = parseYaml(raw) as { packages: AquaPackage[] };
  const verification = new Map<string, Verification[]>();
  for (const pkg of packages) {
    const id =
      pkg.name ??
      (pkg.repo_owner && pkg.repo_name
        ? `${pkg.repo_owner}/${pkg.repo_name}`
        : pkg.path);
    if (!id) continue;

    const all = [pkg, ...(pkg.version_overrides ?? [])];
    const checksum = (p: (typeof all)[number]) =>
      isEnabled(p.checksum) ? p.checksum : undefined;
    const features: Verification[] = [];
    if (
      all.some(
        (p) =>
          isEnabled(p.github_artifact_attestations) ||
          isEnabled(checksum(p)?.github_artifact_attestations),
      )
    ) {
      features.push("github-attestations");
    }
    if (all.some((p) => isEnabled(p.slsa_provenance))) features.push("slsa");
    if (
      all.some(
        (p) =>
          hasNativeCosign(p.cosign) || hasNativeCosign(checksum(p)?.cosign),
      )
    ) {
      features.push("cosign");
    }
    if (
      all.some((p) => isEnabled(p.minisign) || isEnabled(checksum(p)?.minisign))
    ) {
      features.push("minisign");
    }

    for (const name of [id, ...(pkg.aliases ?? []).map((a) => a.name)]) {
      verification.set(name, features);
    }
  }
  return verification;
}

export default {
  watch: ["../registry/*.toml", "../vendor/aqua-registry/registry.yml"],
  load() {
    const registryDir = "./registry";
    const files = fs
      .readdirSync(registryDir)
      .filter((f) => f.endsWith(".toml"))
      .sort();

    const tools: Registry["tools"] = {};
    for (const file of files) {
      const toolName = file.replace(/\.toml$/, "");
      const raw = fs.readFileSync(`${registryDir}/${file}`, "utf-8");
      const toolInfo = load(raw) as Registry["tools"][string];
      tools[toolName] = toolInfo;
    }

    const registry: Record<string, Tool> = {};
    const aquaVerification = loadAquaVerification();

    const urlBuilders: Record<
      string,
      (slug: string, options: Record<string, string>) => string
    > = {
      aqua: (slug) => {
        const repoName = slug.split("/").slice(0, 2).join("/");
        return `https://github.com/${repoName}`;
      },
      asdf: (slug) =>
        slug.startsWith("http") ? slug : `https://github.com/${slug}`,
      conda: (slug, options) =>
        `https://anaconda.org/${options.channel ?? "conda-forge"}/${slug}`,
      cargo: (slug) => `https://crates.io/crates/${slug}`,
      core: (slug) => `https://mise.jdx.dev/lang/${slug}.html`,
      dotnet: (slug) => `https://www.nuget.org/packages/${slug}`,
      gem: (slug) => `https://rubygems.org/gems/${slug}`,
      github: (slug) => `https://github.com/${slug}`,
      gitlab: (slug) => `https://gitlab.com/${slug}`,
      go: (slug) => `https://pkg.go.dev/${slug}`,
      npm: (slug) => `https://www.npmjs.com/package/${slug}`,
      pipx: (slug) => `https://pypi.org/project/${slug}`,
      spm: (slug, options) =>
        slug.startsWith("http")
          ? slug
          : `https://${options.provider == "gitlab" ? "gitlab.com" : "github.com"}/${slug}`,
      http: () => "",
      ubi: (slug, options) => {
        const repoName = slug.split("/").slice(0, 2).join("/");
        return `https://${
          options.provider === "gitlab" ? "gitlab.com" : "github.com"
        }/${repoName}`;
      },
      vfox: (slug) => `https://github.com/${slug}`,
    };

    const nameRegex = /^(?<prefix>.+?):(?<slug>.+?)(?:\[(?<options>.+)\])?$/;

    for (const key in tools) {
      const tool = tools[key];

      const backends = tool.backends.map((backend) => {
        const name = typeof backend === "string" ? backend : backend.full;
        const match = name.match(nameRegex);
        const prefix = match?.groups?.prefix ?? "";
        const slug = match?.groups?.slug ?? "";
        const options = {
          ...(typeof backend === "object" && backend.options
            ? backend.options
            : {}),
          ...(match?.groups?.options
            ? Object.fromEntries(
                match.groups.options.split(",").map((opt) => {
                  const [k, v] = opt.split("=");
                  return [k, v];
                }),
              )
            : {}),
        };
        const verification: Verification[] =
          prefix === "packslip"
            ? ["packslip"]
            : prefix === "aqua"
              ? (aquaVerification.get(slug) ?? [])
              : [];
        return {
          name: `${prefix}:${slug}`,
          url: urlBuilders[prefix] ? urlBuilders[prefix](slug, options) : "",
          verification,
        };
      });

      registry[key] = {
        short: key,
        // Prefer the registry's `url`; the backend URLs are guesses from the backend
        // slug, and some backends (such as http) have none. Only http(s) links are
        // rendered, matching the check build.rs applies to `url`.
        url:
          [tool.url, ...backends.map((backend) => backend.url)].find((url) =>
            /^https?:\/\/[^{}\s]+$/.test(url ?? ""),
          ) ?? "",
        backends,
        aliases: tool.aliases ?? [],
        os: tool.os ?? [],
      };
    }

    return Object.values(registry).sort((a, b) =>
      a.short.localeCompare(b.short, "en"),
    );
  },
};
