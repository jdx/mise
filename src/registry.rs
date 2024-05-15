use std::collections::HashMap;

use once_cell::sync::Lazy;

use crate::cli::args::ForgeArg;

const REGISTRY: &[(&str, &str)] = &[
    ("ubi", "cargo:ubi"),
    ("elixir", "asdf:mise-plugins/mise-elixir"),
];

static MAP: Lazy<HashMap<&'static str, ForgeArg>> =
    Lazy::new(|| REGISTRY.iter().map(|(k, v)| (*k, (*v).into())).collect());

pub fn get(s: &str) -> Option<&'static ForgeArg> {
    MAP.get(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get() {
        assert_eq!(get("ubi").unwrap().id, "cargo:ubi");
        assert_eq!(get("unknown"), None);
    }
}
