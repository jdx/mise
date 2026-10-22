use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use eyre::{Context, Result, bail};

use super::{
    ProjectId, WorkspaceDiscoveryContext, WorkspaceProject, WorkspaceProvenance, WorkspaceProvider,
};

const SETTINGS_KOTLIN: &str = "settings.gradle.kts";
const SETTINGS_GROOVY: &str = "settings.gradle";
const BUILD_KOTLIN: &str = "build.gradle.kts";
const BUILD_GROOVY: &str = "build.gradle";

/// Discovers Gradle projects listed by a settings script.
///
/// Gradle declares its members with `include` directives in `settings.gradle`
/// or `settings.gradle.kts`. This provider reads that script statically and
/// never runs Gradle, the wrapper, or the Tooling API.
///
/// Gradle is the first ecosystem whose structure lives in an executable build
/// script rather than in data, so membership that is not a literal string (an
/// interpolated, looped, or otherwise computed `include`) is an error rather
/// than a guess: a partial project list would make `--affected` silently
/// wrong. The error names the `[monorepo.projects]` override that replaces the
/// provider. Edges are more forgiving: a project whose dependencies cannot be
/// read confidently is still reported, with no edges, and `depends_add` can
/// supply them.
#[derive(Debug, Default)]
pub struct GradleWorkspaceProvider;

impl WorkspaceProvider for GradleWorkspaceProvider {
    fn id(&self) -> &str {
        "gradle"
    }

    fn discover(&self, workspace_root: &Path) -> Result<Vec<WorkspaceProject>> {
        let context = WorkspaceDiscoveryContext::new();
        self.discover_with_context(workspace_root, &context)
    }

    fn discover_with_context(
        &self,
        workspace_root: &Path,
        context: &WorkspaceDiscoveryContext,
    ) -> Result<Vec<WorkspaceProject>> {
        let Some((settings_name, settings_path)) = settings_script(context, workspace_root)? else {
            return Ok(Vec::new());
        };
        let contents = context.read_to_string(&settings_path).wrap_err_with(|| {
            format!(
                "failed to read Gradle settings script {}",
                settings_path.display()
            )
        })?;
        let settings = parse_settings(&tokenize(&contents)).wrap_err_with(|| {
            format!(
                "cannot statically read Gradle projects from {}; declare them under \
                 [monorepo.projects.\"gradle:...\"] instead",
                settings_path.display()
            )
        })?;
        if settings.members.is_empty() {
            return Ok(Vec::new());
        }

        // Gradle project path -> workspace-relative directory.
        let directory_of = |member: &str| {
            settings
                .remapped
                .get(member)
                .cloned()
                .unwrap_or_else(|| project_directory(member))
        };
        let mut roots = BTreeMap::new();
        for member in &settings.members {
            let directory = directory_of(member);
            if is_repository_relative(&directory) {
                roots.insert(member.clone(), directory);
            }
        }
        // `include(":a:b")` also creates `:a`. It is a graph project only when
        // it has a build script of its own, like the root project.
        for member in &settings.members {
            for parent in parent_paths(member) {
                let directory = directory_of(&parent);
                if !roots.contains_key(&parent)
                    && is_repository_relative(&directory)
                    && build_script(context, workspace_root, &directory).is_some()
                {
                    roots.insert(parent, directory);
                }
            }
        }
        // The root project only participates when it has a build script of its
        // own, mirroring how the Cargo provider treats a workspace root package.
        if build_script(context, workspace_root, Path::new(".")).is_some() {
            roots.insert(":".to_string(), PathBuf::from("."));
        }

        let mut ids = BTreeMap::new();
        for member in roots.keys() {
            ids.insert(member.clone(), ProjectId::new(self.id(), member)?);
        }

        let mut projects = Vec::new();
        for (member, directory) in &roots {
            let Some(id) = ids.get(member) else {
                continue;
            };
            let mut project = WorkspaceProject::new(id.clone(), directory.clone());
            project
                .metadata
                .insert("workspace_source".to_string(), settings_name.clone());

            let script = build_script(context, workspace_root, directory);
            let source = script
                .as_ref()
                .and_then(|path| path.strip_prefix(workspace_root).ok())
                .map_or_else(|| PathBuf::from(&settings_name), Path::to_path_buf);
            project.provenance = WorkspaceProvenance {
                provider: Some(self.id().to_string()),
                source: Some(source.clone()),
            };

            if let Some(script) = script {
                let script_contents = context.read_to_string(&script).wrap_err_with(|| {
                    format!("failed to read Gradle build script {}", script.display())
                })?;
                for referenced in project_references(&tokenize(&script_contents)) {
                    let Some(dependency) = ids.get(&referenced) else {
                        continue;
                    };
                    if dependency == id {
                        continue;
                    }
                    project.dependencies.insert(dependency.clone());
                    project.dependency_provenance.insert(
                        dependency.clone(),
                        WorkspaceProvenance {
                            provider: Some(self.id().to_string()),
                            source: Some(source.clone()),
                        },
                    );
                }
            }

            projects.push(project);
        }

        Ok(projects)
    }
}

/// A statically parsed settings script.
#[derive(Debug, Default, Eq, PartialEq)]
struct GradleSettings {
    /// Member project paths in declaration order, such as `:app`.
    members: Vec<String>,
    /// Member paths whose directory is set explicitly, keyed by project path.
    remapped: BTreeMap<String, PathBuf>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Kind {
    Ident(String),
    /// A string literal; `None` when it has interpolation, escapes, or is a
    /// multi-line string, so its value is not statically known.
    Str(Option<String>),
    Punct(char),
}

#[derive(Clone, Debug)]
struct Token {
    kind: Kind,
    newline_before: bool,
}

/// Splits Groovy or Kotlin source into identifiers, strings and punctuation,
/// dropping whitespace and comments.
fn tokenize(source: &str) -> Vec<Token> {
    let chars = source.chars().collect::<Vec<_>>();
    let mut tokens = Vec::new();
    let mut newline = false;
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        if c == '\n' {
            newline = true;
            i += 1;
            continue;
        }
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if c == '/' && next == Some('/') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if c == '/' && next == Some('*') {
            i += 2;
            while i < chars.len() && !(chars[i] == '*' && chars.get(i + 1) == Some(&'/')) {
                newline |= chars[i] == '\n';
                i += 1;
            }
            i = (i + 2).min(chars.len());
            continue;
        }
        let kind = if c == '"' || c == '\'' {
            if chars[i..].starts_with(&[c, c, c]) {
                i += 3;
                while i < chars.len() && !chars[i..].starts_with(&[c, c, c]) {
                    i += 1;
                }
                i = (i + 3).min(chars.len());
                Kind::Str(None)
            } else {
                i += 1;
                let mut text = String::new();
                let mut plain = true;
                while i < chars.len() && chars[i] != c {
                    if chars[i] == '\\' {
                        plain = false;
                        i += 2;
                        continue;
                    }
                    plain &= chars[i] != '$';
                    text.push(chars[i]);
                    i += 1;
                }
                i += 1;
                Kind::Str(plain.then_some(text))
            }
        } else if c.is_alphanumeric() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            Kind::Ident(chars[start..i].iter().collect())
        } else {
            i += 1;
            Kind::Punct(c)
        };
        tokens.push(Token {
            kind,
            newline_before: newline,
        });
        newline = false;
    }

    tokens
}

fn ident_at(tokens: &[Token], index: usize) -> Option<&str> {
    match tokens.get(index).map(|token| &token.kind) {
        Some(Kind::Ident(name)) => Some(name),
        _ => None,
    }
}

fn punct_at(tokens: &[Token], index: usize, expected: char) -> bool {
    matches!(tokens.get(index).map(|token| &token.kind), Some(Kind::Punct(c)) if *c == expected)
}

fn is_member_access(tokens: &[Token], index: usize) -> bool {
    index > 0 && punct_at(tokens, index - 1, '.')
}

/// Returns the index of the bracket closing the one at `open`.
fn matching(tokens: &[Token], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate().skip(open) {
        match token.kind {
            Kind::Punct('(' | '[' | '{') => depth += 1,
            Kind::Punct(')' | ']' | '}') => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

/// Splits tokens on top-level commas.
fn split_arguments(tokens: &[Token]) -> Vec<&[Token]> {
    let mut arguments = Vec::new();
    let mut depth = 0usize;
    let mut start = 0;
    for (index, token) in tokens.iter().enumerate() {
        match token.kind {
            Kind::Punct('(' | '[' | '{') => depth += 1,
            Kind::Punct(')' | ']' | '}') => depth = depth.saturating_sub(1),
            Kind::Punct(',') if depth == 0 => {
                arguments.push(&tokens[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }
    arguments.push(&tokens[start..]);
    arguments
}

/// Reads the arguments of a call whose arguments start at `at`, and returns
/// the index just past the call.
///
/// Handles `f(a, b)` and Groovy command syntax, `f a, b`, which continues
/// across lines after a trailing comma.
fn call_arguments(tokens: &[Token], at: usize) -> Option<(usize, Vec<&[Token]>)> {
    let first = tokens.get(at)?;
    if punct_at(tokens, at, '(') {
        let close = matching(tokens, at)?;
        let mut arguments = split_arguments(&tokens[at + 1..close]);
        if arguments.last().is_some_and(|argument| argument.is_empty()) {
            arguments.pop();
        }
        return Some((close + 1, arguments));
    }
    if first.newline_before {
        return None;
    }
    let mut depth = 0usize;
    let mut end = at;
    while end < tokens.len() {
        if end > at && depth == 0 && tokens[end].newline_before && !punct_at(tokens, end - 1, ',') {
            break;
        }
        match tokens[end].kind {
            Kind::Punct('(' | '[' | '{') => depth += 1,
            Kind::Punct(')' | ']' | '}') => {
                if depth == 0 {
                    break;
                }
                depth -= 1;
            }
            Kind::Punct(';') if depth == 0 => break,
            _ => {}
        }
        end += 1;
    }
    Some((end, split_arguments(&tokens[at..end])))
}

/// The value of an argument that is exactly one plain string literal.
fn plain_literal(argument: &[Token]) -> Option<&str> {
    match argument {
        [
            Token {
                kind: Kind::Str(Some(value)),
                ..
            },
        ] => Some(value),
        _ => None,
    }
}

/// The value of `":lib"`, `path: ":lib"` or `path = ":lib"`.
fn path_argument(argument: &[Token], positional: bool) -> Option<&str> {
    if positional && let Some(value) = plain_literal(argument) {
        return Some(value);
    }
    let [name, separator, value] = argument else {
        return None;
    };
    let named = matches!(&name.kind, Kind::Ident(name) if name == "path");
    let separated = matches!(separator.kind, Kind::Punct(':' | '='));
    if named && separated {
        plain_literal(std::slice::from_ref(value))
    } else {
        None
    }
}

/// Parses `include` directives and `projectDir` remaps out of a settings script.
///
/// Fails when membership is not statically enumerable, so the caller does not
/// report a partial project list.
fn parse_settings(tokens: &[Token]) -> Result<GradleSettings> {
    let mut settings = GradleSettings::default();
    let mut depth = 0usize;
    let mut index = 0;

    while index < tokens.len() {
        match tokens[index].kind {
            Kind::Punct('{') => depth += 1,
            Kind::Punct('}') => depth = depth.saturating_sub(1),
            _ => {}
        }
        let Some(name) = ident_at(tokens, index) else {
            index += 1;
            continue;
        };
        let member = is_member_access(tokens, index);
        let on_settings = member && index >= 2 && ident_at(tokens, index - 2) == Some("settings");
        if member && !on_settings {
            index += 1;
            continue;
        }
        match name {
            "include" => {
                // Inside a block or after a condition, whether the project is
                // included depends on the build, not the script text.
                let statement = if on_settings { index - 2 } else { index };
                if depth > 0 || follows_condition(tokens, statement) {
                    bail!("`include` is conditional or nested in a block");
                }
                let Some((next, arguments)) = call_arguments(tokens, index + 1) else {
                    bail!("`include` is not followed by a readable argument list");
                };
                for argument in arguments {
                    let Some(path) = plain_literal(argument) else {
                        bail!("`include` has an argument that is not a plain string literal");
                    };
                    let path = project_path(path)?;
                    if !settings.members.contains(&path) {
                        settings.members.push(path);
                    }
                }
                index = next;
                continue;
            }
            "project" if !member && punct_at(tokens, index + 1, '(') => {
                if let Some((next, path, directory)) = project_remap(tokens, index)? {
                    settings.remapped.insert(path, directory);
                    index = next;
                    continue;
                }
            }
            _ => {}
        }
        index += 1;
    }

    Ok(settings)
}

/// Whether the statement at `index` is the body of an `if`, `else`, `for`, or
/// `while` without braces, as in `if (ci) include(":ci")`.
fn follows_condition(tokens: &[Token], index: usize) -> bool {
    let Some(previous) = index.checked_sub(1) else {
        return false;
    };
    if ident_at(tokens, previous) == Some("else") {
        return true;
    }
    if !punct_at(tokens, previous, ')') {
        return false;
    }
    let mut depth = 0usize;
    for open in (0..=previous).rev() {
        match tokens[open].kind {
            Kind::Punct(')') => depth += 1,
            Kind::Punct('(') => {
                depth -= 1;
                if depth == 0 {
                    return matches!(
                        ident_at(tokens, open.wrapping_sub(1)),
                        Some("if" | "for" | "while")
                    );
                }
            }
            _ => {}
        }
    }
    false
}

/// Parses `project(":app").projectDir = file("modules/app")` starting at `project`.
///
/// Returns `None` for any other use of `project(...)`.
fn project_remap(tokens: &[Token], index: usize) -> Result<Option<(usize, String, PathBuf)>> {
    let Some((next, arguments)) = call_arguments(tokens, index + 1) else {
        return Ok(None);
    };
    if !(punct_at(tokens, next, '.')
        && ident_at(tokens, next + 1) == Some("projectDir")
        && punct_at(tokens, next + 2, '='))
    {
        return Ok(None);
    }
    let Some(path) = arguments
        .first()
        .and_then(|argument| path_argument(argument, true))
    else {
        bail!(
            "`project(...).projectDir` assignment has a project path that is not a plain string literal"
        );
    };
    let path = project_path(path)?;
    let readable = matches!(ident_at(tokens, next + 3), Some("file" | "File"))
        && punct_at(tokens, next + 4, '(');
    let directory = readable
        .then(|| call_arguments(tokens, next + 4))
        .flatten()
        .and_then(|(end, arguments)| match arguments.as_slice() {
            [argument] => plain_literal(argument).map(|directory| (end, directory)),
            _ => None,
        });
    let Some((end, directory)) = directory else {
        bail!("`projectDir` of {path} is not a plain `file(\"...\")` literal");
    };
    Ok(Some((end, path, PathBuf::from(directory))))
}

/// Normalizes an `include` argument to an absolute Gradle project path.
fn project_path(value: &str) -> Result<String> {
    let path = if value.starts_with(':') {
        value.to_string()
    } else {
        format!(":{value}")
    };
    if path == ":"
        || path.ends_with(':')
        || path.contains("::")
        || path.chars().any(char::is_whitespace)
    {
        bail!("{value:?} is not a valid Gradle project path");
    }
    Ok(path)
}

/// Reads the member paths referenced by `project(...)` calls inside the
/// build script's top-level `dependencies { }` blocks.
///
/// Blocks nested in `project(":x") { }`, `subprojects { }`, `buildscript { }`
/// and the like configure other projects or the build itself, so they are
/// skipped.
fn project_references(tokens: &[Token]) -> Vec<String> {
    let mut references = Vec::new();
    let mut depth = 0usize;
    let mut index = 0;

    while index < tokens.len() {
        match tokens[index].kind {
            Kind::Punct('{') => depth += 1,
            Kind::Punct('}') => depth = depth.saturating_sub(1),
            _ => {}
        }
        if depth == 0
            && ident_at(tokens, index) == Some("dependencies")
            && !is_member_access(tokens, index)
            && punct_at(tokens, index + 1, '{')
            && let Some(close) = matching(tokens, index + 1)
        {
            collect_references(&tokens[index + 2..close], &mut references);
            index = close + 1;
            continue;
        }
        index += 1;
    }

    references
}

fn collect_references(block: &[Token], references: &mut Vec<String>) {
    for index in 0..block.len() {
        if ident_at(block, index) != Some("project")
            || is_member_access(block, index)
            || !punct_at(block, index + 1, '(')
        {
            continue;
        }
        let Some((_, arguments)) = call_arguments(block, index + 1) else {
            continue;
        };
        for (position, argument) in arguments.into_iter().enumerate() {
            let Some(path) = path_argument(argument, position == 0) else {
                continue;
            };
            if path.starts_with(':') && !references.iter().any(|known| known == path) {
                references.push(path.to_string());
            }
        }
    }
}

fn settings_script(
    context: &WorkspaceDiscoveryContext,
    workspace_root: &Path,
) -> Result<Option<(String, PathBuf)>> {
    let found = [SETTINGS_GROOVY, SETTINGS_KOTLIN]
        .into_iter()
        .map(|name| (name, workspace_root.join(name)))
        .filter(|(_, path)| context.is_file(path))
        .collect::<Vec<_>>();
    match found.as_slice() {
        [] => Ok(None),
        [(name, path)] => Ok(Some(((*name).to_string(), path.clone()))),
        _ => bail!(
            "both {SETTINGS_GROOVY} and {SETTINGS_KOTLIN} exist in {}; keep one so the Gradle \
             projects are unambiguous",
            workspace_root.display()
        ),
    }
}

fn build_script(
    context: &WorkspaceDiscoveryContext,
    workspace_root: &Path,
    directory: &Path,
) -> Option<PathBuf> {
    let root = if directory.as_os_str().is_empty() || directory == Path::new(".") {
        workspace_root.to_path_buf()
    } else {
        workspace_root.join(directory)
    };

    for name in [BUILD_KOTLIN, BUILD_GROOVY] {
        let path = root.join(name);
        if context.is_file(&path) {
            return Some(path);
        }
    }
    None
}

/// Converts a Gradle project path such as `:a:b` to a workspace-relative directory.
fn project_directory(path: &str) -> PathBuf {
    let trimmed = path.strip_prefix(':').unwrap_or(path);
    if trimmed.is_empty() {
        return PathBuf::from(".");
    }
    trimmed.split(':').collect()
}

fn is_repository_relative(directory: &Path) -> bool {
    !directory.is_absolute()
        && !directory
            .components()
            .any(|component| matches!(component, Component::ParentDir))
}

/// The enclosing project paths of `:a:b:c`: `:a` and `:a:b`.
fn parent_paths(path: &str) -> Vec<String> {
    let segments = path.trim_start_matches(':').split(':').collect::<Vec<_>>();
    (1..segments.len())
        .map(|count| format!(":{}", segments[..count].join(":")))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    fn write(path: &Path, contents: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    fn settings(root: &Path, contents: &str) {
        write(&root.join(SETTINGS_KOTLIN), contents);
    }

    fn assert_declined(root: &Path) {
        let error = GradleWorkspaceProvider.discover(root).unwrap_err();
        assert!(
            format!("{error:#}").contains("[monorepo.projects.\"gradle:...\"]"),
            "{error:#}"
        );
    }

    fn summary(projects: &[WorkspaceProject]) -> Vec<(&str, &Path)> {
        projects
            .iter()
            .map(|project| (project.id.as_str(), project.root.as_path()))
            .collect()
    }

    #[test]
    fn discovers_members_and_dependency_edges() {
        let temp = tempdir().unwrap();
        settings(
            temp.path(),
            "rootProject.name = \"acme\"\ninclude(\":app\", \":lib\")\n",
        );
        write(
            &temp.path().join("app/build.gradle.kts"),
            "dependencies {\n    implementation(project(\":lib\"))\n}\n",
        );

        let projects = GradleWorkspaceProvider.discover(temp.path()).unwrap();

        assert_eq!(
            summary(&projects),
            vec![
                ("gradle::app", Path::new("app")),
                ("gradle::lib", Path::new("lib"))
            ]
        );
        let app = projects
            .iter()
            .find(|project| project.id.as_str() == "gradle::app")
            .unwrap();
        assert_eq!(
            app.dependencies
                .iter()
                .map(ProjectId::as_str)
                .collect::<Vec<_>>(),
            vec!["gradle::lib"]
        );
        assert_eq!(
            app.metadata.get("workspace_source").unwrap(),
            SETTINGS_KOTLIN
        );
        assert_eq!(
            app.provenance.source,
            Some(PathBuf::from("app/build.gradle.kts"))
        );
    }

    #[test]
    fn supports_groovy_command_syntax_and_nested_paths() {
        let temp = tempdir().unwrap();
        write(
            &temp.path().join(SETTINGS_GROOVY),
            "include ':services:person-service', ':lib' // members\n",
        );

        let projects = GradleWorkspaceProvider.discover(temp.path()).unwrap();

        assert_eq!(
            summary(&projects),
            vec![
                ("gradle::lib", Path::new("lib")),
                (
                    "gradle::services:person-service",
                    Path::new("services/person-service")
                ),
            ]
        );
        assert_eq!(
            projects[0].metadata.get("workspace_source").unwrap(),
            SETTINGS_GROOVY
        );
    }

    #[test]
    fn errors_on_computed_membership() {
        let temp = tempdir().unwrap();
        settings(
            temp.path(),
            "file(\"modules\").listFiles()?.forEach { include(it.name) }\n",
        );

        assert_declined(temp.path());
    }

    #[test]
    fn errors_on_interpolated_membership() {
        let temp = tempdir().unwrap();
        settings(temp.path(), "include(\"$rootDir/app\")\n");

        assert_declined(temp.path());
    }

    #[test]
    fn errors_on_a_single_non_literal_member_among_literals() {
        let temp = tempdir().unwrap();
        settings(temp.path(), "include(\":app\")\ninclude(dynamic)\n");

        assert_declined(temp.path());
    }

    #[test]
    fn ignores_include_build() {
        let temp = tempdir().unwrap();
        settings(temp.path(), "includeBuild(\"../other\")\n");

        assert!(
            GradleWorkspaceProvider
                .discover(temp.path())
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn ignores_roots_without_a_settings_script() {
        let temp = tempdir().unwrap();
        write(&temp.path().join("build.gradle.kts"), "plugins {}\n");

        assert!(
            GradleWorkspaceProvider
                .discover(temp.path())
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn includes_the_root_project_only_when_it_has_a_build_script() {
        let temp = tempdir().unwrap();
        settings(temp.path(), "include(\":app\")\n");

        let projects = GradleWorkspaceProvider.discover(temp.path()).unwrap();
        assert_eq!(summary(&projects), vec![("gradle::app", Path::new("app"))]);

        write(&temp.path().join("build.gradle.kts"), "plugins {}\n");
        let projects = GradleWorkspaceProvider.discover(temp.path()).unwrap();
        assert_eq!(
            summary(&projects),
            vec![
                ("gradle::", Path::new(".")),
                ("gradle::app", Path::new("app"))
            ]
        );
    }

    #[test]
    fn resolves_explicit_project_directories() {
        let temp = tempdir().unwrap();
        settings(
            temp.path(),
            "include(\":app\")\nproject(\":app\").projectDir = file(\"modules/app\")\n",
        );

        let projects = GradleWorkspaceProvider.discover(temp.path()).unwrap();

        assert_eq!(
            summary(&projects),
            vec![("gradle::app", Path::new("modules/app"))]
        );
    }

    #[test]
    fn skips_self_edges_and_unresolved_references() {
        let temp = tempdir().unwrap();
        settings(temp.path(), "include(\":app\")\n");
        write(
            &temp.path().join("app/build.gradle.kts"),
            "dependencies {\n    implementation(project(\":app\"))\n    \
             implementation(project(\":missing\"))\n    \
             implementation(\"com.example:external:1.0\")\n}\n",
        );

        let projects = GradleWorkspaceProvider.discover(temp.path()).unwrap();

        assert!(projects[0].dependencies.is_empty());
    }

    #[test]
    fn reads_labelled_project_references() {
        let temp = tempdir().unwrap();
        settings(temp.path(), "include(\":app\", \":lib\")\n");
        write(
            &temp.path().join("app/build.gradle"),
            "dependencies {\n    implementation project(path: ':lib', configuration: 'default')\n}\n",
        );

        let projects = GradleWorkspaceProvider.discover(temp.path()).unwrap();
        let app = projects
            .iter()
            .find(|project| project.id.as_str() == "gradle::app")
            .unwrap();

        assert_eq!(
            app.dependencies
                .iter()
                .map(ProjectId::as_str)
                .collect::<Vec<_>>(),
            vec!["gradle::lib"]
        );
    }

    #[test]
    fn ignores_identifiers_inside_comments_and_strings() {
        let temp = tempdir().unwrap();
        settings(
            temp.path(),
            "// include(\":commented-out\")\nval label = \"include(':string')\"\ninclude(\":app\")\n",
        );

        let projects = GradleWorkspaceProvider.discover(temp.path()).unwrap();

        assert_eq!(summary(&projects), vec![("gradle::app", Path::new("app"))]);
    }

    #[test]
    fn ignores_settings_without_any_members() {
        let temp = tempdir().unwrap();
        settings(temp.path(), "rootProject.name = \"single\"\n");

        assert!(
            GradleWorkspaceProvider
                .discover(temp.path())
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn converts_project_paths_to_directories() {
        assert_eq!(project_directory(":"), PathBuf::from("."));
        assert_eq!(project_directory(":app"), PathBuf::from("app"));
        assert_eq!(
            project_directory(":a:b:c"),
            PathBuf::from("a").join("b").join("c")
        );
    }

    #[test]
    fn errors_when_both_settings_scripts_exist() {
        let temp = tempdir().unwrap();
        write(&temp.path().join(SETTINGS_GROOVY), "include ':a'\n");
        write(&temp.path().join(SETTINGS_KOTLIN), "include(\":a\")\n");

        let error = GradleWorkspaceProvider.discover(temp.path()).unwrap_err();

        assert!(error.to_string().contains("both settings.gradle"));
    }

    #[test]
    fn accepts_include_paths_without_a_leading_colon() {
        let temp = tempdir().unwrap();
        settings(temp.path(), "include(\"app\", \"services:api\")\n");

        let projects = GradleWorkspaceProvider.discover(temp.path()).unwrap();

        assert_eq!(
            summary(&projects),
            vec![
                ("gradle::app", Path::new("app")),
                ("gradle::services:api", Path::new("services/api")),
            ]
        );
    }

    #[test]
    fn only_reads_edges_inside_dependencies_blocks() {
        let temp = tempdir().unwrap();
        settings(temp.path(), "include(\":app\", \":lib\", \":tools\")\n");
        write(
            &temp.path().join("app/build.gradle.kts"),
            "val helper = project(\":tools\")\n\
             dependencies {\n    // project(\":tools\")\n    \
             implementation(project(\":lib\")) { exclude(group = \"x\") }\n}\n",
        );

        let projects = GradleWorkspaceProvider.discover(temp.path()).unwrap();
        let app = projects
            .iter()
            .find(|project| project.id.as_str() == "gradle::app")
            .unwrap();

        assert_eq!(
            app.dependencies
                .iter()
                .map(ProjectId::as_str)
                .collect::<Vec<_>>(),
            vec!["gradle::lib"]
        );
    }

    #[test]
    fn skips_project_directories_outside_the_repository() {
        let temp = tempdir().unwrap();
        settings(
            temp.path(),
            "include(\":app\", \":shared\")\n\
             project(\":shared\").projectDir = file(\"../shared\")\n",
        );

        let projects = GradleWorkspaceProvider.discover(temp.path()).unwrap();

        assert_eq!(summary(&projects), vec![("gradle::app", Path::new("app"))]);
    }

    fn dependencies_of<'a>(projects: &'a [WorkspaceProject], id: &str) -> Vec<&'a str> {
        projects
            .iter()
            .find(|project| project.id.as_str() == id)
            .unwrap()
            .dependencies
            .iter()
            .map(ProjectId::as_str)
            .collect()
    }

    #[test]
    fn reads_non_ascii_names_and_comments_in_calls() {
        let temp = tempdir().unwrap();
        settings(
            temp.path(),
            "include(/* 括号 ) */ \":应用\", // ) trailing\n    \":模块\")\n\
             project(\":模块\").projectDir = file(\"模块/核心\")\n",
        );

        let projects = GradleWorkspaceProvider.discover(temp.path()).unwrap();

        assert_eq!(
            summary(&projects),
            vec![
                ("gradle::应用", Path::new("应用")),
                ("gradle::模块", Path::new("模块/核心")),
            ]
        );
    }

    #[test]
    fn reads_multi_line_groovy_includes() {
        let temp = tempdir().unwrap();
        write(
            &temp.path().join(SETTINGS_GROOVY),
            "include ':app',\n        ':lib',\n        ':tools'\ninclude ':last'\n",
        );

        let projects = GradleWorkspaceProvider.discover(temp.path()).unwrap();

        assert_eq!(projects.len(), 4);
    }

    #[test]
    fn reads_includes_on_the_settings_object() {
        let temp = tempdir().unwrap();
        settings(
            temp.path(),
            "settings.include(\":app\")\nother.include(\":x\")\n",
        );

        let projects = GradleWorkspaceProvider.discover(temp.path()).unwrap();

        assert_eq!(summary(&projects), vec![("gradle::app", Path::new("app"))]);
    }

    #[test]
    fn errors_on_concatenated_membership() {
        let temp = tempdir().unwrap();
        settings(temp.path(), "include(\":ap\" + \"p\")\n");

        assert_declined(temp.path());
    }

    #[test]
    fn errors_on_unreadable_project_dir_remaps() {
        let temp = tempdir().unwrap();
        settings(
            temp.path(),
            "include(\":app\")\nproject(\":app\").projectDir = File(rootDir, \"modules/app\")\n",
        );

        assert_declined(temp.path());
    }

    #[test]
    fn includes_parent_projects_that_have_build_scripts() {
        let temp = tempdir().unwrap();
        settings(temp.path(), "include(\":services:api\", \":tools:cli\")\n");
        write(
            &temp.path().join("services/build.gradle.kts"),
            "plugins {}\n",
        );
        write(
            &temp.path().join("services/api/build.gradle.kts"),
            "dependencies { implementation(project(\":services\")) }\n",
        );

        let projects = GradleWorkspaceProvider.discover(temp.path()).unwrap();

        assert_eq!(
            summary(&projects),
            vec![
                ("gradle::services", Path::new("services")),
                ("gradle::services:api", Path::new("services/api")),
                ("gradle::tools:cli", Path::new("tools/cli")),
            ]
        );
        assert_eq!(
            dependencies_of(&projects, "gradle::services:api"),
            vec!["gradle::services"]
        );
    }

    #[test]
    fn skips_dependencies_blocks_that_configure_other_projects() {
        let temp = tempdir().unwrap();
        settings(temp.path(), "include(\":app\", \":lib\", \":tools\")\n");
        write(&temp.path().join("build.gradle.kts"), "plugins {}\n");
        write(
            &temp.path().join("build.gradle.kts"),
            "buildscript { dependencies { classpath(project(\":tools\")) } }\n\
             project(\":app\") { dependencies { implementation(project(\":lib\")) } }\n\
             subprojects { dependencies { implementation(project(\":lib\")) } }\n\
             dependencies { implementation(project(\":app\")) }\n",
        );

        let projects = GradleWorkspaceProvider.discover(temp.path()).unwrap();

        assert_eq!(dependencies_of(&projects, "gradle::"), vec!["gradle::app"]);
    }

    #[test]
    fn reads_kotlin_named_project_arguments() {
        let temp = tempdir().unwrap();
        settings(temp.path(), "include(\":app\", \":lib\")\n");
        write(
            &temp.path().join("app/build.gradle.kts"),
            "dependencies { implementation(project(path = \":lib\", configuration = \"x\")) }\n",
        );

        let projects = GradleWorkspaceProvider.discover(temp.path()).unwrap();

        assert_eq!(
            dependencies_of(&projects, "gradle::app"),
            vec!["gradle::lib"]
        );
    }

    #[test]
    fn errors_on_conditional_includes() {
        for script in [
            "if (System.getenv(\"CI\") != null) {\n    include(\":ci\")\n}\n",
            "if (flag) include(\":x\")\n",
            "if (flag) settings.include(\":x\")\n",
            "if (flag) include(\":x\") else include(\":y\")\n",
        ] {
            let temp = tempdir().unwrap();
            settings(temp.path(), script);

            assert_declined(temp.path());
        }
    }

    #[test]
    fn reads_consecutive_includes_and_includes_after_calls() {
        let temp = tempdir().unwrap();
        settings(
            temp.path(),
            "enableFeaturePreview(\"X\")\ninclude(\":app\")\ninclude(\":lib\")\n\
             if (ci) { }\ninclude(\":tools\")\n",
        );

        let projects = GradleWorkspaceProvider.discover(temp.path()).unwrap();

        assert_eq!(projects.len(), 3);
    }

    #[test]
    fn reads_named_path_in_project_dir_remaps() {
        let temp = tempdir().unwrap();
        settings(
            temp.path(),
            "include(\":app\")\nproject(path = \":app\").projectDir = file(\"modules/app\")\n",
        );

        let projects = GradleWorkspaceProvider.discover(temp.path()).unwrap();

        assert_eq!(
            summary(&projects),
            vec![("gradle::app", Path::new("modules/app"))]
        );
    }
}
