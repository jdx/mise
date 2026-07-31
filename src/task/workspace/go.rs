use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

use eyre::{Context, Result, bail};

use super::{ProjectId, WorkspaceProject, WorkspaceProvenance, WorkspaceProvider};

const GO_MOD: &str = "go.mod";
const GO_WORK: &str = "go.work";

/// Discovers Go modules listed by a workspace file.
#[derive(Debug, Default)]
pub struct GoWorkspaceProvider;

impl WorkspaceProvider for GoWorkspaceProvider {
    fn id(&self) -> &str {
        "go"
    }

    fn discover(&self, workspace_root: &Path) -> Result<Vec<WorkspaceProject>> {
        let workfile_path = workspace_root.join(GO_WORK);
        if !workfile_path.is_file() {
            return Ok(Vec::new());
        }
        let canonical_root = workspace_root.canonicalize().wrap_err_with(|| {
            format!(
                "failed to resolve Go workspace root {}",
                workspace_root.display()
            )
        })?;
        let lexical_root = lexical_absolute(workspace_root)?;
        let module_directories = read_workfile(&workfile_path)?;
        let mut modules = BTreeMap::new();

        for directory in module_directories {
            let candidate = if directory.is_absolute() {
                directory
            } else {
                workspace_root.join(directory)
            };
            let lexical_candidate = lexical_absolute(&candidate)?;
            if !lexical_candidate.starts_with(&lexical_root)
                && !lexical_candidate.starts_with(&canonical_root)
            {
                continue;
            }
            let candidate = lexical_candidate;
            if !candidate.exists() {
                if missing_path_resolves_outside(&candidate, &lexical_root, &canonical_root)? {
                    continue;
                }
                bail!(
                    "Go workspace module {} is missing {GO_MOD}",
                    candidate.display()
                );
            }
            let canonical_module = candidate.canonicalize().wrap_err_with(|| {
                format!(
                    "failed to resolve Go workspace module {}",
                    candidate.display()
                )
            })?;
            let Ok(relative) = canonical_module.strip_prefix(&canonical_root) else {
                continue;
            };
            if !canonical_module.join(GO_MOD).is_file() {
                bail!(
                    "Go workspace module {} is missing {GO_MOD}",
                    candidate.display()
                );
            }
            let relative = normalize_relative(relative);
            let module_path = read_module_path(&canonical_module.join(GO_MOD))?;
            let id = ProjectId::new(self.id(), &module_path)?;
            let source = relative.join(GO_MOD);
            let provenance = WorkspaceProvenance {
                provider: Some(self.id().to_string()),
                source: Some(source),
            };
            let mut project = WorkspaceProject::new(id, relative);
            project.provenance = provenance;
            project
                .metadata
                .insert("workspace_source".to_string(), GO_MOD.to_string());
            modules.insert(canonical_module, project);
        }

        Ok(modules.into_values().collect())
    }
}

fn read_workfile(path: &Path) -> Result<Vec<PathBuf>> {
    let contents = fs::read_to_string(path)
        .wrap_err_with(|| format!("failed to read Go workspace metadata {}", path.display()))?;
    parse_workfile(&contents)
        .wrap_err_with(|| format!("failed to parse Go workspace metadata {}", path.display()))
}

fn parse_workfile(contents: &str) -> Result<Vec<PathBuf>> {
    let mut directories = Vec::new();
    let mut in_use_block = false;

    for (index, raw_line) in contents.lines().enumerate() {
        let line_number = index + 1;
        let line = strip_comment(raw_line).trim();
        if line.is_empty() {
            continue;
        }
        if in_use_block {
            if line == ")" {
                in_use_block = false;
                continue;
            }
            directories.push(PathBuf::from(parse_argument(line).wrap_err_with(|| {
                format!("invalid use directive on line {line_number}")
            })?));
            continue;
        }

        let Some(arguments) = directive_arguments(line, "use") else {
            continue;
        };
        if arguments == "(" {
            in_use_block = true;
        } else {
            directories.push(PathBuf::from(parse_argument(arguments).wrap_err_with(
                || format!("invalid use directive on line {line_number}"),
            )?));
        }
    }

    if in_use_block {
        bail!("unterminated use block");
    }
    Ok(directories)
}

fn read_module_path(path: &Path) -> Result<String> {
    let contents = fs::read_to_string(path)
        .wrap_err_with(|| format!("failed to read Go module metadata {}", path.display()))?;
    let mut module_path = None;

    for (index, raw_line) in contents.lines().enumerate() {
        let line = strip_comment(raw_line).trim();
        let Some(arguments) = directive_arguments(line, "module") else {
            continue;
        };
        let parsed = parse_argument(arguments)
            .wrap_err_with(|| format!("invalid module directive on line {}", index + 1))?;
        if module_path.replace(parsed).is_some() {
            bail!("Go module metadata contains multiple module directives");
        }
    }

    module_path.ok_or_else(|| eyre::eyre!("Go module metadata is missing a module directive"))
}

fn directive_arguments<'a>(line: &'a str, directive: &str) -> Option<&'a str> {
    let rest = line.strip_prefix(directive)?;
    if !rest.starts_with(char::is_whitespace) {
        return None;
    }
    Some(rest.trim())
}

fn parse_argument(value: &str) -> Result<String> {
    if value.is_empty() {
        bail!("directive requires one argument");
    }
    if let Some(value) = value.strip_prefix('"') {
        let Some(value) = value.strip_suffix('"') else {
            bail!("unterminated interpreted Go string");
        };
        return unescape_go_string(value);
    }
    if let Some(value) = value.strip_prefix('`') {
        let Some(value) = value.strip_suffix('`') else {
            bail!("unterminated raw Go string");
        };
        if value.contains('`') {
            bail!("unexpected raw Go string delimiter");
        }
        return Ok(value.to_string());
    }
    if value.split_whitespace().count() != 1 {
        bail!("directive requires exactly one argument");
    }
    Ok(value.to_string())
}

fn unescape_go_string(value: &str) -> Result<String> {
    let mut characters = value.chars();
    let mut unescaped = String::new();
    while let Some(character) = characters.next() {
        if character == '"' {
            bail!("unexpected interpreted Go string delimiter");
        }
        if character != '\\' {
            unescaped.push(character);
            continue;
        }
        let escape = characters
            .next()
            .ok_or_else(|| eyre::eyre!("unterminated Go string escape"))?;
        let escaped = match escape {
            'a' => '\u{0007}',
            'b' => '\u{0008}',
            'f' => '\u{000c}',
            'n' => '\n',
            'r' => '\r',
            't' => '\t',
            'v' => '\u{000b}',
            '\\' => '\\',
            '\'' => '\'',
            '"' => '"',
            'x' => escape_character(&mut characters, 2, 16)?,
            'u' => escape_character(&mut characters, 4, 16)?,
            'U' => escape_character(&mut characters, 8, 16)?,
            digit @ '0'..='7' => {
                let mut digits = digit.to_string();
                for _ in 0..2 {
                    digits.push(
                        characters
                            .next()
                            .ok_or_else(|| eyre::eyre!("incomplete octal Go string escape"))?,
                    );
                }
                decoded_character(&digits, 8)?
            }
            _ => bail!("unsupported Go string escape \\{escape}"),
        };
        unescaped.push(escaped);
    }
    Ok(unescaped)
}

fn escape_character(
    characters: &mut std::str::Chars<'_>,
    length: usize,
    radix: u32,
) -> Result<char> {
    let digits = characters.take(length).collect::<String>();
    if digits.chars().count() != length {
        bail!("incomplete Go string escape");
    }
    decoded_character(&digits, radix)
}

fn decoded_character(digits: &str, radix: u32) -> Result<char> {
    let value = u32::from_str_radix(digits, radix)
        .wrap_err_with(|| format!("invalid Go string escape {digits:?}"))?;
    char::from_u32(value).ok_or_else(|| eyre::eyre!("invalid Go character escape {digits:?}"))
}

fn strip_comment(line: &str) -> &str {
    let mut quote = None;
    let mut escaped = false;
    for (index, character) in line.char_indices() {
        match quote {
            Some('"') if escaped => escaped = false,
            Some('"') if character == '\\' => escaped = true,
            Some(delimiter) if character == delimiter => quote = None,
            Some(_) => {}
            None if matches!(character, '"' | '`') => quote = Some(character),
            None if character == '/' && line[index..].starts_with("//") => {
                return &line[..index];
            }
            None => {}
        }
    }
    line
}

fn normalize_relative(path: &Path) -> PathBuf {
    if path.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        path.to_path_buf()
    }
}

fn lexical_absolute(path: &Path) -> Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .wrap_err("failed to resolve the current directory")?
            .join(path)
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if normalized.parent().is_some() {
                    normalized.pop();
                }
            }
            component => normalized.push(component.as_os_str()),
        }
    }
    Ok(normalized)
}

fn missing_path_resolves_outside(
    candidate: &Path,
    lexical_root: &Path,
    canonical_root: &Path,
) -> Result<bool> {
    let mut candidate = candidate.to_path_buf();
    let mut followed = BTreeSet::new();
    loop {
        if !candidate.starts_with(lexical_root) && !candidate.starts_with(canonical_root) {
            return Ok(true);
        }
        let mut next = None;
        for ancestor in candidate.ancestors() {
            if !ancestor.starts_with(lexical_root) && !ancestor.starts_with(canonical_root) {
                break;
            }
            if let Ok(target) = std::fs::read_link(ancestor) {
                if !followed.insert(ancestor.to_path_buf()) {
                    return Ok(false);
                }
                let target = if target.is_absolute() {
                    target
                } else {
                    ancestor.parent().unwrap_or(lexical_root).join(target)
                };
                let suffix = candidate.strip_prefix(ancestor)?;
                next = Some(lexical_absolute(&target.join(suffix))?);
                break;
            }
            if ancestor == lexical_root || ancestor == canonical_root {
                break;
            }
        }
        match next {
            Some(next) => candidate = next,
            None => return Ok(false),
        }
    }
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

    fn module(path: &Path, module_path: &str) {
        write(
            &path.join(GO_MOD),
            &format!("module {module_path}\n\ngo 1.25\n"),
        );
    }

    #[test]
    fn discovers_single_and_block_use_directives() {
        let temp = tempdir().unwrap();
        write(
            &temp.path().join(GO_WORK),
            "go 1.25\nuse ./cmd/api // app\nuse (\n ./lib\n \"./tools/code\\x20gen\"\n)\n",
        );
        module(&temp.path().join("cmd/api"), "example.com/api");
        module(&temp.path().join("lib"), "example.com/lib");
        module(&temp.path().join("tools/code gen"), "example.com/codegen");

        let projects = GoWorkspaceProvider.discover(temp.path()).unwrap();
        let summary = projects
            .iter()
            .map(|project| (project.id.as_str(), project.root.as_path()))
            .collect::<Vec<_>>();

        assert_eq!(
            summary,
            vec![
                ("go:example.com/api", Path::new("cmd/api")),
                ("go:example.com/lib", Path::new("lib")),
                ("go:example.com/codegen", Path::new("tools/code gen")),
            ]
        );
        assert!(
            projects
                .iter()
                .all(|project| project.dependencies.is_empty())
        );
    }

    #[test]
    fn supports_root_modules_and_quoted_module_paths() {
        let temp = tempdir().unwrap();
        write(&temp.path().join(GO_WORK), "go 1.25\nuse .\n");
        write(
            &temp.path().join(GO_MOD),
            "module \"example.com/root\" // root module\n",
        );

        let projects = GoWorkspaceProvider.discover(temp.path()).unwrap();

        assert_eq!(projects[0].id.as_str(), "go:example.com/root");
        assert_eq!(projects[0].root, Path::new("."));
        assert_eq!(projects[0].provenance.source, Some("./go.mod".into()));
    }

    #[test]
    fn interpreted_strings_require_three_digit_octal_escapes() {
        assert_eq!(unescape_go_string(r"code\040gen").unwrap(), "code gen");
        assert!(unescape_go_string(r"code\0gen").is_err());
        assert!(unescape_go_string(r"code\04gen").is_err());
    }

    #[test]
    fn cleans_use_paths_before_filesystem_access() {
        let temp = tempdir().unwrap();
        write(
            &temp.path().join(GO_WORK),
            "go 1.25\nuse ./missing/../app\n",
        );
        module(&temp.path().join("app"), "example.com/app");

        let projects = GoWorkspaceProvider.discover(temp.path()).unwrap();

        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].id.as_str(), "go:example.com/app");
    }

    #[test]
    fn skips_modules_outside_the_monorepo_root() {
        let temp = tempdir().unwrap();
        let workspace = temp.path().join("workspace");
        let external = temp.path().join("external");
        write(
            &workspace.join(GO_WORK),
            "go 1.25\nuse ./app\nuse ../external\nuse ../not-a-module\nuse ../missing\n",
        );
        module(&workspace.join("app"), "example.com/app");
        module(&external, "example.com/external");
        fs::create_dir_all(temp.path().join("not-a-module")).unwrap();

        let projects = GoWorkspaceProvider.discover(&workspace).unwrap();

        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].id.as_str(), "go:example.com/app");
    }

    #[test]
    fn reports_invalid_workspace_modules() {
        let temp = tempdir().unwrap();
        write(&temp.path().join(GO_WORK), "go 1.25\nuse ./missing\n");

        assert!(
            GoWorkspaceProvider
                .discover(temp.path())
                .unwrap_err()
                .to_string()
                .contains("missing go.mod")
        );

        write(&temp.path().join(GO_WORK), "go 1.25\nuse (\n");
        let error = GoWorkspaceProvider.discover(temp.path()).unwrap_err();
        assert!(format!("{error:#}").contains("unterminated use block"));
    }

    #[cfg(unix)]
    #[test]
    fn skips_missing_paths_through_external_symlinks() {
        let temp = tempdir().unwrap();
        let workspace = temp.path().join("workspace");
        let external = temp.path().join("external");
        fs::create_dir_all(&external).unwrap();
        write(
            &workspace.join(GO_WORK),
            "go 1.25\nuse ./app\nuse ./dangling-link\nuse ./external-link/missing\nuse ./chained-link/missing\n",
        );
        module(&workspace.join("app"), "example.com/app");
        std::os::unix::fs::symlink(external.join("missing"), workspace.join("dangling-link"))
            .unwrap();
        std::os::unix::fs::symlink(&external, workspace.join("external-link")).unwrap();
        std::os::unix::fs::symlink("external-link", workspace.join("chained-link")).unwrap();

        let projects = GoWorkspaceProvider.discover(&workspace).unwrap();

        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].id.as_str(), "go:example.com/app");
    }

    #[test]
    fn ignores_roots_without_a_workfile() {
        let temp = tempdir().unwrap();
        module(temp.path(), "example.com/standalone");

        assert!(
            GoWorkspaceProvider
                .discover(temp.path())
                .unwrap()
                .is_empty()
        );
    }
}
