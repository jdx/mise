use std::str::FromStr;
use eyre::Result;

mod setting;

#[derive(Debug, clap::Args)]
#[clap(about = "Manage settings")]
pub struct Settings {
   setting_vars: Option<Vec<SettingsVarArg>>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct SettingsVarArg {
    pub key: String,
    pub value: Option<String>,
}

impl FromStr for SettingsVarArg {
    type Err = eyre::Error;

    fn from_str(input: &str) -> eyre::Result<Self> {
        let sv = match input.split_once('=') {
            Some((k, v)) => Self {
                key: k.to_string(),
                value: Some(v.to_string())
            },
            None => Self {
                key: input.to_string(),
                value: None
            }
        };
        Ok(sv)
    }
}

impl Settings {
    pub fn run(self) -> Result<()> {
        if let Some(setting_vars) = self.setting_vars {
            for var in setting_vars.iter() {
                if !var.value.is_none() {
                    let kv_pair = var.clone();
                    setting::set_settings(kv_pair.key, kv_pair.value.unwrap().clone())?;
                    continue;
                }
                setting::get_setting(var.key.clone())?;
            }
        } else {
            setting::list_settings()?;
        }
        Ok(())
    }
}
