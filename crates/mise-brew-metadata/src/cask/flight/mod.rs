use std::collections::BTreeMap;
use std::path::{Component, Path};

use eyre::{Result, bail, eyre};
use serde_json::Value;

use super::{helpers::*, model::*, types::*};

mod path;
mod step;

pub use path::parse_run_command;
use path::*;
pub use step::parse_flight_step;

pub(super) fn parse_flight_steps(
    cask: &Cask,
    value: &Value,
    kind: &str,
) -> Result<Option<Vec<FlightStep>>> {
    let Some(metadata) = value.as_object().and_then(|o| o.get(kind)) else {
        return Ok(None);
    };
    let groups = metadata.as_array().ok_or_else(|| {
        eyre!(
            "brew-cask:{}: unsupported {kind} metadata format",
            cask.token
        )
    })?;
    let mut steps = Vec::new();
    for group in groups {
        let group = group.as_object().ok_or_else(|| {
            eyre!(
                "brew-cask:{}: unsupported {kind} metadata format",
                cask.token
            )
        })?;
        reject_unsupported_flight_fields(cask, kind, "step group", group, &["steps"])?;
        let group_steps = group
            .get("steps")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                eyre!(
                    "brew-cask:{}: unsupported {kind} metadata format",
                    cask.token
                )
            })?;
        for step in group_steps {
            steps.push(parse_flight_step(cask, kind, step)?);
        }
    }
    Ok(Some(steps))
}
