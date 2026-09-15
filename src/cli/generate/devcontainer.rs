use std::collections::HashMap;

use crate::{
    dirs,
    file::{self, display_path},
    git::Git,
};
use serde::Serialize;

/// Generate devcontainer configuration for mise
///
/// Prints JSON by default. `--write` saves .devcontainer/devcontainer.json;
/// review the image, mounts, and generated setup commands before opening it.
#[derive(Debug, usage_rs::Args)]
#[usage(verbatim_doc_comment, example(r###"mise generate devcontainer"###))]
pub(super) struct Devcontainer {
    /// The image to use for the devcontainer
    #[usage(long, short, verbatim_doc_comment)]
    image: Option<String>,

    /// Bind the mise-data-volume to the devcontainer
    #[usage(long, short, verbatim_doc_comment)]
    mount_mise_data: bool,

    /// The name of the devcontainer
    #[usage(long, short, verbatim_doc_comment)]
    name: Option<String>,

    /// Write to .devcontainer/devcontainer.json
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
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "postCreateCommand")]
    post_create_command: Option<String>,
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

        let mut post_create_command: Option<String> = None;
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
            post_create_command = Some("sudo chown -R vscode:vscode /mnt/mise-data".to_string());
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
