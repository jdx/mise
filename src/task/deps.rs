use crate::config::Settings;
use crate::config::env_directive::EnvDirective;
use crate::task::task_fetcher::TaskFetcher;
use crate::task::{Task, dep_has_usage_ref, parse_usage_values_from_task};
use crate::{config::Config, task::task_list::resolve_depends};
use itertools::Itertools;
use petgraph::Direction;
use petgraph::algo::kosaraju_scc;
use petgraph::graph::{DiGraph, NodeIndex};
use std::{
    collections::{HashMap, HashSet},
    fmt,
    sync::Arc,
};
use tokio::sync::mpsc;

/// Unique key for a task instance, including name, args, and env vars
pub type TaskKey = (String, Vec<String>, Vec<(String, String)>);

pub struct TaskCycleError {
    paths: Vec<Vec<String>>,
    keys: Vec<Vec<TaskKey>>,
}

impl TaskCycleError {
    pub fn path(&self) -> &[String] {
        self.paths.first().map(Vec::as_slice).unwrap_or_default()
    }

    pub fn paths(&self) -> &[Vec<String>] {
        &self.paths
    }

    pub(crate) fn keys(&self) -> &[Vec<TaskKey>] {
        &self.keys
    }
}

impl fmt::Debug for TaskCycleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TaskCycleError")
            .field("paths", &self.paths)
            .finish_non_exhaustive()
    }
}

impl fmt::Display for TaskCycleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "circular dependency detected: {}",
            self.path().iter().join(" -> ")
        )
    }
}

impl std::error::Error for TaskCycleError {}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
/// State contributed by a task's completed direct dependencies.
pub struct TaskDependencyState {
    /// Stable artifact identities to include in the task's cache key.
    pub cache_keys: Vec<String>,
    /// Whether any dependency executed or restored outputs.
    pub any_did_work: bool,
    /// Whether any dependency did work without publishing a stable artifact identity.
    pub any_unkeyed_did_work: bool,
}

#[derive(Debug, Clone, Default)]
/// Completed task state that can be propagated into a nested task graph.
pub struct TaskCompletionState {
    completed: HashSet<TaskKey>,
    did_work: HashSet<TaskKey>,
    cache_keys: HashMap<TaskKey, String>,
}

impl TaskCompletionState {
    /// Merge state returned by a completed nested task graph.
    pub fn merge(&mut self, other: Self) {
        self.completed.extend(other.completed);
        self.did_work.extend(other.did_work);
        self.cache_keys.extend(other.cache_keys);
    }
}

#[derive(Debug)]
pub struct Deps {
    pub graph: DiGraph<Task, ()>,
    sent: HashSet<TaskKey>, // tasks that have already started so should not run again
    removed: HashSet<TaskKey>, // tasks that have already finished to track if we are in an infinite loop
    executed: HashSet<TaskKey>, // tasks that actually began executing (not just scheduled)
    did_work: HashSet<TaskKey>, // tasks that executed or restored outputs (not freshness-skipped)
    cache_keys: HashMap<TaskKey, String>, // stable artifact identities published by completed tasks
    dep_edges: HashMap<TaskKey, HashSet<TaskKey>>, // maps each task to its direct dependency task keys
    post_dep_parents: HashMap<TaskKey, HashSet<TaskKey>>, // maps each post-dep to its parent tasks
    tx: mpsc::UnboundedSender<Option<Task>>,
    // not clone, notify waiters via tx None
}

/// Extract a hashable key from a task, including env vars set via dependencies
pub fn task_key(task: &Task) -> TaskKey {
    // Extract simple key-value env vars for deduplication
    // This ensures tasks with same name/args but different env are treated as distinct
    let env_key: Vec<(String, String)> = task
        .env
        .0
        .iter()
        .filter_map(|d| match d {
            EnvDirective::Val(k, v, _) => Some((k.clone(), v.clone())),
            _ => None,
        })
        .sorted()
        .collect();
    (task.name.clone(), task.args.clone(), env_key)
}

/// manages a dependency graph of tasks so `mise run` knows what to run next
impl Deps {
    pub async fn new(config: &Arc<Config>, tasks: Vec<Task>) -> eyre::Result<Self> {
        Self::new_with_cycle_limit(config, tasks, Some(1)).await
    }

    pub(crate) async fn new_for_validation(
        config: &Arc<Config>,
        tasks: Vec<Task>,
    ) -> eyre::Result<Self> {
        Self::new_with_cycle_limit(config, tasks, None).await
    }

    async fn new_with_cycle_limit(
        config: &Arc<Config>,
        tasks: Vec<Task>,
        cycle_limit: Option<usize>,
    ) -> eyre::Result<Self> {
        let mut graph = DiGraph::new();
        let mut indexes = HashMap::new();
        let mut stack = vec![];
        let mut seen = HashSet::new();
        let mut post_dep_parents: HashMap<TaskKey, HashSet<TaskKey>> = HashMap::new();
        let mut dep_edges: HashMap<TaskKey, HashSet<TaskKey>> = HashMap::new();

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
        let no_cache = Settings::get().task.remote_no_cache.unwrap_or(false);
        let fetcher = TaskFetcher::new(no_cache);
        while let Some(mut a) = stack.pop() {
            if seen.contains(&a) {
                // prevent infinite loop
                continue;
            }
            // Fetch remote task files so file-based tasks have local paths
            // before we try to parse their usage specs or execute them.
            if a.file
                .as_ref()
                .is_some_and(|f| TaskFetcher::is_remote_source(&f.to_string_lossy()))
            {
                let mut tasks_to_fetch = vec![a];
                fetcher.fetch_tasks(config, &mut tasks_to_fetch).await?;
                a = tasks_to_fetch.into_iter().next().unwrap();
            }
            // Re-render dependency templates with usage values (including defaults)
            // so {{usage.*}} resolves.
            let has_usage_deps = |raw: &Option<Vec<_>>| {
                raw.as_ref()
                    .is_some_and(|r| r.iter().any(dep_has_usage_ref))
            };
            if has_usage_deps(&a.depends_raw)
                || has_usage_deps(&a.depends_post_raw)
                || has_usage_deps(&a.wait_for_raw)
            {
                let usage_values = parse_usage_values_from_task(config, &a).await?;
                if !usage_values.is_empty() {
                    a.render_depends_with_usage(config, &usage_values).await?;
                }
            }
            let a_idx = add_idx(&a, &mut graph);
            // Update the graph node with the fetched version of the task
            // (add_idx may have returned an existing index with an unfetched task)
            graph[a_idx] = a.clone();
            let (pre, post) = a.resolve_depends(config, &all_tasks_to_run).await?;
            for b in pre {
                let b_idx = add_idx(&b, &mut graph);
                graph.update_edge(a_idx, b_idx, ());
                dep_edges
                    .entry(task_key(&a))
                    .or_default()
                    .insert(task_key(&b));
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
        let cycles = find_cycles(&graph, cycle_limit);
        if !cycles.is_empty() {
            let paths = cycles
                .iter()
                .map(|cycle| {
                    cycle
                        .iter()
                        .map(|&idx| task_cycle_label(&graph[idx]))
                        .collect()
                })
                .collect();
            let keys = cycles
                .iter()
                .map(|cycle| cycle.iter().map(|&idx| task_key(&graph[idx])).collect())
                .collect();
            return Err(eyre::Report::new(TaskCycleError { paths, keys }));
        }
        let (tx, _) = mpsc::unbounded_channel();
        let sent = HashSet::new();
        let removed = HashSet::new();
        let executed = HashSet::new();
        let did_work = HashSet::new();
        let cache_keys = HashMap::new();
        Ok(Self {
            graph,
            tx,
            sent,
            removed,
            executed,
            did_work,
            cache_keys,
            dep_edges,
            post_dep_parents,
        })
    }

    /// Create a sub-graph that prunes tasks already completed by the caller.
    /// `completed` is a snapshot of task keys that have finished in the parent
    /// graph — these are removed from the sub-graph so they don't run again.
    pub async fn new_pruned(
        config: &Arc<Config>,
        tasks: Vec<Task>,
        completed: &TaskCompletionState,
    ) -> eyre::Result<Self> {
        let mut deps = Self::new(config, tasks).await?;
        deps.did_work.extend(completed.did_work.iter().cloned());
        deps.cache_keys.extend(completed.cache_keys.clone());
        let mut to_remove = vec![];
        for idx in deps.graph.node_indices() {
            let key = task_key(&deps.graph[idx]);
            if completed.completed.contains(&key) {
                to_remove.push(idx);
            }
        }
        // Remove in reverse index order so petgraph swap-remove
        // doesn't invalidate indices we haven't processed yet
        to_remove.sort_unstable_by(|a, b| b.cmp(a));
        for idx in to_remove {
            deps.graph.remove_node(idx);
        }
        deps.mark_ambiguous_prefixes();
        Ok(deps)
    }

    /// main method to emit tasks that no longer have dependencies being waited on
    fn emit_leaves(&mut self) {
        let leaves = leaves(&self.graph);
        let leaves_is_empty = leaves.is_empty();

        for task in leaves {
            let key = task_key(&task);

            if self.sent.insert(key.clone()) {
                trace!("Scheduling task {0}", task.name);
                if let Err(e) = self.tx.send(Some(task)) {
                    trace!("Error sending task: {e:?}");
                    self.sent.remove(&key);
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

    /// Snapshot completed task state for nested task sub-graphs.
    pub fn completion_state(&self) -> TaskCompletionState {
        TaskCompletionState {
            completed: self.removed.clone(),
            did_work: self.did_work.clone(),
            cache_keys: self.cache_keys.clone(),
        }
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

    /// Mark a task as having executed or restored outputs.
    /// Used to invalidate dependent tasks' source freshness checks.
    pub fn mark_did_work(&mut self, task: &Task) {
        self.did_work.insert(task_key(task));
    }

    /// Record a stable artifact identity produced or reused by a completed task.
    pub fn mark_cache_key(&mut self, task: &Task, cache_key: String) {
        self.cache_keys.insert(task_key(task), cache_key);
    }

    /// Return the completed dependency state needed for freshness and artifact caching.
    pub fn dependency_state(&self, task: &Task) -> TaskDependencyState {
        let key = task_key(task);
        let deps = self
            .dep_edges
            .get(&key)
            .into_iter()
            .flatten()
            .chain(self.post_dep_parents.get(&key).into_iter().flatten())
            .collect::<HashSet<_>>();
        let mut cache_keys = deps
            .iter()
            .filter_map(|dep_key| self.cache_keys.get(dep_key).cloned())
            .collect::<Vec<_>>();
        cache_keys.sort();
        cache_keys.dedup();
        TaskDependencyState {
            cache_keys,
            any_did_work: deps.iter().any(|dep_key| self.did_work.contains(dep_key)),
            any_unkeyed_did_work: deps.iter().any(|dep_key| {
                self.did_work.contains(dep_key) && !self.cache_keys.contains_key(dep_key)
            }),
        }
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

    /// Mark tasks that share a display_name so their prefix includes args
    /// for disambiguation (e.g. `[test-docker 4.1]` vs `[test-docker 4.2]`).
    pub fn mark_ambiguous_prefixes(&mut self) {
        let mut name_to_indices: HashMap<String, Vec<petgraph::graph::NodeIndex>> = HashMap::new();
        for idx in self.graph.node_indices() {
            name_to_indices
                .entry(self.graph[idx].display_name.clone())
                .or_default()
                .push(idx);
        }
        for indices in name_to_indices.values() {
            if indices.len() > 1 {
                for &idx in indices {
                    self.graph[idx].show_args_in_prefix = true;
                }
            }
        }
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
    graph
        .externals(Direction::Outgoing)
        .map(|idx| graph[idx].clone())
        .collect()
}

pub(crate) fn task_cycle_label(task: &Task) -> String {
    let label = if task.args.is_empty() {
        task.name.clone()
    } else {
        format!("{} {}", task.name, task.args.join(" "))
    };
    let env_keys = task
        .env
        .0
        .iter()
        .filter_map(|directive| match directive {
            EnvDirective::Val(key, _, _) => Some(key),
            _ => None,
        })
        .sorted()
        .unique()
        .join(", ");
    if env_keys.is_empty() {
        label
    } else {
        format!("{label} [env: {env_keys}]")
    }
}

fn find_cycles(graph: &DiGraph<Task, ()>, limit: Option<usize>) -> Vec<Vec<NodeIndex>> {
    let mut cycles = Vec::new();
    for mut component in kosaraju_scc(graph) {
        component.sort_by_key(|node| node.index());
        let component: HashSet<_> = component.into_iter().collect();
        if component.len() == 1 {
            let node = *component.iter().next().unwrap();
            if graph.find_edge(node, node).is_some() {
                cycles.push(vec![node, node]);
            }
            if limit.is_some_and(|limit| cycles.len() >= limit) {
                return cycles;
            }
            continue;
        }

        let mut starts = component.iter().copied().collect_vec();
        starts.sort_by_key(|node| node.index());
        for start in starts {
            let mut path = vec![start];
            let mut in_path = HashSet::from([start]);
            let mut stack = vec![(
                start,
                graph
                    .neighbors_directed(start, Direction::Outgoing)
                    .filter(|node| component.contains(node))
                    .sorted_by_key(|node| node.index())
                    .collect_vec(),
                0,
            )];

            while !stack.is_empty() {
                let dependency = {
                    let (_, dependencies, next) = stack.last_mut().unwrap();
                    if *next < dependencies.len() {
                        let dependency = dependencies[*next];
                        *next += 1;
                        Some(dependency)
                    } else {
                        None
                    }
                };

                let Some(dependency) = dependency else {
                    let (node, _, _) = stack.pop().unwrap();
                    path.pop();
                    in_path.remove(&node);
                    continue;
                };
                if dependency == start {
                    let mut cycle = path.clone();
                    cycle.push(start);
                    cycles.push(cycle);
                    if limit.is_some_and(|limit| cycles.len() >= limit) {
                        return cycles;
                    }
                } else if dependency.index() >= start.index() && in_path.insert(dependency) {
                    path.push(dependency);
                    stack.push((
                        dependency,
                        graph
                            .neighbors_directed(dependency, Direction::Outgoing)
                            .filter(|node| component.contains(node))
                            .sorted_by_key(|node| node.index())
                            .collect_vec(),
                        0,
                    ));
                }
            }
        }
    }
    cycles
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(name: &str) -> Task {
        Task {
            name: name.to_string(),
            ..Default::default()
        }
    }

    fn deps_with_relationships(
        dep_edges: HashMap<TaskKey, HashSet<TaskKey>>,
        post_dep_parents: HashMap<TaskKey, HashSet<TaskKey>>,
    ) -> Deps {
        let (tx, _) = mpsc::unbounded_channel();
        Deps {
            graph: DiGraph::new(),
            sent: HashSet::new(),
            removed: HashSet::new(),
            executed: HashSet::new(),
            did_work: HashSet::new(),
            cache_keys: HashMap::new(),
            dep_edges,
            post_dep_parents,
            tx,
        }
    }

    #[test]
    fn dependency_state_tracks_direct_artifact_identity_and_unkeyed_work() {
        let a = task("a");
        let b = task("b");
        let c = task("c");
        let dep_edges = HashMap::from([
            (task_key(&b), HashSet::from([task_key(&a)])),
            (task_key(&c), HashSet::from([task_key(&b)])),
        ]);
        let mut deps = deps_with_relationships(dep_edges, HashMap::new());

        deps.mark_did_work(&b);
        assert_eq!(
            deps.dependency_state(&c),
            TaskDependencyState {
                cache_keys: vec![],
                any_did_work: true,
                any_unkeyed_did_work: true,
            }
        );

        deps.mark_cache_key(&b, "b-key".to_string());
        assert_eq!(
            deps.dependency_state(&c),
            TaskDependencyState {
                cache_keys: vec!["b-key".to_string()],
                any_did_work: true,
                any_unkeyed_did_work: false,
            }
        );
    }

    #[test]
    fn dependency_state_includes_post_dependency_parents() {
        let parent = task("parent");
        let post = task("post");
        let post_dep_parents =
            HashMap::from([(task_key(&post), HashSet::from([task_key(&parent)]))]);
        let mut deps = deps_with_relationships(HashMap::new(), post_dep_parents);

        deps.mark_did_work(&parent);
        deps.mark_cache_key(&parent, "parent-key".to_string());

        assert_eq!(
            deps.dependency_state(&post),
            TaskDependencyState {
                cache_keys: vec!["parent-key".to_string()],
                any_did_work: true,
                any_unkeyed_did_work: false,
            }
        );
    }

    #[tokio::test]
    async fn new_pruned_preserves_completed_artifact_state() {
        let completed_task = task("completed");
        let key = task_key(&completed_task);
        let completion_state = TaskCompletionState {
            completed: HashSet::from([key.clone()]),
            did_work: HashSet::from([key.clone()]),
            cache_keys: HashMap::from([(key.clone(), "completed-key".to_string())]),
        };
        let config = Config::get().await.unwrap();

        let deps = Deps::new_pruned(&config, vec![completed_task], &completion_state)
            .await
            .unwrap();
        let propagated = deps.completion_state();

        assert!(deps.is_empty());
        assert!(propagated.did_work.contains(&key));
        assert_eq!(
            propagated.cache_keys.get(&key).map(String::as_str),
            Some("completed-key")
        );
    }

    #[test]
    fn finds_cycle_path() {
        let mut graph = DiGraph::new();
        let a = graph.add_node(task("a"));
        let b = graph.add_node(task("b"));
        let c = graph.add_node(task("c"));
        graph.update_edge(a, b, ());
        graph.update_edge(b, c, ());
        graph.update_edge(c, a, ());

        let cycle = find_cycles(&graph, Some(1)).pop().unwrap();
        let labels = cycle
            .iter()
            .map(|&idx| task_cycle_label(&graph[idx]))
            .collect_vec();
        assert_eq!(labels, ["a", "b", "c", "a"]);
    }

    #[test]
    fn accepts_acyclic_graph() {
        let mut graph = DiGraph::new();
        let a = graph.add_node(task("a"));
        let b = graph.add_node(task("b"));
        graph.update_edge(b, a, ());

        assert!(find_cycles(&graph, None).is_empty());
    }

    #[test]
    fn accepts_deep_acyclic_graph() {
        let mut graph = DiGraph::new();
        let nodes = (0..10_000)
            .map(|i| graph.add_node(task(&format!("task-{i}"))))
            .collect_vec();
        for pair in nodes.windows(2) {
            graph.update_edge(pair[0], pair[1], ());
        }

        assert!(find_cycles(&graph, None).is_empty());
    }

    #[test]
    fn finds_overlapping_cycles() {
        let mut graph = DiGraph::new();
        let root = graph.add_node(task("root"));
        let left = graph.add_node(task("left"));
        let right = graph.add_node(task("right"));
        graph.update_edge(root, left, ());
        graph.update_edge(left, root, ());
        graph.update_edge(root, right, ());
        graph.update_edge(right, root, ());

        let cycles = find_cycles(&graph, None)
            .into_iter()
            .map(|cycle| {
                cycle
                    .iter()
                    .map(|&idx| task_cycle_label(&graph[idx]))
                    .collect_vec()
            })
            .collect_vec();

        assert_eq!(
            cycles,
            [["root", "left", "root"], ["root", "right", "root"]]
        );
    }

    #[test]
    fn cycle_label_disambiguates_environment_variants_without_values() {
        let mut task = task("build");
        task.args = vec!["linux".to_string()];
        task.env.0 = vec![
            EnvDirective::Val(
                "TOKEN".to_string(),
                "secret".to_string(),
                Default::default(),
            ),
            EnvDirective::Val(
                "TARGET".to_string(),
                "linux".to_string(),
                Default::default(),
            ),
        ];

        assert_eq!(task_cycle_label(&task), "build linux [env: TARGET, TOKEN]");
    }
}
