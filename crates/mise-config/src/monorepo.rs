//! `[monorepo]`: how task discovery finds the projects of a monorepo.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use indexmap::IndexMap;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::task::TaskTemplate;

// The doc comments on these types are their descriptions in schema/mise.json, which editors show
// to users. Notes for Rust readers go in plain comments.

/// Configuration for monorepo task discovery
#[derive(Debug, Default, Clone, Deserialize, JsonSchema)]
#[schemars(extend("unevaluatedProperties" = false))]
pub struct MonorepoConfig {
    /// Explicit list of config root paths for monorepo task discovery. Supports single-level glob patterns (*). When set, skips filesystem walking for better performance.
    pub config_roots: Option<Vec<String>>,
    /// Short names for configured monorepo task roots. Keys are single path segments; values are paths in config_roots.
    #[serde(default)]
    #[schemars(with = "BTreeMap<String, MonorepoPathAlias>")]
    pub path_aliases: BTreeMap<String, String>,
    /// Use a single lockfile at the monorepo root for descendant config roots. true opts in now, false keeps lockfiles next to subproject configs, and unset keeps the current behavior until the scheduled default flip.
    // None follows the rollout default; true opts in, false keeps colocated locks.
    pub lockfile: Option<bool>,
    /// Task defaults applied by task name across inferred and explicit workspace projects
    #[serde(default)]
    #[schemars(schema_with = "task_defaults")]
    pub task_defaults: IndexMap<String, TaskTemplate>,
    /// Explicit additions, removals, and overrides applied to provider-inferred workspace projects
    // Consumed when workspace providers are connected to task loading.
    #[serde(default)]
    pub projects: BTreeMap<String, WorkspaceProjectOverride>,
}

/// Config root this alias names, as listed in config_roots
#[derive(JsonSchema)]
pub struct MonorepoPathAlias(pub String);

fn task_defaults(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({
        "type": "object",
        "additionalProperties": {"$ref": "#/$defs/task_template"}
    })
}

// Explicit changes applied after workspace providers discover their projects.
/// Changes to the workspace project with this ID, or a new project when the ID is new
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, JsonSchema)]
#[serde(default, deny_unknown_fields)]
#[schemars(transform = crate::schema::unevaluated)]
pub struct WorkspaceProjectOverride {
    /// Remove this project and every dependency edge connected to it
    pub remove: bool,
    /// Add the project at this workspace-relative root or replace its inferred root
    pub root: Option<PathBuf>,
    /// Replace provider-inferred project metadata
    pub metadata: Option<BTreeMap<String, String>>,
    /// Replace the complete provider-inferred dependency set
    pub depends: Option<BTreeSet<String>>,
    /// Add dependency edges after an optional depends replacement
    pub depends_add: BTreeSet<String>,
    /// Remove dependency edges after an optional depends replacement
    pub depends_remove: BTreeSet<String>,
}
