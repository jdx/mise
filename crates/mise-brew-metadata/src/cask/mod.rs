//! Cask API models and artifact parsing. Installation remains in mise.

mod artifacts;
mod flight;
mod helpers;
mod model;
mod types;

pub use artifacts::{
    cask_artifacts, parse_app_artifact, parse_binary_artifact, parse_command_wrapper_artifact,
    parse_font_artifact, parse_generated_completion_artifact, parse_generic_artifact,
    parse_pkg_artifact,
};
pub use flight::{parse_flight_step, parse_run_command};
pub use helpers::{
    artifact_type, has_lifecycle_hook, is_flight_glob, validate_flight_relative_path,
};
pub use model::{Cask, CaskConflicts, CaskDependencies, CaskManager, CaskUrlSpecs};
pub use types::{
    AppArtifact, BinaryArtifact, CaskArtifacts, CommandWrapperArtifact, CompletionArtifact,
    CompletionShell, FlightGuard, FlightPath, FlightPathBase, FlightStep, FlightSudo, FontArtifact,
    GeneratedCompletionArtifact, GenericArtifact, InstallerArtifact, PkgArtifact, PkgChoice,
    PkgChoiceChange, ProcessMatch,
};

#[cfg(test)]
mod tests {
    use super::*;

    fn cask_with_artifacts(artifacts: serde_json::Value) -> Cask {
        serde_json::from_value(serde_json::json!({
            "token": "example",
            "version": "1.0",
            "url": "https://example.com/example.dmg",
            "artifacts": artifacts,
        }))
        .unwrap()
    }

    #[test]
    fn parses_cask_artifacts_without_mise_startup() {
        let cask = cask_with_artifacts(serde_json::json!([
            {"app": ["Example.app"]},
            {"preflight_steps": [{"steps": [{
                "type": "move",
                "source": {"base": "staged_path", "path": "bin/tool"},
                "target": {"base": "staged_path", "path": "tool"}
            }]}]}
        ]));

        let artifacts = cask_artifacts(&cask).unwrap();
        assert_eq!(artifacts.apps[0].source, "Example.app");
        assert!(matches!(
            artifacts.preflight_steps[0],
            FlightStep::Move { .. }
        ));
    }

    #[test]
    fn rejects_unknown_artifacts() {
        let cask = cask_with_artifacts(serde_json::json!([{"mystery": "item"}]));
        let err = cask_artifacts(&cask).unwrap_err();
        assert!(
            err.to_string()
                .contains("unsupported artifact type mystery")
        );
    }

    #[test]
    fn rejects_flight_paths_outside_the_stage() {
        let cask = cask_with_artifacts(serde_json::json!([{
            "preflight_steps": [{"steps": [{
                "type": "move",
                "source": {"base": "staged_path", "path": "../escape"},
                "target": {"base": "staged_path", "path": "tool"}
            }]}]
        }]));
        let err = cask_artifacts(&cask).unwrap_err();
        assert!(
            err.to_string()
                .contains("invalid preflight_steps source path ../escape"),
            "{err:#}"
        );
    }

    #[test]
    fn requires_receipt_ids_for_pkg_artifacts() {
        let cask = cask_with_artifacts(serde_json::json!([{"pkg": "Example.pkg"}]));
        let err = cask_artifacts(&cask).unwrap_err();
        assert!(err.to_string().contains("pkgutil ids"));
    }
}
