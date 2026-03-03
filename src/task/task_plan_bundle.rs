use crate::Result;
use crate::config::Config;
use crate::task::task_execution_plan::{
    ExecutionPlan, execution_plan_config_files, execution_plan_hash,
};
use crate::task::task_fetcher::TaskFetcher;
use crate::task::task_list::{get_task_lists, resolve_depends};
use crate::task::task_plan_analysis::{
    ChangeImpact, ContentionAnalysis, GraphCycle, analyze_changed_impact, analyze_contention,
    detect_cycle,
};
use crate::task::task_planner::build_execution_plan;
use crate::task::{Deps, Task};
use std::sync::Arc;

#[derive(Debug, Clone, Default)]
pub struct PlanBuildRequest {
    pub requested_task_specs: Vec<String>,
    pub cli_args: Vec<String>,
    pub trailing_args: Vec<String>,
    pub changed_files: Vec<String>,
    pub jobs: usize,
    pub task_list_with_context: bool,
    pub task_list_skip_deps: bool,
    pub deps_skip_deps: bool,
    pub fetch_remote: bool,
    pub no_cache: bool,
}

#[derive(Debug, Clone)]
pub struct PlanBuildResolvedTasksRequest {
    pub requested_task_specs: Vec<String>,
    pub resolved_cli_args: Vec<String>,
    pub resolved_tasks: Vec<Task>,
    pub changed_files: Vec<String>,
    pub jobs: usize,
    pub deps_skip_deps: bool,
    pub fetch_remote: bool,
    pub no_cache: bool,
}

#[derive(Debug, Clone)]
pub struct ExecutionPlanBundle {
    pub requested_task_specs: Vec<String>,
    pub resolved_cli_args: Vec<String>,
    pub deps: Deps,
    pub cycle: Option<GraphCycle>,
    pub plan: Option<ExecutionPlan>,
    pub plan_hash: Option<String>,
    pub config_files: Vec<String>,
    pub change_impact: ChangeImpact,
    pub contention: Option<ContentionAnalysis>,
}

pub async fn build_execution_plan_bundle(
    config: &Arc<Config>,
    request: PlanBuildRequest,
) -> Result<ExecutionPlanBundle> {
    let resolved_cli_args = normalized_cli_args(&request.cli_args);
    let mut task_list = get_task_lists(
        config,
        &resolved_cli_args,
        request.task_list_with_context,
        request.task_list_skip_deps,
    )
    .await?;

    if !request.trailing_args.is_empty() {
        for task in &mut task_list {
            task.args.extend(request.trailing_args.clone());
        }
    }

    let resolved_tasks = resolve_depends(config, task_list).await?;
    build_execution_plan_bundle_from_resolved_tasks(
        config,
        PlanBuildResolvedTasksRequest {
            requested_task_specs: request.requested_task_specs,
            resolved_cli_args,
            resolved_tasks,
            changed_files: request.changed_files,
            jobs: request.jobs,
            deps_skip_deps: request.deps_skip_deps,
            fetch_remote: request.fetch_remote,
            no_cache: request.no_cache,
        },
    )
    .await
}

pub async fn build_execution_plan_bundle_from_resolved_tasks(
    config: &Arc<Config>,
    request: PlanBuildResolvedTasksRequest,
) -> Result<ExecutionPlanBundle> {
    let mut resolved_tasks = request.resolved_tasks;
    if request.fetch_remote {
        TaskFetcher::new(request.no_cache)
            .fetch_tasks(&mut resolved_tasks)
            .await?;
    }

    let deps = Deps::new_with_skip(config, resolved_tasks.clone(), request.deps_skip_deps).await?;
    let cycle = detect_cycle(&deps.graph);
    let plan = if cycle.is_none() {
        Some(build_execution_plan(&deps)?)
    } else {
        None
    };
    let plan_hash = plan.as_ref().and_then(|p| execution_plan_hash(p).ok());
    let config_files = plan
        .as_ref()
        .map(execution_plan_config_files)
        .unwrap_or_default();
    let change_impact = analyze_changed_impact(&deps.graph, &request.changed_files);
    let contention = plan.as_ref().map(|p| analyze_contention(p, request.jobs));

    Ok(ExecutionPlanBundle {
        requested_task_specs: request.requested_task_specs,
        resolved_cli_args: request.resolved_cli_args,
        deps,
        cycle,
        plan,
        plan_hash,
        config_files,
        change_impact,
        contention,
    })
}

pub fn join_task_specs_for_cli(task_specs: &[String]) -> Vec<String> {
    let mut args = Vec::new();
    for (idx, task) in task_specs.iter().enumerate() {
        if idx > 0 {
            args.push(":::".to_string());
        }
        args.push(task.clone());
    }
    args
}

pub fn normalized_cli_args(args: &[String]) -> Vec<String> {
    if args.is_empty() {
        vec!["default".to_string()]
    } else {
        args.to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_join_task_specs_for_cli_inserts_group_separator() {
        let specs = vec![
            "lint".to_string(),
            "test --all".to_string(),
            "build".to_string(),
        ];
        assert_eq!(
            join_task_specs_for_cli(&specs),
            vec![
                "lint".to_string(),
                ":::".to_string(),
                "test --all".to_string(),
                ":::".to_string(),
                "build".to_string()
            ]
        );
    }

    #[test]
    fn test_normalized_cli_args_defaults_to_default_task() {
        assert_eq!(normalized_cli_args(&[]), vec!["default".to_string()]);
        assert_eq!(
            normalized_cli_args(&["build".to_string()]),
            vec!["build".to_string()]
        );
    }
}
