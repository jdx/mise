use crate::shorthand_list::SHORTHAND_LIST;
use once_cell::sync::Lazy;
use std::collections::HashMap;

pub fn shorthand_to_repository(name: &str) -> Option<&'static str> {
    SHORTHAND_MAP.get(name).copied()
}

pub static SHORTHAND_MAP: Lazy<HashMap<&'static str, &'static str>> =
    Lazy::new(|| HashMap::from(SHORTHAND_LIST));
