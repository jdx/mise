//! Cask API models and artifact parsing. Installation remains in mise.

mod artifacts;
mod helpers;
pub mod model;
mod types;

pub use artifacts::*;
pub use helpers::*;
pub use model::*;
pub use types::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cask_artifacts_without_mise_startup() {
        let cask: Cask = serde_json::from_value(serde_json::json!({
            "token": "example",
            "version": "1.0",
            "url": "https://example.com/example.dmg",
            "artifacts": [
                {"app": ["Example.app"]},
                {"preflight_steps": [{"steps": [{
                    "type": "move",
                    "source": {"base": "staged_path", "path": "bin/tool"},
                    "target": {"base": "staged_path", "path": "tool"}
                }]}]}
            ]
        }))
        .unwrap();

        let artifacts = cask_artifacts(&cask).unwrap();
        assert_eq!(artifacts.apps[0].source, "Example.app");
        assert!(matches!(
            artifacts.preflight_steps[0],
            FlightStep::Move { .. }
        ));
    }
}
