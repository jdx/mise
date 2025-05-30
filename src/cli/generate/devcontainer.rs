use std::collections::HashMap;

use crate::{
    config::Settings,
    file::{self, display_path},
    git::Git,
};
use serde::Serialize;

/// [experimental] Generate a devcontainer to execute mise
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Devcontainer {
    /// The name of the devcontainer
    #[clap(long, short, verbatim_doc_comment)]
    name: Option<String>,

    /// The image to use for the devcontainer
    #[clap(long, short, verbatim_doc_comment)]
    image: Option<String>,

    /// Bind the mise-data-volume to the devcontainer
    #[clap(long, short, verbatim_doc_comment)]
    mount_mise_data: bool,

    /// write to .devcontainer/devcontainer.json
    #[clap(long, short)]
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
    pub async fn run(self) -> eyre::Result<()> {
        Settings::get().ensure_experimental("generate devcontainer")?;
        let output = self.generate()?;

        if self.write {
            let path = Git::get_root()?.join(".devcontainer/devcontainer.json");
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

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise generate devcontainer</bold>
"#
);
