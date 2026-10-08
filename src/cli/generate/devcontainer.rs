use std::collections::HashMap;

use crate::{
    dirs,
    file::{self, display_path},
    git::Git,
};
use serde::Serialize;

/// Generate a devcontainer configuration that installs mise and the project's tools
///
/// Prints JSON by default. `--write` saves .devcontainer/devcontainer.json in the
/// repository root (or the current directory outside a git repository), replacing
/// an existing file. Review the image, mounts, and setup commands before opening it.
#[derive(Debug, usage_rs::Args)]
#[usage(
    verbatim_doc_comment,
    example("mise generate devcontainer", help = "Print the configuration"),
    example(
        "mise generate devcontainer --mount-mise-data --write",
        help = "Keep installed tools in a volume and save .devcontainer/devcontainer.json"
    )
)]
pub(super) struct Devcontainer {
    /// Base image; defaults to mcr.microsoft.com/devcontainers/base:ubuntu
    #[usage(long, short, verbatim_doc_comment)]
    image: Option<String>,

    /// Keep installed tools in a `mise-data-volume` Docker volume so they survive rebuilds
    ///
    /// Mounts the volume at /mnt/mise-data, sets MISE_DATA_DIR to it, and adds its
    /// shims directory to PATH.
    #[usage(long, short, verbatim_doc_comment)]
    mount_mise_data: bool,

    /// Container name; defaults to mise
    #[usage(long, short, verbatim_doc_comment)]
    name: Option<String>,

    /// Write to .devcontainer/devcontainer.json instead of printing
    #[usage(long, short)]
    write: bool,
}

#[derive(Serialize)]
struct DevcontainerTemplate {
    name: String,
    image: String,
    features: HashMap<String, HashMap<String, String>>,
    customizations: HashMap<String, HashMap<String, Vec<String>>>,
    mounts: Vec<DevcontainerMount>,
    #[serde(rename = "containerEnv")]
    container_env: HashMap<String, String>,
    #[serde(rename = "remoteEnv")]
    remote_env: HashMap<String, String>,
    #[serde(rename = "postCreateCommand")]
    post_create_command: String,
}

#[derive(Serialize)]
struct DevcontainerMount {
    source: String,
    target: String,
    #[serde(rename = "type")]
    type_field: String,
}

impl Devcontainer {
    pub(super) async fn run(self) -> eyre::Result<()> {
        let output = self.generate()?;

        if self.write {
            let path = match Git::get_root() {
                Ok(root) => root.join(".devcontainer/devcontainer.json"),
                Err(_) => dirs::CWD
                    .as_ref()
                    .unwrap()
                    .join(".devcontainer/devcontainer.json"),
            };
            file::create(&path)?;
            file::write(&path, &output)?;
            miseprintln!("Wrote to {}", display_path(&path));
        } else {
            miseprintln!("{output}");
        }

        Ok(())
    }

    fn generate(&self) -> eyre::Result<String> {
        let name = self.name.as_deref().unwrap_or("mise");
        let image = self
            .image
            .as_deref()
            .unwrap_or("mcr.microsoft.com/devcontainers/base:ubuntu");

        // The mise feature installs mise but not the project's tools, so
        // install them once the workspace is mounted.
        let mut post_create_command = "mise install".to_string();
        let mut mounts = vec![];
        let mut container_env = HashMap::new();
        let mut remote_env = HashMap::new();
        if self.mount_mise_data {
            mounts.push(DevcontainerMount {
                source: "mise-data-volume".to_string(),
                target: "/mnt/mise-data".to_string(),
                type_field: "volume".to_string(),
            });
            container_env.insert("MISE_DATA_DIR".to_string(), "/mnt/mise-data".to_string());
            remote_env.insert(
                "PATH".to_string(),
                "${containerEnv:PATH}:/mnt/mise-data/shims".to_string(),
            );
            post_create_command =
                format!("sudo chown -R vscode:vscode /mnt/mise-data && {post_create_command}");
        }

        let mut features = HashMap::new();
        features.insert(
            "ghcr.io/devcontainers-extra/features/mise:1".to_string(),
            HashMap::new(),
        );

        let mut customizations = HashMap::new();
        let mut extensions = HashMap::new();

        extensions.insert(
            "extensions".to_string(),
            vec!["hverlin.mise-vscode".to_string()],
        );

        customizations.insert("vscode".to_string(), extensions);

        let template = DevcontainerTemplate {
            name: name.to_string(),
            image: image.to_string(),
            features,
            customizations,
            mounts,
            container_env,
            remote_env,
            post_create_command,
        };

        let output = serde_json::to_string_pretty(&template)?;

        Ok(output)
    }
}
