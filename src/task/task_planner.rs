use crate::task::task_execution_plan::{ExecutionPlan, ExecutionStage, PlannedTask};
use crate::task::task_helpers::classify_ready_tasks;
use crate::task::task_identity::TaskIdentity;
use crate::task::task_plan_analysis::{cycle_path_label, detect_cycle};
use crate::task::{Deps, Task};
use eyre::{Result, eyre};
use petgraph::Direction;
use petgraph::graph::DiGraph;

pub fn build_execution_plan(deps: &Deps) -> Result<ExecutionPlan> {
    build_execution_plan_from_graph(&deps.graph).map_err(|err| eyre!(err))
}

fn sorted_ready_tasks(graph: &DiGraph<Task, ()>) -> Vec<Task> {
    let mut ready = graph
        .externals(Direction::Outgoing)
        .map(|idx| graph[idx].clone())
        .collect::<Vec<_>>();
    ready.sort_by_key(TaskIdentity::from_task);
    ready
}

fn remove_task(graph: &mut DiGraph<Task, ()>, task: &Task) {
    if let Some(idx) = graph.node_indices().find(|&i| &graph[i] == task) {
        graph.remove_node(idx);
    }
}

fn remove_tasks(graph: &mut DiGraph<Task, ()>, tasks: &[Task]) {
    for task in tasks {
        remove_task(graph, task);
    }
}

fn to_planned(mut tasks: Vec<Task>) -> Vec<PlannedTask> {
    tasks.sort_by_key(TaskIdentity::from_task);
    tasks
        .into_iter()
        .map(|t| PlannedTask::from_task(&t))
        .collect()
}

pub(crate) fn build_execution_plan_from_graph(
    graph: &DiGraph<Task, ()>,
) -> std::result::Result<ExecutionPlan, String> {
    let mut graph = graph.clone();
    let mut plan = ExecutionPlan::default();

    while graph.node_count() > 0 {
        let ready = sorted_ready_tasks(&graph);
        if ready.is_empty() {
            if let Some(cycle) = detect_cycle(&graph) {
                return Err(format!(
                    "unable to build execution plan: circular dependency detected: {}",
                    cycle_path_label(&cycle)
                ));
            }
            return Err("unable to build execution plan: graph has no leaves".to_string());
        }

        let buckets = classify_ready_tasks(ready);
        let runtime_non_interactive = buckets.runtime_non_interactive;
        let interactive_runtime = buckets.interactive_runtime;
        let orchestrators = buckets.orchestrators;

        // Respect interactive barrier by running non-interactive runtime work first.
        // Orchestrators can join that parallel stage because they are non-runtime.
        if !runtime_non_interactive.is_empty() {
            let mut stage_tasks = runtime_non_interactive;
            stage_tasks.extend(orchestrators.clone());
            let planned = to_planned(stage_tasks.clone());
            plan.stages.push(ExecutionStage::parallel(planned));
            remove_tasks(&mut graph, &stage_tasks);
            continue;
        }

        // No runtime work in flight: schedule one interactive task (deterministic tie-break).
        if let Some(next_interactive) = interactive_runtime.into_iter().next() {
            let planned = PlannedTask::from_task(&next_interactive);
            plan.stages.push(ExecutionStage::interactive(planned));
            remove_task(&mut graph, &next_interactive);
            continue;
        }

        // Only pure orchestrators are ready.
        if !orchestrators.is_empty() {
            let planned = to_planned(orchestrators.clone());
            plan.stages.push(ExecutionStage::parallel(planned));
            remove_tasks(&mut graph, &orchestrators);
            continue;
        }
    }

    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::config_file::mise_toml::EnvList;
    use crate::config::env_directive::{EnvDirective, EnvDirectiveOptions};
    use crate::task::RunEntry;
    use crate::task::task_execution_plan::ExecutionStageKind;

    fn runtime(name: &str) -> Task {
        Task {
            name: name.to_string(),
            run: vec![RunEntry::Script(format!("echo {name}"))],
            ..Default::default()
        }
    }

    fn interactive(name: &str) -> Task {
        Task {
            name: name.to_string(),
            interactive: Some(true),
            run: vec![RunEntry::Script("read x".to_string())],
            ..Default::default()
        }
    }

    #[test]
    fn test_planner_runtime_parallel_before_interactive() {
        // MatrixRef: B01,B07,B08 / C1,C11
        let mut graph = DiGraph::new();
        graph.add_node(runtime("build"));
        graph.add_node(interactive("prompt"));

        let plan = build_execution_plan_from_graph(&graph).unwrap();
        assert_eq!(plan.stages.len(), 2);
        assert_eq!(plan.stages[0].kind, ExecutionStageKind::Parallel);
        assert_eq!(
            plan.stages[1].kind,
            ExecutionStageKind::InteractiveExclusive
        );
        assert_eq!(plan.stages[1].tasks[0].name(), "prompt");
    }

    #[test]
    fn test_planner_interactive_tie_break_is_lexicographic_identity() {
        // MatrixRef: B05,B06 / C10
        let mut graph = DiGraph::new();
        graph.add_node(Task {
            name: "ask".to_string(),
            args: vec!["z".to_string()],
            interactive: Some(true),
            run: vec![RunEntry::Script("read x".to_string())],
            ..Default::default()
        });
        graph.add_node(Task {
            name: "ask".to_string(),
            args: vec!["a".to_string()],
            interactive: Some(true),
            run: vec![RunEntry::Script("read x".to_string())],
            env: EnvList(vec![EnvDirective::Val(
                "X".to_string(),
                "1".to_string(),
                EnvDirectiveOptions::default(),
            )]),
            ..Default::default()
        });

        let plan = build_execution_plan_from_graph(&graph).unwrap();
        let names: Vec<(String, Vec<String>)> = plan
            .stages
            .iter()
            .flat_map(|s| s.tasks.iter())
            .map(|t| (t.identity.name.clone(), t.identity.args.clone()))
            .collect();
        assert_eq!(
            names,
            vec![
                ("ask".to_string(), vec!["a".to_string()]),
                ("ask".to_string(), vec!["z".to_string()]),
            ]
        );
    }

    #[test]
    fn test_planner_depends_then_interactive() {
        // MatrixRef: B02 / C3
        let mut graph = DiGraph::new();
        let ask_idx = graph.add_node(interactive("ask"));
        let prep_idx = graph.add_node(runtime("prep"));
        graph.update_edge(ask_idx, prep_idx, ());

        let plan = build_execution_plan_from_graph(&graph).unwrap();
        assert_eq!(plan.stages.len(), 2);
        assert_eq!(plan.stages[0].kind, ExecutionStageKind::Parallel);
        assert_eq!(plan.stages[0].tasks[0].name(), "prep");
        assert_eq!(
            plan.stages[1].kind,
            ExecutionStageKind::InteractiveExclusive
        );
        assert_eq!(plan.stages[1].tasks[0].name(), "ask");
    }

    #[test]
    fn test_planner_interactive_then_depends_post() {
        // MatrixRef: B03 / C4
        let mut graph = DiGraph::new();
        let ask_idx = graph.add_node(interactive("ask"));
        let post_idx = graph.add_node(runtime("cleanup"));
        // depends_post edge orientation in Deps: post -> parent
        graph.update_edge(post_idx, ask_idx, ());

        let plan = build_execution_plan_from_graph(&graph).unwrap();
        assert_eq!(plan.stages.len(), 2);
        assert_eq!(
            plan.stages[0].kind,
            ExecutionStageKind::InteractiveExclusive
        );
        assert_eq!(plan.stages[0].tasks[0].name(), "ask");
        assert_eq!(plan.stages[1].kind, ExecutionStageKind::Parallel);
        assert_eq!(plan.stages[1].tasks[0].name(), "cleanup");
    }

    #[test]
    fn test_planner_orchestrator_stage() {
        // MatrixRef: V11 / C7
        let mut graph = DiGraph::new();
        graph.add_node(Task {
            name: "group".to_string(),
            run: vec![RunEntry::SingleTask {
                task: "build".to_string(),
            }],
            ..Default::default()
        });

        let plan = build_execution_plan_from_graph(&graph).unwrap();
        assert_eq!(plan.stages.len(), 1);
        assert_eq!(plan.stages[0].kind, ExecutionStageKind::Parallel);
        assert!(!plan.stages[0].tasks[0].runtime);
    }

    #[test]
    fn test_planner_fails_on_cycle_without_leaves() {
        // MatrixRef: O7 / C13
        let mut graph = DiGraph::new();
        let a = graph.add_node(runtime("a"));
        let b = graph.add_node(runtime("b"));
        graph.update_edge(a, b, ());
        graph.update_edge(b, a, ());

        let err = build_execution_plan_from_graph(&graph).unwrap_err();
        assert!(err.contains("circular dependency detected"));
        assert!(err.contains("a -> b -> a"));
    }

    #[test]
    fn test_planner_prefers_interactive_before_orchestrator_without_runtime_contention() {
        // MatrixRef: B05,B08 / C1,C10,C11
        let mut graph = DiGraph::new();
        graph.add_node(interactive("ask"));
        graph.add_node(Task {
            name: "group".to_string(),
            run: vec![RunEntry::SingleTask {
                task: "build".to_string(),
            }],
            ..Default::default()
        });

        let plan = build_execution_plan_from_graph(&graph).unwrap();
        assert_eq!(plan.stages.len(), 2);
        assert_eq!(
            plan.stages[0].kind,
            ExecutionStageKind::InteractiveExclusive
        );
        assert_eq!(plan.stages[0].tasks[0].name(), "ask");
        assert_eq!(plan.stages[1].kind, ExecutionStageKind::Parallel);
        assert_eq!(plan.stages[1].tasks[0].name(), "group");
    }

    #[test]
    fn test_planner_parallel_stage_is_identity_sorted() {
        // MatrixRef: B06 / C10
        let mut graph = DiGraph::new();
        graph.add_node(runtime("b"));
        graph.add_node(runtime("a"));
        graph.add_node(Task {
            name: "group".to_string(),
            run: vec![RunEntry::SingleTask {
                task: "a".to_string(),
            }],
            ..Default::default()
        });

        let plan = build_execution_plan_from_graph(&graph).unwrap();
        assert_eq!(plan.stages.len(), 1);
        assert_eq!(plan.stages[0].kind, ExecutionStageKind::Parallel);
        let names: Vec<String> = plan.stages[0]
            .tasks
            .iter()
            .map(|t| t.name().to_string())
            .collect();
        assert_eq!(
            names,
            vec!["a".to_string(), "b".to_string(), "group".to_string()]
        );
    }
}
