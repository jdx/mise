use crate::task::task_helpers::{
    STATIC_BARRIER_END_SEGMENT, STATIC_BARRIER_START_SEGMENT, STATIC_INTERNAL_TASK_PREFIX,
    classify_ready_tasks, task_logical_name,
};
use crate::task::task_identity::TaskIdentity;
use crate::task::task_list::split_task_spec;
use crate::task::task_resolution_diagnostic::{
    AvailableTaskDiagnostic, DEFAULT_AVAILABLE_TASKS_PREVIEW_LIMIT, ResolutionScope,
    append_resolution_sections, available_tasks_from_tasks,
};
use crate::task::{GetMatchingExt, RunEntry, Task, TaskLoadContext, resolve_task_pattern};
use crate::{config::Config, task::task_list::resolve_depends};
use eyre::{Result, eyre};
use itertools::Itertools;
use petgraph::Direction;
use petgraph::graph::DiGraph;
use petgraph::visit::EdgeRef;
use serde::Serialize;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fmt::{Display, Formatter},
    future::Future,
    pin::Pin,
    sync::Arc,
};
use tokio::sync::mpsc;

/// Unique key for a task instance, including name, args, and env vars
pub type TaskKey = TaskIdentity;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MissingRunTaskDiagnostic {
    pub missing_task: String,
    pub dependency_path: Vec<String>,
    pub config_files: Vec<String>,
    pub available_tasks: Vec<AvailableTaskDiagnostic>,
}

impl MissingRunTaskDiagnostic {
    fn for_resolution(
        missing_name: &str,
        parent: &Task,
        dependency_chain: &[String],
        config_files: &[String],
        available_tasks: &BTreeMap<String, Task>,
    ) -> Self {
        let mut dependency_path = dependency_chain
            .iter()
            .filter(|name| !name.is_empty())
            .cloned()
            .collect_vec();
        if dependency_path.is_empty() {
            dependency_path.push(task_user_facing_name(parent));
        }
        dependency_path.push(missing_name.to_string());

        let available_tasks = available_tasks_from_tasks(available_tasks.values());

        Self {
            missing_task: missing_name.to_string(),
            dependency_path,
            config_files: config_files.to_vec(),
            available_tasks,
        }
    }

    fn render_user_message(&self) -> String {
        let mut lines = vec![format!("task not found: {}", self.missing_task)];
        lines.push(String::new());
        lines.push("Dependency path:".to_string());
        lines.push(format!("  {}", self.dependency_path.join(" -> ")));

        append_resolution_sections(
            &mut lines,
            &self.config_files,
            &self.available_tasks,
            DEFAULT_AVAILABLE_TASKS_PREVIEW_LIMIT,
        );

        lines.join("\n")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum DepsValidationError {
    InvalidTask { task: String, message: String },
}

impl Display for DepsValidationError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidTask { task, message } => write!(f, "invalid task `{task}`: {message}"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Deps {
    pub graph: DiGraph<Task, ()>,
    sent: HashSet<TaskKey>, // tasks that have already started so should not run again
    removed: HashSet<TaskKey>, // tasks that have already finished to track if we are in an infinitve loop
    executed: HashSet<TaskKey>, // tasks that actually began executing (not just scheduled)
    post_dep_parents: HashMap<TaskKey, HashSet<TaskKey>>, // maps each post-dep to its parent tasks
    validation_errors: Vec<DepsValidationError>,
    tx: mpsc::UnboundedSender<Option<Task>>,
    // not clone, notify waiters via tx None
}

/// Extract a hashable key from a task, including env vars set via dependencies
pub fn task_key(task: &Task) -> TaskKey {
    TaskIdentity::from_task(task)
}

#[derive(Debug, Clone, Copy)]
struct NodeSpan {
    start: petgraph::graph::NodeIndex,
    end: petgraph::graph::NodeIndex,
}

fn has_run_task_refs(task: &Task) -> bool {
    task.run()
        .iter()
        .any(|e| matches!(e, RunEntry::SingleTask { .. } | RunEntry::TaskGroup { .. }))
}

fn next_internal_name(seed: &str, counter: &mut usize) -> String {
    *counter += 1;
    format!("{STATIC_INTERNAL_TASK_PREFIX}{seed}::{}", *counter)
}

fn make_internal_noop_task(source: &Task, name: String) -> Task {
    let mut task = source.clone();
    task.name = name;
    task.aliases = vec![task_logical_name(source).to_string()];
    task.file = None;
    task.run.clear();
    task.run_windows.clear();
    task.depends.clear();
    task.depends_post.clear();
    task.wait_for.clear();
    task.interactive = Some(false);
    task
}

fn make_internal_script_task(source: &Task, name: String, script: String) -> Task {
    let mut task = source.clone();
    task.name = name;
    task.aliases = vec![task_logical_name(source).to_string()];
    task.file = None;
    task.run = vec![RunEntry::Script(script.clone())];
    task.run_windows = vec![RunEntry::Script(script)];
    task.depends.clear();
    task.depends_post.clear();
    task.wait_for.clear();
    task
}

async fn resolve_run_specs(
    config: &Arc<Config>,
    parent: &Task,
    specs: &[String],
    skip_deps: bool,
    dependency_chain: &[String],
) -> Result<Vec<Task>> {
    let ctx = TaskLoadContext::from_patterns(specs.iter().map(|s| split_task_spec(s).0));
    let tasks = config.tasks_with_context(Some(&ctx)).await?;
    let tasks_with_aliases = crate::task::build_task_ref_map(tasks.iter());
    let config_files = collect_resolution_config_files(config, &tasks);

    let mut resolved = Vec::new();
    for spec in specs {
        let (name, args) = split_task_spec(spec);
        let matches = tasks_with_aliases.get_matching(name)?;
        if matches.is_empty() {
            let diagnostic = MissingRunTaskDiagnostic::for_resolution(
                name,
                parent,
                dependency_chain,
                &config_files,
                &tasks,
            );
            return Err(eyre!(diagnostic.render_user_message()));
        }
        for matched in matches {
            let mut t = (*matched).clone();
            t.args = args.clone();
            if skip_deps {
                t.depends.clear();
                t.depends_post.clear();
                t.wait_for.clear();
            }
            // Preserve inherited env propagation semantics for static run subgraphs.
            if !parent.env.is_empty() {
                t.inherited_env.0.extend(parent.env.0.clone());
            }
            if !parent.inherited_env.is_empty() {
                t.inherited_env.0.extend(parent.inherited_env.0.clone());
            }
            resolved.push(t);
        }
    }
    Ok(resolved)
}

fn task_user_facing_name(task: &Task) -> String {
    let name = task_logical_name(task);
    if name.is_empty() {
        task.name.clone()
    } else {
        name.to_string()
    }
}

fn dependency_chain_for_node(
    graph: &DiGraph<Task, ()>,
    node_idx: petgraph::graph::NodeIndex,
) -> Vec<String> {
    let mut chain = vec![node_idx];
    let mut current = node_idx;
    let mut seen = HashSet::from([node_idx]);

    loop {
        let next_parent = graph
            .neighbors_directed(current, Direction::Incoming)
            .sorted_by_key(|idx| (task_user_facing_name(&graph[*idx]), idx.index()))
            .find(|idx| !seen.contains(idx));

        let Some(parent_idx) = next_parent else {
            break;
        };
        seen.insert(parent_idx);
        chain.push(parent_idx);
        current = parent_idx;
    }

    let mut names = chain
        .into_iter()
        .rev()
        .map(|idx| task_user_facing_name(&graph[idx]))
        .filter(|name| !name.is_empty())
        .collect_vec();
    names.dedup();
    names
}

fn collect_resolution_config_files(
    config: &Config,
    available_tasks: &BTreeMap<String, Task>,
) -> Vec<String> {
    ResolutionScope::from_config_and_tasks(config, available_tasks).config_files
}

fn merge_subgraph_into_graph(
    graph: &mut DiGraph<Task, ()>,
    post_dep_parents: &mut HashMap<TaskKey, HashSet<TaskKey>>,
    source_task: &Task,
    mut sub_deps: Deps,
    seed: &str,
    counter: &mut usize,
) -> NodeSpan {
    let starts = sub_deps
        .graph
        .externals(Direction::Outgoing)
        .collect::<Vec<_>>();
    let ends = sub_deps
        .graph
        .externals(Direction::Incoming)
        .collect::<Vec<_>>();

    let mut idx_map = HashMap::new();
    let mut key_map = HashMap::new();

    for idx in sub_deps.graph.node_indices() {
        let old_task = sub_deps.graph[idx].clone();
        let mut new_task = old_task.clone();
        let old_key = task_key(&old_task);
        let name_seed = format!("{seed}::{}", old_task.name);
        new_task.name = next_internal_name(&name_seed, counter);
        let mut aliases = vec![old_task.name.clone()];
        aliases.extend(old_task.aliases.clone());
        aliases.sort();
        aliases.dedup();
        new_task.aliases = aliases;
        let new_idx = graph.add_node(new_task.clone());
        idx_map.insert(idx, new_idx);
        key_map.insert(old_key, task_key(&new_task));
    }

    for edge in sub_deps.graph.edge_references() {
        let from = idx_map[&edge.source()];
        let to = idx_map[&edge.target()];
        graph.update_edge(from, to, ());
    }

    for (post_key, parent_keys) in sub_deps.post_dep_parents.drain() {
        if let Some(mapped_post) = key_map.get(&post_key) {
            let entry = post_dep_parents.entry(mapped_post.clone()).or_default();
            for pk in parent_keys {
                if let Some(mapped_parent) = key_map.get(&pk) {
                    entry.insert(mapped_parent.clone());
                }
            }
        }
    }

    let start_join = graph.add_node(make_internal_noop_task(
        source_task,
        next_internal_name(&format!("{seed}::{STATIC_BARRIER_START_SEGMENT}"), counter),
    ));
    let end_join = graph.add_node(make_internal_noop_task(
        source_task,
        next_internal_name(&format!("{seed}::{STATIC_BARRIER_END_SEGMENT}"), counter),
    ));

    for s in starts {
        let s = idx_map[&s];
        graph.update_edge(s, start_join, ());
    }
    for e in ends {
        let e = idx_map[&e];
        graph.update_edge(end_join, e, ());
    }

    NodeSpan {
        start: start_join,
        end: end_join,
    }
}

async fn expand_static_run_subgraphs(
    config: &Arc<Config>,
    graph: &mut DiGraph<Task, ()>,
    post_dep_parents: &mut HashMap<TaskKey, HashSet<TaskKey>>,
    validation_errors: &mut Vec<DepsValidationError>,
    skip_deps: bool,
    dependency_chain_prefix: &[String],
) -> Result<()> {
    let mut counter: usize = 0;
    let mut queue = graph.node_indices().collect::<Vec<_>>();
    let mut seen = HashSet::new();

    while let Some(node_idx) = queue.pop() {
        if !seen.insert(node_idx) {
            continue;
        }
        let Some(task) = graph.node_weight(node_idx).cloned() else {
            continue;
        };
        if !has_run_task_refs(&task) {
            continue;
        }
        let mut dependency_chain = dependency_chain_prefix.to_vec();
        dependency_chain.extend(dependency_chain_for_node(graph, node_idx));
        dependency_chain = normalize_dependency_chain(dependency_chain);
        trace!(
            "static-run-expand: processing task {} with {} run entries",
            task.name,
            task.run().len()
        );

        // Preserve validation behavior for statically-resolved run subgraphs.
        if let Some(msg) = task.interactive_validation_error() {
            trace!(
                "static-run-expand: task {} remains unexpanded due to validation error: {}",
                task.name, msg
            );
            continue;
        }

        let original_run = task.run().clone();
        let prior_deps = graph
            .neighbors_directed(node_idx, Direction::Outgoing)
            .collect::<Vec<_>>();

        let outgoing_edges = graph
            .edges_directed(node_idx, Direction::Outgoing)
            .map(|e| e.id())
            .collect::<Vec<_>>();
        for edge_id in outgoing_edges {
            graph.remove_edge(edge_id);
        }

        if let Some(node) = graph.node_weight_mut(node_idx) {
            node.file = None;
            node.run.clear();
            node.run_windows.clear();
            node.depends.clear();
            node.depends_post.clear();
            node.wait_for.clear();
            node.interactive = Some(false);
        }

        let mut prev = prior_deps;
        for entry in original_run {
            match entry {
                RunEntry::Script(script) => {
                    let script_name =
                        next_internal_name(&format!("{}::script", task.name), &mut counter);
                    let script_task = make_internal_script_task(&task, script_name, script);
                    let script_idx = graph.add_node(script_task);
                    for dep in &prev {
                        graph.update_edge(script_idx, *dep, ());
                    }
                    queue.push(script_idx);
                    prev = vec![script_idx];
                }
                RunEntry::SingleTask { task: spec } => {
                    trace!(
                        "static-run-expand: single task entry {} -> {}",
                        task.name, spec
                    );
                    let resolved = resolve_task_pattern(&spec, Some(&task));
                    let resolved_tasks =
                        resolve_run_specs(config, &task, &[resolved], skip_deps, &dependency_chain)
                            .await?;
                    trace!(
                        "static-run-expand: resolved {} sub task(s) for {}",
                        resolved_tasks.len(),
                        task.name
                    );
                    let mut has_invalid = false;
                    for t in &resolved_tasks {
                        if let Some(msg) = t.interactive_validation_error() {
                            has_invalid = true;
                            let validation_error = DepsValidationError::InvalidTask {
                                task: task_user_facing_name(t),
                                message: msg,
                            };
                            validation_errors.push(validation_error.clone());
                            trace!(
                                "static-run-expand: resolved invalid sub-task {} for {}: {}",
                                t.name, task.name, validation_error
                            );
                        }
                    }
                    if has_invalid {
                        continue;
                    }
                    let sub_deps = Deps::new_boxed(
                        config,
                        resolved_tasks,
                        skip_deps,
                        dependency_chain.clone(),
                    )
                    .await?;
                    let span = merge_subgraph_into_graph(
                        graph,
                        post_dep_parents,
                        &task,
                        sub_deps,
                        &task.name,
                        &mut counter,
                    );
                    trace!(
                        "static-run-expand: merged single-task subgraph for {} start={:?} end={:?}",
                        task.name, span.start, span.end
                    );
                    for dep in &prev {
                        graph.update_edge(span.start, *dep, ());
                    }
                    queue.push(span.start);
                    queue.push(span.end);
                    prev = vec![span.end];
                }
                RunEntry::TaskGroup { tasks } => {
                    trace!(
                        "static-run-expand: task-group entry {} with {} member(s)",
                        task.name,
                        tasks.len()
                    );
                    let resolved = tasks
                        .iter()
                        .map(|t| resolve_task_pattern(t, Some(&task)))
                        .collect::<Vec<_>>();
                    let resolved_tasks =
                        resolve_run_specs(config, &task, &resolved, skip_deps, &dependency_chain)
                            .await?;
                    trace!(
                        "static-run-expand: resolved {} grouped sub task(s) for {}",
                        resolved_tasks.len(),
                        task.name
                    );
                    let mut has_invalid = false;
                    for t in &resolved_tasks {
                        if let Some(msg) = t.interactive_validation_error() {
                            has_invalid = true;
                            let validation_error = DepsValidationError::InvalidTask {
                                task: task_user_facing_name(t),
                                message: msg,
                            };
                            validation_errors.push(validation_error.clone());
                            trace!(
                                "static-run-expand: resolved invalid grouped sub-task {} for {}: {}",
                                t.name, task.name, validation_error
                            );
                        }
                    }
                    if has_invalid {
                        continue;
                    }
                    let sub_deps = Deps::new_boxed(
                        config,
                        resolved_tasks,
                        skip_deps,
                        dependency_chain.clone(),
                    )
                    .await?;
                    let span = merge_subgraph_into_graph(
                        graph,
                        post_dep_parents,
                        &task,
                        sub_deps,
                        &task.name,
                        &mut counter,
                    );
                    trace!(
                        "static-run-expand: merged task-group subgraph for {} start={:?} end={:?}",
                        task.name, span.start, span.end
                    );
                    for dep in &prev {
                        graph.update_edge(span.start, *dep, ());
                    }
                    queue.push(span.start);
                    queue.push(span.end);
                    prev = vec![span.end];
                }
            }
        }

        for dep in prev {
            graph.update_edge(node_idx, dep, ());
        }
    }

    Ok(())
}

fn normalize_dependency_chain(chain: Vec<String>) -> Vec<String> {
    let mut out = Vec::with_capacity(chain.len());
    for name in chain.into_iter().filter(|name| !name.is_empty()) {
        if out.last() != Some(&name) {
            out.push(name);
        }
    }
    out
}

/// manages a dependency graph of tasks so `mise run` knows what to run next
impl Deps {
    pub async fn new(config: &Arc<Config>, tasks: Vec<Task>) -> Result<Self> {
        Self::new_with_skip(config, tasks, false).await
    }

    pub async fn new_with_skip(
        config: &Arc<Config>,
        tasks: Vec<Task>,
        skip_deps: bool,
    ) -> Result<Self> {
        Self::new_boxed(config, tasks, skip_deps, Vec::new()).await
    }

    fn new_boxed<'a>(
        config: &'a Arc<Config>,
        tasks: Vec<Task>,
        skip_deps: bool,
        dependency_chain_prefix: Vec<String>,
    ) -> Pin<Box<dyn Future<Output = Result<Self>> + Send + 'a>> {
        Box::pin(async move {
            let mut graph = DiGraph::new();
            let mut indexes = HashMap::new();
            let mut stack = vec![];
            let mut seen = HashSet::new();
            let mut post_dep_parents: HashMap<TaskKey, HashSet<TaskKey>> = HashMap::new();

            let mut add_idx = |task: &Task, graph: &mut DiGraph<Task, ()>| {
                *indexes
                    .entry(task_key(task))
                    .or_insert_with(|| graph.add_node(task.clone()))
            };

            // first we add all tasks to the graph, create a stack of work for this function, and
            // store the index of each task in the graph
            for t in &tasks {
                stack.push(t.clone());
                add_idx(t, &mut graph);
            }
            let all_tasks_to_run = resolve_depends(config, tasks).await?;
            while let Some(a) = stack.pop() {
                if seen.contains(&a) {
                    // prevent infinite loop
                    continue;
                }
                let a_idx = add_idx(&a, &mut graph);
                let (pre, post) = if skip_deps {
                    (Vec::new(), Vec::new())
                } else {
                    a.resolve_depends(config, &all_tasks_to_run).await?
                };
                for b in pre {
                    let b_idx = add_idx(&b, &mut graph);
                    graph.update_edge(a_idx, b_idx, ());
                    stack.push(b.clone());
                }
                for b in post {
                    let b_idx = add_idx(&b, &mut graph);
                    graph.update_edge(b_idx, a_idx, ());
                    post_dep_parents
                        .entry(task_key(&b))
                        .or_default()
                        .insert(task_key(&a));
                    stack.push(b.clone());
                }
                seen.insert(a);
            }
            let (tx, _) = mpsc::unbounded_channel();
            let sent = HashSet::new();
            let removed = HashSet::new();
            let executed = HashSet::new();
            let mut validation_errors = Vec::new();

            expand_static_run_subgraphs(
                config,
                &mut graph,
                &mut post_dep_parents,
                &mut validation_errors,
                skip_deps,
                &dependency_chain_prefix,
            )
            .await?;

            Ok(Self {
                graph,
                tx,
                sent,
                removed,
                executed,
                post_dep_parents,
                validation_errors,
            })
        })
    }

    /// main method to emit tasks that no longer have dependencies being waited on
    fn emit_leaves(&mut self) {
        let leaves = leaves(&self.graph);
        let leaves_is_empty = leaves.is_empty();

        for task in leaves {
            let key = task_key(&task);

            if self.sent.insert(key) {
                trace!("Scheduling task {0}", task.name);
                if let Err(e) = self.tx.send(Some(task)) {
                    trace!("Error sending task: {e:?}");
                }
            }
        }

        if self.is_empty() {
            trace!("All tasks finished");
            if let Err(e) = self.tx.send(None) {
                trace!("Error closing task stream: {e:?}");
            }
        } else if leaves_is_empty && self.sent.len() == self.removed.len() {
            panic!(
                "Infinitive loop detected, all tasks are finished but the graph isn't empty {0} {1:#?}",
                self.all().map(|t| t.name.clone()).join(", "),
                self.graph
            )
        }
    }

    /// listened to by `mise run` which gets a stream of tasks to run
    pub fn subscribe(&mut self) -> mpsc::UnboundedReceiver<Option<Task>> {
        let (tx, rx) = mpsc::unbounded_channel();
        self.tx = tx;
        self.emit_leaves();
        rx
    }

    pub fn is_empty(&self) -> bool {
        self.graph.node_count() == 0
    }

    /// Check if a post-dep task should actually run: it must be a post-dependency
    /// AND its parent must have actually started executing (not just been scheduled).
    /// Returns false for non-post-dep tasks or post-deps whose parent was never executed.
    pub fn is_runnable_post_dep(&self, task: &Task) -> bool {
        let key = task_key(task);
        match self.post_dep_parents.get(&key) {
            Some(parent_keys) => parent_keys.iter().any(|pk| self.executed.contains(pk)),
            None => false,
        }
    }

    /// Mark a task as having actually started execution.
    /// This is distinct from being scheduled (sent) — a task may be scheduled as a
    /// graph leaf but then skipped because an earlier task failed.
    pub fn mark_executed(&mut self, task: &Task) {
        self.executed.insert(task_key(task));
    }

    /// Remove multiple tasks from the graph in a batch, emitting leaves only once at the end.
    /// This prevents intermediate emit_leaves from scheduling tasks that will be removed later.
    pub fn remove_batch(&mut self, tasks: &[Task]) {
        for task in tasks {
            if let Some(idx) = self.node_idx(task) {
                self.graph.remove_node(idx);
                let key = task_key(task);
                self.removed.insert(key);
            }
        }
        self.emit_leaves();
    }

    // use contracts::{ensures, requires};
    // #[requires(self.graph.node_count() > 0)]
    // #[ensures(self.graph.node_count() == old(self.graph.node_count()) - 1)]
    pub fn remove(&mut self, task: &Task) {
        if let Some(idx) = self.node_idx(task) {
            self.graph.remove_node(idx);
            let key = task_key(task);
            self.removed.insert(key);
            self.emit_leaves();
        }
    }

    fn node_idx(&self, task: &Task) -> Option<petgraph::graph::NodeIndex> {
        self.graph
            .node_indices()
            .find(|&idx| &self.graph[idx] == task)
    }

    pub fn all(&self) -> impl Iterator<Item = &Task> {
        self.graph.node_indices().map(|idx| &self.graph[idx])
    }

    pub fn validation_errors(&self) -> &[DepsValidationError] {
        &self.validation_errors
    }

    pub fn is_linear(&self) -> bool {
        let mut graph = self.graph.clone();
        // pop dependencies off, if we get multiple dependencies at once it's not linear
        loop {
            let leaves = leaves(&graph);
            if leaves.is_empty() {
                return true;
            } else if leaves.len() > 1 {
                return false;
            } else {
                let idx = self
                    .graph
                    .node_indices()
                    .find(|&idx| graph[idx] == leaves[0])
                    .unwrap();
                graph.remove_node(idx);
            }
        }
    }
}

fn leaves(graph: &DiGraph<Task, ()>) -> Vec<Task> {
    // Static-DAG compatible priority for ready leaves:
    // 1) runtime non-interactive first (maximize parallel runtime before barrier)
    // 2) if no runtime non-interactive exists, interactive runtime before pure orchestrators
    // 3) deterministic tie-break in each bucket via Task::Ord (TaskIdentity)
    // MatrixRef: B01,B05,B06,B08 / C1,C10,C11
    let ready = graph
        .externals(Direction::Outgoing)
        .map(|idx| graph[idx].clone())
        .collect::<Vec<_>>();
    let buckets = classify_ready_tasks(ready);
    let runtime_non_interactive = buckets.runtime_non_interactive;
    let interactive_runtime = buckets.interactive_runtime;
    let orchestrators = buckets.orchestrators;

    let mut ordered = Vec::new();
    if !runtime_non_interactive.is_empty() {
        ordered.extend(runtime_non_interactive);
        ordered.extend(orchestrators);
        ordered.extend(interactive_runtime);
    } else if !interactive_runtime.is_empty() {
        ordered.extend(interactive_runtime);
        ordered.extend(orchestrators);
    } else {
        ordered.extend(orchestrators);
    }
    ordered
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task::RunEntry;
    use tempfile::tempdir;

    #[test]
    fn test_leaves_are_sorted_deterministically() {
        // MatrixRef: B06 / C10
        let mut graph = DiGraph::new();
        let a_z = Task {
            name: "a".to_string(),
            args: vec!["z".to_string()],
            ..Default::default()
        };
        let a_a = Task {
            name: "a".to_string(),
            args: vec!["a".to_string()],
            ..Default::default()
        };
        let b = Task {
            name: "b".to_string(),
            ..Default::default()
        };
        graph.add_node(a_z);
        graph.add_node(a_a);
        graph.add_node(b);

        let names_and_args: Vec<(String, Vec<String>)> = leaves(&graph)
            .into_iter()
            .map(|t| (t.name, t.args))
            .collect();
        assert_eq!(
            names_and_args,
            vec![
                ("a".to_string(), vec!["a".to_string()]),
                ("a".to_string(), vec!["z".to_string()]),
                ("b".to_string(), vec![]),
            ]
        );
    }

    #[test]
    fn test_leaves_prioritize_runtime_before_interactive_when_both_ready() {
        // MatrixRef: B01,B07 / C1,C11
        let mut graph = DiGraph::new();
        graph.add_node(Task {
            name: "a_interactive".to_string(),
            interactive: Some(true),
            run: vec![RunEntry::Script("read x".to_string())],
            ..Default::default()
        });
        graph.add_node(Task {
            name: "z_runtime".to_string(),
            run: vec![RunEntry::Script("echo z".to_string())],
            ..Default::default()
        });

        let names: Vec<String> = leaves(&graph).into_iter().map(|t| t.name).collect();
        assert_eq!(
            names,
            vec!["z_runtime".to_string(), "a_interactive".to_string()]
        );
    }

    #[test]
    fn test_leaves_prioritize_interactive_before_orchestrator_without_runtime_contention() {
        // MatrixRef: B05,B08 / C1,C10,C11
        let mut graph = DiGraph::new();
        graph.add_node(Task {
            name: "ask".to_string(),
            interactive: Some(true),
            run: vec![RunEntry::Script("read x".to_string())],
            ..Default::default()
        });
        graph.add_node(Task {
            name: "group".to_string(),
            run: vec![RunEntry::SingleTask {
                task: "build".to_string(),
            }],
            ..Default::default()
        });

        let names: Vec<String> = leaves(&graph).into_iter().map(|t| t.name).collect();
        assert_eq!(names, vec!["ask".to_string(), "group".to_string()]);
    }

    #[test]
    fn test_dependency_chain_for_node_follows_graph_ancestry() {
        let mut graph = DiGraph::new();
        let root = graph.add_node(Task {
            name: "demo_path".to_string(),
            ..Default::default()
        });
        let mid = graph.add_node(Task {
            name: "build".to_string(),
            ..Default::default()
        });
        let leaf = graph.add_node(Task {
            name: "release".to_string(),
            ..Default::default()
        });
        graph.update_edge(root, mid, ());
        graph.update_edge(mid, leaf, ());

        let chain = dependency_chain_for_node(&graph, leaf);
        assert_eq!(
            chain,
            vec![
                "demo_path".to_string(),
                "build".to_string(),
                "release".to_string()
            ]
        );
    }

    #[test]
    fn test_missing_run_task_diagnostic_contains_sections_and_truncation() {
        let available_tasks: BTreeMap<String, Task> = (0..35)
            .map(|idx| {
                let name = format!("task-{idx:02}");
                (
                    name.clone(),
                    Task {
                        name: name.clone(),
                        ..Default::default()
                    },
                )
            })
            .collect();

        let parent = Task {
            name: "demo_path".to_string(),
            ..Default::default()
        };
        let config_files = vec!["/tmp/mise.toml".to_string()];
        let diagnostic = MissingRunTaskDiagnostic::for_resolution(
            "missing-task",
            &parent,
            &["demo_path".to_string(), "build".to_string()],
            &config_files,
            &available_tasks,
        );
        let err = diagnostic.render_user_message();

        assert!(err.contains("task not found: missing-task"));
        assert!(err.contains("Dependency path:"));
        assert!(err.contains("demo_path -> build -> missing-task"));
        assert!(err.contains("Config files loaded for task resolution (1):"));
        assert!(err.contains("Available tasks (35):"));
        assert!(err.contains("  - ... and 5 more"));
    }

    #[test]
    fn test_missing_run_task_diagnostic_shows_task_declaration_locations() {
        let dir = tempdir().unwrap();
        let cfg = dir.path().join("mise.toml");
        std::fs::write(
            &cfg,
            r#"
[tasks.demo]
run = "echo demo"
"#,
        )
        .unwrap();

        let available_tasks = BTreeMap::from([(
            "demo".to_string(),
            Task {
                name: "demo".to_string(),
                config_source: cfg,
                run: vec![RunEntry::Script("echo demo".to_string())],
                ..Default::default()
            },
        )]);
        let parent = Task {
            name: "demo_path".to_string(),
            ..Default::default()
        };

        let diagnostic = MissingRunTaskDiagnostic::for_resolution(
            "missing-task",
            &parent,
            &["demo_path".to_string()],
            &[],
            &available_tasks,
        );
        let err = diagnostic.render_user_message();

        assert!(err.contains("  - demo ("));
        assert!(err.contains("mise.toml:2"));
    }
}
