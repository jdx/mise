use std::collections::HashMap;

use crate::{
    config::SETTINGS,
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
    mounts: Vec<DevcontainerMount>,
    container_env: HashMap<String, String>,
}

#[derive(Serialize)]
struct DevcontainerMount {
    source: String,
    target: String,
    #[serde(rename = "type")]
    type_field: String,
}

impl Devcontainer {
    pub fn run(self) -> eyre::Result<()> {
        SETTINGS.ensure_experimental("generate devcontainer")?;
        let output = self.generate()?;

        if self.write {
            let path = Git::get_root()?.join(".devcontainer/devcontainer.json");
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

        let mut mounts = vec![];
        let mut container_env = HashMap::new();
        if self.mount_mise_data {
            mounts.push(DevcontainerMount {
                source: "mise-data-volume".to_string(),
                target: "/mnt/mise-data".to_string(),
                type_field: "volume".to_string(),
            });
            container_env.insert("MISE_DATA_VOLUME".to_string(), "/mnt/mise-data".to_string());
        }

        let mut features = HashMap::new();
        features.insert(
            "ghcr.io/jdx/devcontainer-features/mise:1".to_string(),
            HashMap::new(),
        );

        let template = DevcontainerTemplate {
            name: name.to_string(),
            image: image.to_string(),
            features,
            mounts,
            container_env,
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
