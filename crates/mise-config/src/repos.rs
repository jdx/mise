use schemars::JsonSchema;
use serde::Deserialize;

// The doc comments on these types are their descriptions in schema/mise.json, which editors show
// to users. Notes for Rust readers go in plain comments.

/// git repository to clone to this path, then keep on ref when one is set
#[derive(Debug, Default, Clone, Deserialize, JsonSchema)]
pub struct RepoTomlConfig {
    /// git repository URL
    // Optional in the type, required by the schema. An `Option` field is optional to serde
    // without `#[serde(default)]`, which would also stop schemars from marking it required.
    #[schemars(required, length(min = 1), regex(pattern = r".*\S.*"))]
    pub url: Option<String>,
    /// optional branch, tag, or full commit SHA to check out
    #[serde(default, rename = "ref")]
    #[schemars(length(min = 1), regex(pattern = r".*\S.*"))]
    pub git_ref: Option<String>,
    /// partial clone filter passed to `git clone --filter`, such as `blob:none`; applies to the first clone only
    #[serde(default)]
    #[schemars(length(min = 1), regex(pattern = r".*\S.*"))]
    pub filter: Option<String>,
    /// shallow clone depth passed to `git clone --depth`; applies to the first clone only
    #[serde(default)]
    #[schemars(range(min = 1))]
    pub depth: Option<u32>,
    /// clone with `--recurse-submodules`; applies to the first clone only
    #[serde(default)]
    pub submodules: Option<bool>,
}
