use crate::config::Settings;
use crate::shorthand_list::SHORTHAND_LIST;
use once_cell::sync::Lazy;
use std::collections::HashMap;

pub fn shorthand_to_repository(settings: &Settings, name: &str) -> Option<&'static str> {
    if !settings.disable_default_shorthands {
        SHORTHAND_MAP.get(name).copied()
    } else {
        None
    }
}

pub static SHORTHAND_MAP: Lazy<HashMap<&'static str, &'static str>> =
    Lazy::new(|| HashMap::from(SHORTHAND_LIST));
