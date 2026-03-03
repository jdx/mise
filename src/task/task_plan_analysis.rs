use crate::task::Task;
use crate::task::task_execution_plan::{ExecutionPlan, ExecutionStageKind};
use crate::task::task_identity::TaskIdentity;
use globset::GlobBuilder;
use petgraph::Direction;
use petgraph::graph::{DiGraph, NodeIndex};
use serde::Serialize;
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GraphCycle {
    pub path: Vec<TaskIdentity>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct ChangeImpact {
    pub changed_files: Vec<String>,
    pub directly_matched: Vec<ChangedTaskMatch>,
    pub impacted: Vec<TaskIdentity>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChangedTaskMatch {
    pub task: TaskIdentity,
    pub matched_files: Vec<String>,
    pub matched_source_patterns: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct ContentionAnalysis {
    pub jobs: usize,
    pub interactive_stage_count: usize,
    pub max_parallel_runtime_tasks: usize,
    pub stages_exceeding_jobs: Vec<usize>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DfsState {
    Visiting,
    Done,
}

pub fn detect_cycle(graph: &DiGraph<Task, ()>) -> Option<GraphCycle> {
    let mut states: HashMap<NodeIndex, DfsState> = HashMap::new();
    let mut stack: Vec<NodeIndex> = Vec::new();
    let mut stack_pos: HashMap<NodeIndex, usize> = HashMap::new();

    let mut nodes = graph.node_indices().collect::<Vec<_>>();
    nodes.sort_by_key(|idx| TaskIdentity::from_task(&graph[*idx]));

    for node in nodes {
        if states.contains_key(&node) {
            continue;
        }
        if let Some(cycle) = detect_cycle_dfs(graph, node, &mut states, &mut stack, &mut stack_pos)
        {
            let path = cycle
                .into_iter()
                .map(|idx| TaskIdentity::from_task(&graph[idx]))
                .collect();
            return Some(GraphCycle { path });
        }
    }

    None
}

fn detect_cycle_dfs(
    graph: &DiGraph<Task, ()>,
    node: NodeIndex,
    states: &mut HashMap<NodeIndex, DfsState>,
    stack: &mut Vec<NodeIndex>,
    stack_pos: &mut HashMap<NodeIndex, usize>,
) -> Option<Vec<NodeIndex>> {
    states.insert(node, DfsState::Visiting);
    stack_pos.insert(node, stack.len());
    stack.push(node);

    let mut neighbors = graph
        .neighbors_directed(node, Direction::Outgoing)
        .collect::<Vec<_>>();
    neighbors.sort_by_key(|idx| TaskIdentity::from_task(&graph[*idx]));

    for neighbor in neighbors {
        match states.get(&neighbor) {
            None => {
                if let Some(cycle) = detect_cycle_dfs(graph, neighbor, states, stack, stack_pos) {
                    return Some(cycle);
                }
            }
            Some(DfsState::Visiting) => {
                let start = *stack_pos
                    .get(&neighbor)
                    .expect("neighbor in visiting state must be in stack");
                let mut cycle = stack[start..].to_vec();
                cycle.push(neighbor);
                return Some(cycle);
            }
            Some(DfsState::Done) => {}
        }
    }

    stack.pop();
    stack_pos.remove(&node);
    states.insert(node, DfsState::Done);
    None
}

pub fn cycle_path_label(cycle: &GraphCycle) -> String {
    cycle
        .path
        .iter()
        .map(identity_label)
        .collect::<Vec<_>>()
        .join(" -> ")
}

pub fn identity_label(identity: &TaskIdentity) -> String {
    let mut label = identity.name.clone();
    if !identity.args.is_empty() {
        label = format!("{label} {}", identity.args.join(" "));
    }
    if !identity.env.is_empty() {
        let env = identity
            .env
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join(",");
        label = format!("{label} {{{env}}}");
    }
    label
}

pub fn analyze_changed_impact(graph: &DiGraph<Task, ()>, changed_files: &[String]) -> ChangeImpact {
    if changed_files.is_empty() {
        return ChangeImpact::default();
    }

    let normalized_changed = changed_files
        .iter()
        .map(|s| normalize_changed_file(s))
        .collect::<Vec<_>>();

    let mut directly_matched = Vec::new();
    let mut directly_impacted_nodes = Vec::new();

    let mut nodes = graph.node_indices().collect::<Vec<_>>();
    nodes.sort_by_key(|idx| TaskIdentity::from_task(&graph[*idx]));

    for idx in nodes {
        let task = &graph[idx];
        if task.sources.is_empty() {
            continue;
        }

        let mut matched_files = BTreeSet::new();
        let mut matched_patterns = BTreeSet::new();
        for source_pattern in &task.sources {
            let candidate_patterns = source_patterns_for_task(task, source_pattern);
            let mut source_pattern_matched = false;

            for candidate_pattern in candidate_patterns {
                let matcher = match GlobBuilder::new(&candidate_pattern).build() {
                    Ok(glob) => glob.compile_matcher(),
                    Err(_) => continue,
                };
                for changed in &normalized_changed {
                    if matcher.is_match(changed) {
                        matched_files.insert(changed.clone());
                        source_pattern_matched = true;
                    }
                }
            }

            if source_pattern_matched {
                matched_patterns.insert(source_pattern.clone());
            }
        }

        if !matched_files.is_empty() {
            directly_impacted_nodes.push(idx);
            directly_matched.push(ChangedTaskMatch {
                task: TaskIdentity::from_task(task),
                matched_files: matched_files.into_iter().collect(),
                matched_source_patterns: matched_patterns.into_iter().collect(),
            });
        }
    }

    directly_matched.sort_by(|a, b| a.task.cmp(&b.task));

    let mut impacted_nodes = HashSet::new();
    let mut queue = VecDeque::new();

    for idx in directly_impacted_nodes {
        if impacted_nodes.insert(idx) {
            queue.push_back(idx);
        }
    }

    // Edges are task -> dependency, so dependents are incoming neighbors.
    while let Some(current) = queue.pop_front() {
        for dependent in graph.neighbors_directed(current, Direction::Incoming) {
            if impacted_nodes.insert(dependent) {
                queue.push_back(dependent);
            }
        }
    }

    let mut impacted = impacted_nodes
        .into_iter()
        .map(|idx| TaskIdentity::from_task(&graph[idx]))
        .collect::<Vec<_>>();
    impacted.sort();

    ChangeImpact {
        changed_files: normalized_changed,
        directly_matched,
        impacted,
    }
}

pub fn analyze_contention(plan: &ExecutionPlan, jobs: usize) -> ContentionAnalysis {
    let mut interactive_stage_count = 0;
    let mut max_parallel_runtime_tasks = 0;
    let mut stages_exceeding_jobs = Vec::new();

    for (idx, stage) in plan.stages.iter().enumerate() {
        match stage.kind {
            ExecutionStageKind::InteractiveExclusive => {
                interactive_stage_count += 1;
            }
            ExecutionStageKind::Parallel => {
                let runtime_count = stage.tasks.iter().filter(|t| t.runtime).count();
                max_parallel_runtime_tasks = max_parallel_runtime_tasks.max(runtime_count);
                if runtime_count > jobs {
                    stages_exceeding_jobs.push(idx + 1);
                }
            }
        }
    }

    let mut warnings = Vec::new();
    if interactive_stage_count > 1 {
        warnings.push(format!(
            "{} interactive stages will execute strictly sequentially due to the global interactive barrier",
            interactive_stage_count
        ));
    }
    if !stages_exceeding_jobs.is_empty() {
        warnings.push(format!(
            "jobs={} limits runtime parallelism for stage(s): {}",
            jobs,
            stages_exceeding_jobs
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if interactive_stage_count > 0 && max_parallel_runtime_tasks > jobs {
        warnings.push(
            "interactive barriers plus current jobs setting can create noticeable scheduling contention"
                .to_string(),
        );
    }

    ContentionAnalysis {
        jobs,
        interactive_stage_count,
        max_parallel_runtime_tasks,
        stages_exceeding_jobs,
        warnings,
    }
}

fn normalize_changed_file(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    if let Some(stripped) = normalized.strip_prefix("./") {
        stripped.to_string()
    } else {
        normalized
    }
}

fn source_patterns_for_task(task: &Task, source_pattern: &str) -> Vec<String> {
    let normalized_source = normalize_changed_file(source_pattern);
    let mut patterns = BTreeSet::new();
    patterns.insert(normalized_source.clone());

    if let Some(task_dir) = task
        .dir
        .as_deref()
        .map(normalize_changed_file)
        .map(|d| d.trim_end_matches('/').to_string())
        && !task_dir.is_empty()
        && !Path::new(&normalized_source).is_absolute()
    {
        let scoped = format!(
            "{}/{}",
            task_dir,
            normalized_source.trim_start_matches("./")
        );
        patterns.insert(scoped);
    }

    patterns.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task::RunEntry;

    fn runtime(name: &str) -> Task {
        Task {
            name: name.to_string(),
            run: vec![RunEntry::Script(format!("echo {name}"))],
            ..Default::default()
        }
    }

    #[test]
    fn test_detect_cycle_returns_deterministic_path() {
        // MatrixRef: cycle-detect / C11
        let mut graph = DiGraph::new();
        let a = graph.add_node(runtime("a"));
        let b = graph.add_node(runtime("b"));
        let c = graph.add_node(runtime("c"));
        graph.update_edge(a, b, ());
        graph.update_edge(b, c, ());
        graph.update_edge(c, a, ());

        let cycle = detect_cycle(&graph).expect("must detect cycle");
        assert_eq!(cycle_path_label(&cycle), "a -> b -> c -> a");
    }

    #[test]
    fn test_analyze_changed_impact_includes_transitive_dependents() {
        // MatrixRef: impact-analysis / C11
        let mut graph = DiGraph::new();
        let mut build = runtime("build");
        build.sources = vec!["src/**/*.ts".to_string()];
        let test = runtime("test");
        let package = runtime("package");

        let build_idx = graph.add_node(build);
        let test_idx = graph.add_node(test);
        let package_idx = graph.add_node(package);

        // test depends on build, package depends on test.
        graph.update_edge(test_idx, build_idx, ());
        graph.update_edge(package_idx, test_idx, ());

        let impact = analyze_changed_impact(&graph, &["src/main.ts".to_string()]);
        let impacted_names = impact
            .impacted
            .iter()
            .map(|i| i.name.clone())
            .collect::<Vec<_>>();
        assert_eq!(impacted_names, vec!["build", "package", "test"]);
        assert_eq!(impact.directly_matched.len(), 1);
        assert_eq!(impact.directly_matched[0].task.name, "build");
    }

    #[test]
    fn test_analyze_changed_impact_matches_sources_scoped_by_task_dir() {
        // MatrixRef: STATIC-003 / C11
        let mut graph = DiGraph::new();
        let mut build = runtime("pkg_build");
        build.dir = Some("pkg".to_string());
        build.sources = vec!["src/**/*.ts".to_string()];
        let root = runtime("root");

        let build_idx = graph.add_node(build);
        let root_idx = graph.add_node(root);
        graph.update_edge(root_idx, build_idx, ());

        let impact = analyze_changed_impact(&graph, &["pkg/src/main.ts".to_string()]);
        assert_eq!(impact.directly_matched.len(), 1);
        assert_eq!(impact.directly_matched[0].task.name, "pkg_build");
        assert!(
            impact
                .impacted
                .iter()
                .any(|identity| identity.name == "root"),
            "dependent task should be marked impacted"
        );
    }

    #[test]
    fn test_analyze_contention_warns_for_interactive_and_jobs_limit() {
        // MatrixRef: contention-analysis / C1,C10,C11
        let plan = ExecutionPlan {
            stages: vec![
                crate::task::task_execution_plan::ExecutionStage::parallel(vec![
                    crate::task::task_execution_plan::PlannedTask {
                        identity: TaskIdentity {
                            name: "a".to_string(),
                            args: vec![],
                            env: vec![],
                        },
                        runtime: true,
                        interactive: false,
                        declaration: Default::default(),
                    },
                    crate::task::task_execution_plan::PlannedTask {
                        identity: TaskIdentity {
                            name: "b".to_string(),
                            args: vec![],
                            env: vec![],
                        },
                        runtime: true,
                        interactive: false,
                        declaration: Default::default(),
                    },
                ]),
                crate::task::task_execution_plan::ExecutionStage::interactive(
                    crate::task::task_execution_plan::PlannedTask {
                        identity: TaskIdentity {
                            name: "ask".to_string(),
                            args: vec![],
                            env: vec![],
                        },
                        runtime: true,
                        interactive: true,
                        declaration: Default::default(),
                    },
                ),
                crate::task::task_execution_plan::ExecutionStage::interactive(
                    crate::task::task_execution_plan::PlannedTask {
                        identity: TaskIdentity {
                            name: "ask2".to_string(),
                            args: vec![],
                            env: vec![],
                        },
                        runtime: true,
                        interactive: true,
                        declaration: Default::default(),
                    },
                ),
            ],
        };

        let contention = analyze_contention(&plan, 1);
        assert_eq!(contention.interactive_stage_count, 2);
        assert_eq!(contention.stages_exceeding_jobs, vec![1]);
        assert!(
            contention
                .warnings
                .iter()
                .any(|w| w.contains("strictly sequentially"))
        );
        assert!(
            contention
                .warnings
                .iter()
                .any(|w| w.contains("jobs=1 limits runtime parallelism"))
        );
    }
}
