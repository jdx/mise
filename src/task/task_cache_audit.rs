use crate::config::{Config, Settings};
#[cfg(target_os = "linux")]
use crate::file;
use crate::task::Task;
use crate::task::task_source_checker::lexical_normalize;
#[cfg(target_os = "linux")]
use crate::task::task_source_checker::{
    build_output_matcher, build_source_matcher, task_cwd, task_source_match_root,
};
use eyre::Result;
use ignore::overrides::Override;
use serde::Serialize;
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
#[cfg(target_os = "linux")]
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use tempfile::NamedTempFile;
#[cfg(target_os = "linux")]
use tokio::sync::OnceCell;

const MAX_REPORTED_PATHS: usize = 20;

#[cfg(target_os = "linux")]
static STRACE: OnceCell<Option<PathBuf>> = OnceCell::const_new();

/// `true` once the report file has been truncated by this process: the first writer replaces a
/// report left by an earlier run, every later writer appends to it. Audited tasks run in parallel
/// and share one file, so each task's block is written under this lock to keep blocks intact.
static REPORT_FILE: Mutex<bool> = Mutex::new(false);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum AccessKind {
    Read,
    Write,
}

impl AccessKind {
    fn as_str(&self) -> &'static str {
        match self {
            AccessKind::Read => "read",
            AccessKind::Write => "write",
        }
    }
}

#[derive(Serialize)]
struct ReportEntry<'a> {
    task: &'a str,
    kind: &'a str,
    path: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TraceAccess {
    kind: AccessKind,
    path: PathBuf,
    base: Option<PathBuf>,
}

pub(crate) struct TaskCacheAudit {
    #[cfg(target_os = "linux")]
    strace: PathBuf,
    trace: NamedTempFile,
    root: PathBuf,
    source_root: PathBuf,
    sources: Override,
    outputs: Override,
    config_sources: BTreeSet<PathBuf>,
}

impl TaskCacheAudit {
    pub(crate) async fn prepare(task: &Task, config: &Arc<Config>) -> Result<Option<Self>> {
        if !task
            .cache
            .as_ref()
            .is_some_and(|cache| cache.enabled && cache.audit)
        {
            return Ok(None);
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (task, config);
            warn_once!("task cache audit is currently supported only on Linux with strace");
            Ok(None)
        }
        #[cfg(target_os = "linux")]
        {
            let Some(strace) = usable_strace().await else {
                return Ok(None);
            };
            let root = task_cwd(task, config).await?;
            let source_root = task_source_match_root(&root, config);
            let sources = build_source_matcher(&source_root, &root, &task.sources);
            let outputs = build_output_matcher(&root, &task.outputs.patterns())?;
            let config_sources = task
                .config_sources()
                .into_iter()
                .map(|path| {
                    lexical_normalize(&if path.is_absolute() {
                        path.to_path_buf()
                    } else {
                        root.join(path)
                    })
                })
                .collect();
            Ok(Some(Self {
                strace,
                trace: NamedTempFile::new()?,
                root,
                source_root,
                sources,
                outputs,
                config_sources,
            }))
        }
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn wrap(&self, program: OsString, args: &[String]) -> (OsString, Vec<String>) {
        let mut wrapped = vec![
            "-f".to_string(),
            "-qq".to_string(),
            "-yy".to_string(),
            "-e".to_string(),
            "trace=%file".to_string(),
            "-s".to_string(),
            "4096".to_string(),
            "-o".to_string(),
            self.trace.path().to_string_lossy().into_owned(),
            "--".to_string(),
            program.to_string_lossy().into_owned(),
        ];
        wrapped.extend(args.iter().cloned());
        (self.strace.clone().into_os_string(), wrapped)
    }

    #[cfg(not(target_os = "linux"))]
    pub(crate) fn wrap(&self, program: OsString, args: &[String]) -> (OsString, Vec<String>) {
        (program, args.to_vec())
    }

    pub(crate) fn report(&self, task: &Task) {
        let trace = match fs::read_to_string(self.trace.path()) {
            Ok(trace) => trace,
            Err(err) => {
                warn!(
                    "task {} cache audit could not read its trace: {err}",
                    task.name
                );
                return;
            }
        };
        let mut undeclared = BTreeSet::new();
        for line in trace.lines() {
            for access in parse_trace_line(line) {
                let path = lexical_normalize(&if access.path.is_absolute() {
                    access.path
                } else {
                    access
                        .base
                        .unwrap_or_else(|| self.root.clone())
                        .join(access.path)
                });
                let scope_root = if access.kind == AccessKind::Read {
                    &self.source_root
                } else {
                    &self.root
                };
                let Ok(relative) = path.strip_prefix(scope_root) else {
                    continue;
                };
                if relative.as_os_str().is_empty()
                    || (access.kind == AccessKind::Read && path.is_dir())
                    || self.is_declared(access.kind, &path)
                {
                    continue;
                }
                undeclared.insert((access.kind, relative_to(&self.root, &path)));
            }
        }
        let total = undeclared.len();
        let mut report = Settings::get().task.cache.audit_report.clone();
        if let Some(path) = &report
            && let Err(err) = write_report(path, task, &undeclared)
        {
            warn!(
                "task {} cache audit could not write its report to {}: {err}",
                task.name,
                path.display()
            );
            report = None;
        }
        for (kind, path) in undeclared.into_iter().take(MAX_REPORTED_PATHS) {
            warn!(
                "task {} cache audit detected undeclared {}: {}",
                task.name,
                kind.as_str(),
                path.display()
            );
        }
        if total > MAX_REPORTED_PATHS {
            let omitted = total - MAX_REPORTED_PATHS;
            match &report {
                Some(path) => warn!(
                    "task {} cache audit omitted {omitted} additional paths; full report written to {}",
                    task.name,
                    path.display()
                ),
                None => warn!(
                    "task {} cache audit omitted {omitted} additional paths",
                    task.name
                ),
            }
        }
    }

    fn is_declared(&self, kind: AccessKind, path: &Path) -> bool {
        if self.config_sources.contains(path) {
            return true;
        }
        if let Ok(relative) = path.strip_prefix(&self.root)
            && matches_override(&self.outputs, relative)
        {
            return true;
        }
        kind == AccessKind::Read
            && path
                .strip_prefix(&self.source_root)
                .is_ok_and(|relative| matches_override(&self.sources, relative))
    }
}

fn write_report(
    path: &Path,
    task: &Task,
    undeclared: &BTreeSet<(AccessKind, PathBuf)>,
) -> Result<()> {
    let mut block = String::new();
    for (kind, undeclared_path) in undeclared {
        let undeclared_path = undeclared_path.to_string_lossy();
        let entry = ReportEntry {
            task: &task.name,
            kind: kind.as_str(),
            path: &undeclared_path,
        };
        block.push_str(&serde_json::to_string(&entry)?);
        block.push('\n');
    }
    let mut truncated = REPORT_FILE.lock().unwrap_or_else(|err| err.into_inner());
    let mut file = if *truncated {
        fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?
    } else {
        fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(path)?
    };
    file.write_all(block.as_bytes())?;
    *truncated = true;
    Ok(())
}

#[cfg(target_os = "linux")]
async fn usable_strace() -> Option<PathBuf> {
    STRACE
        .get_or_init(|| async {
            let Some(strace) = file::which("strace") else {
                warn_once!("task cache audit requires strace; running without filesystem auditing");
                return None;
            };
            let status = tokio::process::Command::new(&strace)
                .args(["-qq", "-yy", "-e", "trace=none", "--", "true"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .await;
            if !status.is_ok_and(|status| status.success()) {
                warn_once!(
                    "task cache audit could not start strace; running without filesystem auditing"
                );
                return None;
            }
            Some(strace)
        })
        .await
        .clone()
}

fn matches_override(matcher: &Override, path: &Path) -> bool {
    matcher.matched(path, false).is_whitelist() || matcher.matched(path, true).is_whitelist()
}

fn parse_trace_line(line: &str) -> Vec<TraceAccess> {
    if line.contains(" = -1 ") || line.ends_with(" = -1") {
        return Vec::new();
    }
    let Some(open) = line.find('(') else {
        return Vec::new();
    };
    let syscall = line[..open].split_whitespace().last().unwrap_or_default();
    let arguments = &line[open + 1..];
    let mut paths = quoted_paths(arguments).into_iter();
    let write = matches!(
        syscall,
        "creat"
            | "mkdir"
            | "mkdirat"
            | "mknod"
            | "mknodat"
            | "rename"
            | "renameat"
            | "renameat2"
            | "rmdir"
            | "truncate"
            | "unlink"
            | "unlinkat"
            | "utime"
            | "utimes"
            | "utimensat"
            | "chmod"
            | "fchmodat"
            | "chown"
            | "lchown"
            | "fchownat"
            | "link"
            | "linkat"
            | "symlink"
            | "symlinkat"
    ) || matches!(syscall, "open" | "openat" | "openat2")
        && ["O_WRONLY", "O_RDWR", "O_CREAT", "O_TRUNC"]
            .iter()
            .any(|flag| contains_unquoted_token(arguments, flag));
    if write {
        paths
            .map(|(path, base)| TraceAccess {
                kind: AccessKind::Write,
                path,
                base,
            })
            .collect()
    } else {
        paths
            .next()
            .map(|(path, base)| {
                vec![TraceAccess {
                    kind: AccessKind::Read,
                    path,
                    base,
                }]
            })
            .unwrap_or_default()
    }
}

fn contains_unquoted_token(input: &str, expected: &str) -> bool {
    let mut quoted = false;
    let mut escaped = false;
    let mut token = Vec::new();
    for byte in input.bytes() {
        if quoted {
            match (escaped, byte) {
                (true, _) => escaped = false,
                (false, b'\\') => escaped = true,
                (false, b'"') => quoted = false,
                _ => {}
            }
        } else if byte == b'"' {
            if token == expected.as_bytes() {
                return true;
            }
            token.clear();
            quoted = true;
        } else if byte.is_ascii_alphanumeric() || byte == b'_' {
            token.push(byte);
        } else {
            if token == expected.as_bytes() {
                return true;
            }
            token.clear();
        }
    }
    token == expected.as_bytes()
}

#[cfg(test)]
fn quoted_strings(input: &str) -> Vec<String> {
    quoted_paths(input)
        .into_iter()
        .map(|(path, _)| path.to_string_lossy().into_owned())
        .collect()
}

fn quoted_paths(input: &str) -> Vec<(PathBuf, Option<PathBuf>)> {
    let bytes = input.as_bytes();
    let mut values = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'"' {
            index += 1;
            continue;
        }
        let start = index;
        index += 1;
        let mut escaped = false;
        while index < bytes.len() {
            match (escaped, bytes[index]) {
                (true, _) => escaped = false,
                (false, b'\\') => escaped = true,
                (false, b'"') => {
                    let encoded = &input[start..=index];
                    if let Ok(value) = serde_json::from_str::<String>(encoded) {
                        let base = dirfd_base(&input[..start]);
                        values.push((PathBuf::from(value), base));
                    }
                    index += 1;
                    break;
                }
                _ => {}
            }
            index += 1;
        }
    }
    values
}

fn dirfd_base(input: &str) -> Option<PathBuf> {
    let end = input.rfind('>')?;
    let start = input[..end].rfind('<')?;
    let path = Path::new(&input[start + 1..end]);
    path.is_absolute().then(|| path.to_path_buf())
}

/// Render `path` relative to `base`, climbing with `..` when `path` is not
/// beneath `base`. Both must be absolute and lexically normalized.
///
/// Reads are audited against the workspace root, so one can legitimately sit
/// above the task directory. Rendering those against a different base than
/// in-task reads makes two distinct files print the same string, and neither
/// string is usable as a `sources` entry.
fn relative_to(base: &Path, path: &Path) -> PathBuf {
    let base: Vec<_> = base.components().collect();
    let path: Vec<_> = path.components().collect();
    let common = base.iter().zip(&path).take_while(|(b, p)| b == p).count();
    let mut relative = PathBuf::new();
    for _ in common..base.len() {
        relative.push("..");
    }
    relative.extend(&path[common..]);
    relative
}

#[cfg(test)]
mod tests {
    use super::{AccessKind, TraceAccess, parse_trace_line, quoted_strings, relative_to};
    use std::path::{Path, PathBuf};

    #[test]
    fn renders_paths_relative_to_the_task_directory() {
        let base = Path::new("/workspace/pkg");
        assert_eq!(
            relative_to(base, Path::new("/workspace/pkg/node_modules/dep.js")),
            PathBuf::from("node_modules/dep.js")
        );
        assert_eq!(
            relative_to(base, Path::new("/workspace/node_modules/dep.js")),
            PathBuf::from("../node_modules/dep.js")
        );
        assert_eq!(
            relative_to(base, Path::new("/node_modules/dep.js")),
            PathBuf::from("../../node_modules/dep.js")
        );
        assert_eq!(
            relative_to(base, Path::new("/workspace/other/dep.js")),
            PathBuf::from("../other/dep.js")
        );
    }

    #[test]
    fn parses_strace_file_accesses() {
        assert_eq!(
            parse_trace_line(r#"123 openat(AT_FDCWD, "src/input.txt", O_RDONLY) = 3"#),
            vec![TraceAccess {
                kind: AccessKind::Read,
                path: PathBuf::from("src/input.txt"),
                base: None,
            }]
        );
        assert_eq!(
            parse_trace_line(
                r#"123 openat(AT_FDCWD, "dist/output.txt", O_WRONLY|O_CREAT|O_TRUNC, 0666) = 3"#
            ),
            vec![TraceAccess {
                kind: AccessKind::Write,
                path: PathBuf::from("dist/output.txt"),
                base: None,
            }]
        );
        assert_eq!(
            parse_trace_line(r#"rename("tmp", "dist/output.txt") = 0"#),
            vec![
                TraceAccess {
                    kind: AccessKind::Write,
                    path: PathBuf::from("tmp"),
                    base: None,
                },
                TraceAccess {
                    kind: AccessKind::Write,
                    path: PathBuf::from("dist/output.txt"),
                    base: None,
                }
            ]
        );
        assert!(parse_trace_line(r#"access("missing", F_OK) = -1 ENOENT"#).is_empty());
        assert_eq!(
            parse_trace_line(r#"openat(AT_FDCWD, "O_CREAT-report", O_RDONLY) = 3"#),
            vec![TraceAccess {
                kind: AccessKind::Read,
                path: PathBuf::from("O_CREAT-report"),
                base: None,
            }]
        );
    }

    #[test]
    fn parses_escaped_quoted_paths() {
        assert_eq!(quoted_strings(r#"AT_FDCWD, "a\"b", O_RDONLY"#), ["a\"b"]);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn resolves_strace_dirfd_annotations() {
        assert_eq!(
            parse_trace_line(
                r#"123 openat(AT_FDCWD</workspace/pkg>, "src/input.txt", O_RDONLY) = 3</workspace/pkg/src/input.txt>"#
            ),
            vec![TraceAccess {
                kind: AccessKind::Read,
                path: PathBuf::from("src/input.txt"),
                base: Some(PathBuf::from("/workspace/pkg")),
            }]
        );
        assert_eq!(
            parse_trace_line(
                r#"123 openat(3</workspace/shared>, "input.txt", O_RDONLY) = 4</workspace/shared/input.txt>"#
            ),
            vec![TraceAccess {
                kind: AccessKind::Read,
                path: PathBuf::from("input.txt"),
                base: Some(PathBuf::from("/workspace/shared")),
            }]
        );
    }
}
