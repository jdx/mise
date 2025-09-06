use super::{EnvDirective, EnvResolveOptions, ToolsFilter};
use std::path::PathBuf;

pub struct DirectivePlanner;

impl DirectivePlanner {
    pub fn plan(
        directives: &[(EnvDirective, PathBuf)],
        resolve_opts: &EnvResolveOptions,
    ) -> Vec<(EnvDirective, PathBuf)> {
        let last_python_venv = directives
            .iter()
            .rev()
            .find_map(|(d, _)| matches!(d, EnvDirective::PythonVenv { .. }).then_some(d));

        directives
            .iter()
            .fold(Vec::new(), |mut acc, (directive, source)| {
                let should_include = match resolve_opts.tools {
                    ToolsFilter::ToolsOnly => directive.options().tools,
                    ToolsFilter::NonToolsOnly => !directive.options().tools,
                    ToolsFilter::Both => true,
                };
                if !should_include {
                    return acc;
                }

                if let Some(d) = &last_python_venv {
                    if matches!(directive, EnvDirective::PythonVenv { .. }) && **d != *directive {
                        return acc;
                    }
                }

                acc.push((directive.clone(), source.clone()));
                acc
            })
    }
}
