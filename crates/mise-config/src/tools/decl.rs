//! The declaration types behind the schema for `[tools]` options.
//!
//! Which options a tool takes depends on its backend, which only its key names, so the schema
//! picks a backend's option set with `patternProperties` on the key (`^github:`, `^npm:`, ...), as
//! `schema.rs` lays out. None of that is visible to a derive on
//! [`ToolSpecList`](super::ToolSpecList), whose options are raw until mise checks them: these
//! types describe what each backend's option reader accepts, and nothing parses into them. The doc
//! comments are the descriptions in `schema/mise.json`; notes for Rust readers go in plain
//! comments.

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};

use crate::bootstrap::OneOrManyStrings;
use crate::schema::refs;

#[derive(JsonSchema)]
#[serde(untagged)]
#[schemars(transform = crate::schema::one_of)]
pub enum IntegerOrString {
    Integer(i64),
    String(String),
}

// A name for the executable, or a table of glob = new name.
#[derive(JsonSchema)]
#[serde(untagged)]
#[schemars(transform = crate::schema::one_of)]
pub enum RenameExe {
    Name(String),
    Globs(std::collections::BTreeMap<String, RenameExeTarget>),
}

/// new name for the executables matching this glob
#[derive(JsonSchema)]
pub struct RenameExeTarget(pub String);

/// value of this request header; tera templated
#[derive(JsonSchema)]
pub struct HttpHeaderValue(pub String);

/// hosts this header may follow a redirect to: exact hosts or *.suffix
#[derive(JsonSchema)]
pub struct HttpHeaderForwardHosts(pub OneOrManyStrings);

/// value of this aqua registry template variable
#[derive(JsonSchema)]
pub struct AquaVarValue(pub String);

#[derive(JsonSchema)]
#[serde(untagged)]
#[schemars(transform = crate::schema::one_of)]
pub enum BoolOrStrings {
    Bool(bool),
    One(String),
    Many(Vec<String>),
}

#[derive(JsonSchema)]
#[serde(untagged)]
#[schemars(transform = crate::schema::one_of)]
pub enum WorkflowRefs {
    One(String),
    Many(#[schemars(length(min = 1))] Vec<String>),
}

#[derive(JsonSchema)]
#[schemars(extend("oneOf" = [{"required": ["version"]}, {"required": ["path"]}, {"required": ["prefix"]}, {"required": ["ref"]}]))]
pub struct ToolOptions {
    /// version of the tool to install
    pub version: Option<String>,
    /// path to a custom compiled version
    pub path: Option<String>,
    /// version prefix to resolve
    pub prefix: Option<String>,
    /// version control reference to compile
    #[serde(rename = "ref")]
    pub ref_: Option<String>,
    #[serde(default)]
    #[schemars(schema_with = "refs::os_filter")]
    pub os: Option<Vec<String>>,
    /// minimum time since a version's release before mise selects it, as a duration such as "7d" or an absolute date
    pub minimum_release_age: Option<String>,
    /// deprecated alias for minimum_release_age
    #[schemars(extend("deprecated" = true))]
    pub install_before: Option<String>,
    /// ordering applied to remote versions: "source" keeps the backend's order, "semver" sorts semantic versions
    #[serde(default)]
    #[schemars(schema_with = "tool_options_version_order")]
    pub version_order: Option<toml::Value>,
    /// include pre-release versions when listing versions and resolving latest
    #[serde(default)]
    #[schemars(schema_with = "refs::tool_bool")]
    pub prerelease: Option<bool>,
    /// per-platform option overrides keyed by <os>-<arch>, such as linux-x64 or macos-arm64
    #[serde(default)]
    #[schemars(schema_with = "refs::tool_platforms")]
    pub platforms: Option<std::collections::BTreeMap<String, toml::Value>>,
    /// per-platform option overrides (alias of platforms)
    #[serde(default)]
    #[schemars(schema_with = "refs::tool_platforms")]
    pub platform: Option<std::collections::BTreeMap<String, toml::Value>>,
}

#[derive(JsonSchema)]
pub struct ToolForgeOptions {
    /// release tag prefix stripped from versions and re-added when resolving tags
    pub version_prefix: Option<String>,
    /// glob (* and ?) matched against release asset names, tera templated; replaces autodetection
    pub asset_pattern: Option<String>,
    /// extra archives from the same release extracted over the primary asset, in order
    pub additional_asset_patterns: Option<OneOrManyStrings>,
    /// substring an asset name must contain, applied before platform autodetection
    pub matching: Option<String>,
    /// regex an asset name must match, applied before platform autodetection
    pub matching_regex: Option<String>,
    /// penalize macos .app bundle assets during autodetection
    #[serde(default)]
    #[schemars(schema_with = "refs::tool_bool")]
    pub no_app: Option<bool>,
    /// only expose these binaries on PATH
    pub filter_bins: Option<OneOrManyStrings>,
    /// force the archive format instead of detecting it from the file name
    #[schemars(extend("enum" = ["tar.gz", "tgz", "gz", "tar.xz", "txz", "xz", "tar.bz2", "tbz2", "tbz", "bz2", "tar.zst", "tzst", "zst", "tar", "zip", "vsix", "7z", "tar.br", "tbr", "br", "tar.lz4", "tlz4", "lz4", "tar.sz", "tsz", "sz", "rar", "raw"]))]
    pub format: Option<String>,
    /// expected artifact digest as algo:hash (sha256, sha512, sha1, md5, blake3)
    pub checksum: Option<String>,
    /// expected artifact size in bytes
    pub size: Option<IntegerOrString>,
    /// leading directory components to strip on extract (auto 1 for single-root archives)
    pub strip_components: Option<IntegerOrString>,
    /// executable name (or safe relative path) to install the downloaded binary as
    pub bin: Option<String>,
    /// rename the extracted executable: a name, or a table of glob = new name
    pub rename_exe: Option<RenameExe>,
    /// directory inside the install that contains binaries (tera templated)
    pub bin_path: Option<String>,
}

#[derive(JsonSchema)]
pub struct ToolGithubOptions {
    /// api base url, for github enterprise or self-hosted instances
    pub api_url: Option<String>,
    /// verify github artifact attestations when available
    #[serde(default)]
    #[schemars(schema_with = "refs::tool_bool")]
    pub github_attestations: Option<bool>,
    /// expected fulcio certificate identity for slsa provenance, tera templated
    pub slsa_signer_identity: Option<String>,
    /// expected oidc issuer for slsa provenance; requires slsa_signer_identity
    pub slsa_signer_issuer: Option<String>,
}

#[derive(JsonSchema)]
pub struct ToolGitlabOptions {
    /// api base url, for self-hosted gitlab instances
    pub api_url: Option<String>,
}

#[derive(JsonSchema)]
pub struct ToolForgejoOptions {
    /// api base url, for self-hosted forgejo or gitea instances
    pub api_url: Option<String>,
}

#[derive(JsonSchema)]
pub struct ToolUbiOptions {
    /// name of the executable inside the archive to extract
    pub exe: Option<String>,
    /// name to give the extracted executable
    pub rename_exe: Option<String>,
    /// substring to match release filenames when several match the os and arch
    pub matching: Option<String>,
    /// regex release filenames must match before os and arch matching
    pub matching_regex: Option<String>,
    /// forge used to fetch releases and assets
    #[schemars(extend("enum" = ["github", "gitlab"]))]
    pub provider: Option<String>,
    /// provider api base url, for self-hosted instances
    pub api_url: Option<String>,
    /// regex release tags must match to be listed as versions
    pub tag_regex: Option<String>,
    /// extract every file in the archive instead of only the binary
    #[serde(default)]
    #[schemars(schema_with = "refs::tool_bool")]
    pub extract_all: Option<bool>,
    /// directory in the install containing binaries, used with extract_all
    pub bin_path: Option<String>,
}

#[derive(JsonSchema)]
pub struct ToolHttpOptions {
    /// download url, tera templated, http(s) or file://; required for each platform
    pub url: Option<String>,
    /// expected artifact digest as algo:hash
    pub checksum: Option<String>,
    /// checksum file or manifest url, used by mise lock without downloading
    pub checksum_url: Option<String>,
    /// expr-lang expression extracting algo:hash from the checksum_url manifest
    pub checksum_expr: Option<String>,
    /// request headers sent with the download, version_list_url and checksum_url requests; values are tera templated (e.g. Bearer {{ env.TOKEN }})
    pub headers: Option<std::collections::BTreeMap<String, HttpHeaderValue>>,
    /// hosts each header in headers may follow a redirect to; exact hosts or *.suffix, and a header with no entry is never forwarded
    pub headers_forward: Option<std::collections::BTreeMap<String, HttpHeaderForwardHosts>>,
    /// expected artifact size in bytes
    pub size: Option<IntegerOrString>,
    /// force the archive format instead of detecting it from the url
    #[schemars(extend("enum" = ["tar.gz", "tgz", "gz", "tar.xz", "txz", "xz", "tar.bz2", "tbz2", "tbz", "bz2", "tar.zst", "tzst", "zst", "tar", "zip", "vsix", "7z", "tar.br", "tbr", "br", "tar.lz4", "tlz4", "lz4", "tar.sz", "tsz", "sz", "rar", "raw"]))]
    pub format: Option<String>,
    /// leading directory components to strip on extract (auto 1 for single-root archives)
    pub strip_components: Option<IntegerOrString>,
    /// name for a downloaded raw or compressed single binary (ignored for archives)
    pub bin: Option<String>,
    /// rename the extracted executable: a name, or a table of glob = new name
    pub rename_exe: Option<RenameExe>,
    /// directory inside the install that contains binaries (tera templated)
    pub bin_path: Option<String>,
    /// share extracted content between installs with identical artifacts
    #[serde(default)]
    #[schemars(schema_with = "refs::tool_bool")]
    pub shared_extraction: Option<bool>,
    /// windows only: write a .cmd launcher running the raw script with this interpreter
    pub windows_script_interpreter: Option<String>,
    /// url returning the version list (text, json or html)
    pub version_list_url: Option<String>,
    /// regex extracting versions from the list (first capture group or whole match)
    pub version_regex: Option<String>,
    /// jq-like path extracting versions from json, such as .[].version
    pub version_json_path: Option<String>,
    /// expr-lang expression post-processing versions (variables: body, versions)
    pub version_expr: Option<String>,
    /// url holding release dates; defaults to the version_list_url body
    pub version_date_url: Option<String>,
    /// expr-lang expression returning the url of the next page of version_date_url, or nil after the last (variables: body, url)
    pub version_date_next_expr: Option<String>,
    /// expr-lang expression mapping each version to its release date, as a list of [version, date] pairs or a map of version to date (variables: body, bodies, versions)
    pub version_date_expr: Option<String>,
}

#[derive(JsonSchema)]
pub struct ToolS3Options {
    /// s3://bucket/key object url, tera templated; required for each platform
    pub url: Option<String>,
    /// expected artifact digest as algo:hash
    pub checksum: Option<String>,
    /// expected artifact size in bytes
    pub size: Option<IntegerOrString>,
    /// force the archive format when the object key has no usable extension
    #[schemars(extend("enum" = ["tar.gz", "tgz", "gz", "tar.xz", "txz", "xz", "tar.bz2", "tbz2", "tbz", "bz2", "tar.zst", "tzst", "zst", "tar", "zip", "vsix", "7z", "tar.br", "tbr", "br", "tar.lz4", "tlz4", "lz4", "tar.sz", "tsz", "sz", "rar", "raw"]))]
    pub format: Option<String>,
    /// leading directory components to strip on extract (auto 1 for single-root archives)
    pub strip_components: Option<IntegerOrString>,
    /// executable name (or safe relative path) to install the downloaded binary as
    pub bin: Option<String>,
    /// rename the extracted executable: a name, or a table of glob = new name
    pub rename_exe: Option<RenameExe>,
    /// directory inside the install that contains binaries (tera templated)
    pub bin_path: Option<String>,
    /// aws region used for the s3 client
    pub region: Option<String>,
    /// custom s3-compatible endpoint url (minio and similar); enables path-style addressing
    pub endpoint: Option<String>,
    /// s3:// url of a manifest file listing versions
    pub version_list_url: Option<String>,
    /// s3 key prefix to list objects under for version discovery
    pub version_prefix: Option<String>,
    /// regex extracting versions from the manifest or object keys
    pub version_regex: Option<String>,
    /// jq-like path extracting versions from json, such as .[].version
    pub version_json_path: Option<String>,
    /// expr-lang expression post-processing versions (variables: body, versions)
    pub version_expr: Option<String>,
}

#[derive(JsonSchema)]
pub struct ToolAquaOptions {
    /// expose only the registry-declared binaries through a filtered bin directory
    #[serde(default)]
    #[schemars(schema_with = "refs::tool_bool")]
    pub symlink_bins: Option<bool>,
    /// force strict glibc or musl asset selection on linux
    #[schemars(extend("enum" = ["glibc", "gnu", "musl"]))]
    pub libc: Option<String>,
    /// expected fulcio certificate subject for slsa provenance; requires slsa_signer_issuer
    pub slsa_signer_identity: Option<String>,
    /// expected oidc issuer for slsa provenance; requires slsa_signer_identity
    pub slsa_signer_issuer: Option<String>,
    /// values for aqua registry template variables
    pub vars: Option<std::collections::BTreeMap<String, AquaVarValue>>,
}

#[derive(JsonSchema)]
pub struct ToolCargoOptions {
    /// crate features to enable (cargo install --features); skips cargo-binstall
    pub features: Option<OneOrManyStrings>,
    /// set to false to pass --no-default-features; skips cargo-binstall
    #[serde(rename = "default-features")]
    #[serde(default)]
    #[schemars(schema_with = "refs::tool_bool")]
    pub default_features_: Option<bool>,
    /// binary to install when a crate has several (cargo install --bin)
    pub bin: Option<String>,
    /// rename the installed executable: a name, or a table of glob = new name
    pub rename_exe: Option<RenameExe>,
    /// crate to install when a git repository contains several
    #[serde(rename = "crate")]
    pub crate_: Option<String>,
    /// pass --locked to cargo install (default true)
    #[serde(default)]
    #[schemars(schema_with = "refs::tool_bool")]
    pub locked: Option<bool>,
}

#[derive(JsonSchema)]
pub struct ToolNpmOptions {
    /// dependency packages whose build scripts are approved; true approves all
    pub allow_builds: Option<BoolOrStrings>,
    /// packages or version ranges exempt from aube's trustPolicy no-downgrade check
    pub trust_policy_excludes: Option<OneOrManyStrings>,
    /// approve the requested package past aube's reputation checks
    #[serde(default)]
    #[schemars(schema_with = "refs::tool_bool_text")]
    pub allow_low_downloads: Option<bool>,
    /// dependencies allowed from non-registry sources (git, file, tarball url); true allows all
    pub allow_exotic_deps: Option<BoolOrStrings>,
    /// extra arguments for aube add --global (only with npm.package_manager = aube_cli)
    pub aube_args: Option<String>,
    /// extra arguments for pnpm installs (shell-split)
    pub pnpm_args: Option<String>,
    /// extra arguments for bun installs (shell-split)
    pub bun_args: Option<String>,
    /// extra arguments for npm installs (shell-split)
    pub npm_args: Option<String>,
}

#[derive(JsonSchema)]
pub struct ToolPypiOptions {
    /// python package extras to install
    pub extras: Option<OneOrManyStrings>,
    /// additional python requirements installed into the tool environment (requires uv)
    pub with: Option<OneOrManyStrings>,
    /// extra requirements whose executables are exposed (requires uv 0.8.5+)
    pub expose: Option<OneOrManyStrings>,
    /// uv prerelease policy for dependency resolution (requires uv)
    #[schemars(extend("enum" = ["disallow", "allow", "if-necessary", "explicit"]))]
    pub dependency_prereleases: Option<String>,
    /// mise-managed CPython version for this tool (minor or exact patch)
    pub python: Option<String>,
    /// distribution name for git sources, used to attach extras
    pub package_name: Option<String>,
    /// extra arguments for pipx install (shell-split)
    pub pipx_args: Option<String>,
    /// set to false to install with pipx instead of uv
    #[serde(default)]
    #[schemars(schema_with = "refs::tool_bool")]
    pub uvx: Option<bool>,
    /// extra arguments for uv tool install (shell-split)
    pub uvx_args: Option<String>,
    /// version-listing url containing a {} placeholder for the package name
    pub registry_url: Option<String>,
}

#[derive(JsonSchema)]
pub struct ToolGemOptions {
    /// gem registry http(s) url used to resolve and install this gem
    pub source: Option<String>,
}

#[derive(JsonSchema)]
pub struct ToolGoOptions {
    /// build tags passed to go install -tags
    pub tags: Option<OneOrManyStrings>,
}

#[derive(JsonSchema)]
pub struct ToolCondaOptions {
    /// conda channel to resolve the package from (default conda-forge)
    pub channel: Option<String>,
}

#[derive(JsonSchema)]
pub struct ToolSpmOptions {
    /// git host used for release info and assets
    #[schemars(extend("enum" = ["github", "gitlab"]))]
    pub provider: Option<String>,
    /// provider api url, for self-hosted instances
    pub api_url: Option<String>,
    /// true requires an artifactbundle, false forces a source build, unset tries the bundle first
    #[serde(default)]
    #[schemars(schema_with = "refs::tool_bool")]
    pub artifactbundle: Option<bool>,
    /// specific *.artifactbundle.zip release asset to use
    pub artifactbundle_asset: Option<String>,
    /// executable products to install
    pub filter_bins: Option<OneOrManyStrings>,
    /// command run in the checkout instead of swift build; incompatible with filter_bins
    pub install_command: Option<String>,
}

#[derive(JsonSchema)]
pub struct ToolPackslipOptions {
    /// publisher-declared alternative build to select, such as fips
    pub variant: Option<String>,
    /// minisign public key line or path to a .pub file pinning the signer
    pub pubkey: Option<String>,
    /// exact keyless signer certificate identity
    pub identity: Option<String>,
    /// keyless signer certificate identity prefix
    pub identity_prefix: Option<String>,
    /// repository workflow file, or a list of them, that signs releases on a tag (or on the ref a name ends with, as in release.yml@refs/heads/main); github.com projects only, not valid with pubkey, identity, identity_prefix, or issuer
    pub workflow: Option<WorkflowRefs>,
    /// separate identity prefix for the vendor release list; needs an issuer, not valid with pubkey
    pub list_identity_prefix: Option<String>,
    /// oidc issuer for the keyless signer
    pub issuer: Option<String>,
    /// accept key-signed bundles without transparency-log evidence
    #[serde(default)]
    #[schemars(schema_with = "refs::tool_bool")]
    pub allow_unlogged: Option<bool>,
    /// install despite confirmed host requirement failures
    #[serde(default)]
    #[schemars(schema_with = "refs::tool_bool")]
    pub ignore_requirements: Option<bool>,
    /// vendor exempts this tool from configured packslip stampers
    #[schemars(extend("enum" = ["vendor"]))]
    pub trust: Option<String>,
}

#[derive(JsonSchema)]
pub struct ToolSpinelOptions {
    /// ruby file to compile, relative to the repository root (default main.rb)
    pub entrypoint: Option<String>,
    /// name of the executable written to bin/ (default: the repository name)
    pub bin: Option<String>,
    /// text before the version in each git tag, such as v
    pub tag_prefix: Option<String>,
    /// full 40-character commit SHA to build instead of the tag
    #[schemars(regex(pattern = r"^[0-9a-fA-F]{40}$"))]
    pub source_ref: Option<String>,
    /// spinel compiler to run (default: spinel on PATH)
    pub spinel: Option<String>,
}

#[derive(JsonSchema)]
pub struct ToolCorePythonOptions {
    /// patch sysconfig paths of precompiled python after install (default true, unix only)
    #[serde(default)]
    #[schemars(schema_with = "refs::tool_bool")]
    pub patch_sysconfig: Option<bool>,
    /// virtualenv path to activate; use env._.python.venv instead
    #[schemars(extend("deprecated" = true))]
    pub virtualenv: Option<String>,
}

#[derive(JsonSchema)]
pub struct ToolCoreJavaOptions {
    /// jdk metadata channel: ga (default) or ea for early access builds
    #[schemars(extend("enum" = ["ga", "ea"]))]
    pub release_type: Option<String>,
}

#[derive(JsonSchema)]
pub struct ToolCoreRustOptions {
    /// rustup profile to install, such as minimal, default or complete
    pub profile: Option<String>,
    /// rustup components to install
    pub components: Option<OneOrManyStrings>,
    /// rustup targets to install, for cross-compilation
    pub targets: Option<OneOrManyStrings>,
    /// wrap cargo with mr boxington (mbx); needs mr-boxington in tools (default false)
    #[serde(default)]
    #[schemars(schema_with = "refs::tool_bool")]
    pub mr_boxington: Option<bool>,
}

#[derive(JsonSchema)]
pub struct ToolCoreDotnetOptions {
    /// install a shared runtime instead of the sdk; the version is the runtime version
    #[schemars(extend("enum" = ["dotnet", "aspnetcore", "windowsdesktop"]))]
    pub runtime: Option<String>,
}

fn tool_options_version_order(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "enum": [
            "source",
            "semver"
        ]
    })
}
