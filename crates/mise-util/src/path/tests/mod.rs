use super::*;

fn sv(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| s.to_string()).collect()
}

#[cfg(windows)]
fn env_with_path(path: &str) -> std::collections::BTreeMap<String, String> {
    let mut env = std::collections::BTreeMap::new();
    env.insert((*crate::env::PATH_KEY).to_string(), path.to_string());
    env.insert("OTHER".to_string(), "unchanged".to_string());
    env
}

mod display_shell;
mod split_paths;
#[cfg(windows)]
mod windows_resolution;
