//! What a tool installed from a packslip declares beyond its executables.
//!
//! The backend keeps the verified statement beside each install. This
//! module reads it back and turns the `resources` it lists into things
//! mise can hand a shell: a completion script for whichever version of the
//! tool is active, from the most verifiable source the vendor offered.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use eyre::{Result, WrapErr, bail, eyre};
use packslip::model::{Resource, ResourceSource, Statement};
use reqwest::header::HeaderMap;

use crate::backend::Backend;
use crate::backend::packslip::{STATEMENT_FILE, is_safe_relative, locate_in_install};
use crate::cmd::CmdLineRunner;
use crate::config::{Config, Settings};
use crate::file;
use crate::github;
use crate::http::HTTP;
use crate::toolset::{ToolVersion, Toolset};
use crate::ui::progress_report::SingleReport;

/// Resources fetched from outside the artifact live here in the install.
pub(crate) const RESOURCES_DIR: &str = ".mise-packslip";

/// The statement kept beside an install, if the tool came from a packslip.
pub(crate) fn statement(install_path: &Path) -> Result<Option<Statement>> {
    let path = install_path.join(STATEMENT_FILE);
    if !path.is_file() {
        return Ok(None);
    }
    let text = file::read_to_string(&path)?;
    let statement: Statement =
        serde_json::from_str(&text).wrap_err_with(|| format!("reading {}", path.display()))?;
    statement
        .validate()
        .wrap_err_with(|| format!("{} is not a valid packslip statement", path.display()))?;
    Ok(Some(statement))
}

/// Where a resource's file is inside the install, if it is there: in the
/// unpacked artifact, or where [`fetch_files`] put it.
pub(crate) fn resource_path(install_path: &Path, resource: &Resource) -> Option<PathBuf> {
    let fetched = |sub: &str, rel: &str| {
        Some(install_path.join(RESOURCES_DIR).join(sub).join(rel)).filter(|p| p.is_file())
    };
    match resource.source()? {
        ResourceSource::Archive => locate_in_install(install_path, resource.archive.as_deref()?),
        ResourceSource::Asset => fetched("assets", asset_name(resource)?),
        ResourceSource::Repo => fetched("repo", repo_path(resource)?),
        ResourceSource::Exec => None,
    }
}

/// The asset an entry names, if it is a plain file name. A verified
/// statement is still the vendor's data: nothing in it may name a path
/// outside the install.
fn asset_name(resource: &Resource) -> Option<&str> {
    resource
        .asset
        .as_deref()
        .filter(|name| file::is_plain_file_name(name))
}

/// The repository path an entry names, if it is safe to join.
fn repo_path(resource: &Resource) -> Option<&str> {
    resource.repo.as_deref().filter(|rel| is_safe_relative(rel))
}

/// The raw-file URL of a repository path at the release's commit, for the
/// forges mise knows how to read.
pub(crate) fn raw_file_url(statement: &Statement, rel: &str) -> Option<String> {
    let source = statement.predicate.source.as_ref()?;
    let commit = source.commit.as_deref()?;
    let repo = source.repo.trim_end_matches('/').trim_end_matches(".git");
    if let Some(path) = repo.strip_prefix("https://github.com/") {
        Some(format!(
            "https://raw.githubusercontent.com/{path}/{commit}/{rel}"
        ))
    } else {
        repo.strip_prefix("https://gitlab.com/")
            .map(|path| format!("https://gitlab.com/{path}/-/raw/{commit}/{rel}"))
    }
}

fn headers_for(url: &str) -> Result<HeaderMap> {
    if url.starts_with("https://github.com/")
        || url.starts_with("https://raw.githubusercontent.com/")
    {
        github::get_headers(url)
    } else {
        Ok(HeaderMap::new())
    }
}

/// Fetch the files the statement sources from separate release assets and
/// from the source repository, so they are on disk before a shell asks for
/// one. An asset must match the digest the statement signed; a repository
/// file is pinned by the commit it is fetched at. Skills are directories
/// and are not fetched here.
pub(crate) async fn fetch_files(
    tv: &ToolVersion,
    statement: &Statement,
    pr: &dyn SingleReport,
) -> Result<()> {
    let base = tv.install_path().join(RESOURCES_DIR);
    for resource in &statement.predicate.resources {
        match resource.source() {
            Some(ResourceSource::Asset) => {
                let Some(name) = asset_name(resource) else {
                    warn!(
                        "{}: the packslip names an asset {:?}, which is not a plain file name",
                        tv.style(),
                        resource.asset.as_deref().unwrap_or_default()
                    );
                    continue;
                };
                let dest = base.join("assets").join(name);
                if dest.exists() {
                    continue;
                }
                let Some(url) = &resource.url else {
                    warn!("{}: asset {name} has no download URL", tv.style());
                    continue;
                };
                pr.set_message(format!("download {name}"));
                file::create_dir_all(dest.parent().unwrap_or(&base))?;
                HTTP.download_file_with_headers(url, &dest, &headers_for(url)?, Some(pr))
                    .await?;
                let (actual, _) = packslip::digest_file(&dest)?;
                let expected = statement.digest_of(name);
                if expected != Some(actual.as_str()) {
                    let _ = file::remove_all(&dest);
                    bail!(
                        "{name}: sha256 is {actual}, the packslip says {}",
                        expected.unwrap_or("it is not a subject")
                    );
                }
            }
            Some(ResourceSource::Repo) if resource.kind != "skill" => {
                let Some(rel) = repo_path(resource) else {
                    warn!(
                        "{}: the packslip names a repository path {:?}, which is not safe to fetch",
                        tv.style(),
                        resource.repo.as_deref().unwrap_or_default()
                    );
                    continue;
                };
                let dest = base.join("repo").join(rel);
                if dest.exists() {
                    continue;
                }
                let Some(url) = raw_file_url(statement, rel) else {
                    warn!(
                        "{}: {rel} comes from the source repository, which mise cannot read files from",
                        tv.style()
                    );
                    continue;
                };
                pr.set_message(format!("download {rel}"));
                file::create_dir_all(dest.parent().unwrap_or(&base))?;
                if let Err(err) = HTTP
                    .download_file_with_headers(&url, &dest, &headers_for(&url)?, Some(pr))
                    .await
                {
                    warn!(
                        "{}: could not fetch {rel} from the source repository: {err}",
                        tv.style()
                    );
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// Where a completion for one shell can come from, most verifiable first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CompletionSource {
    /// A script the vendor shipped, on disk.
    File(PathBuf),
    /// A CLI spec on disk to derive the script from.
    Spec {
        format: String,
        bin: String,
        path: PathBuf,
    },
    /// A command of the tool's that prints the script.
    Exec(Vec<String>),
    /// A command of the tool's that prints a CLI spec to derive from.
    SpecExec {
        format: String,
        bin: String,
        argv: Vec<String>,
    },
}

/// Every way the statement offers a `shell` completion, in the order the
/// specification says a consumer takes them: shipped scripts, then a
/// script derived from a CLI spec, then anything that runs the tool.
pub(crate) fn completion_sources(
    statement: &Statement,
    install_path: &Path,
    shell: &str,
) -> Vec<CompletionSource> {
    let resources = &statement.predicate.resources;
    let completions = || resources.iter().filter(|r| r.kind == "completion");
    let for_shell =
        |r: &Resource| r.shell.as_deref() == Some(shell) || r.shells.iter().any(|s| s == shell);
    let mut sources = Vec::new();
    for rank in [
        ResourceSource::Archive,
        ResourceSource::Asset,
        ResourceSource::Repo,
    ] {
        for r in completions().filter(|r| r.source() == Some(rank) && for_shell(r)) {
            if let Some(path) = resource_path(install_path, r) {
                sources.push(CompletionSource::File(path));
            }
        }
    }
    let specs = || {
        resources
            .iter()
            .filter(|r| r.kind == "cli-spec")
            .filter_map(|r| Some((r, r.format.clone()?, r.bin.clone()?)))
    };
    for (r, format, bin) in specs().filter(|(r, ..)| r.source() != Some(ResourceSource::Exec)) {
        if let Some(path) = resource_path(install_path, r) {
            sources.push(CompletionSource::Spec { format, bin, path });
        }
    }
    let substitute = |argv: &[String]| -> Vec<String> {
        argv.iter().map(|a| a.replace("{shell}", shell)).collect()
    };
    for r in completions().filter(|r| r.source() == Some(ResourceSource::Exec) && for_shell(r)) {
        sources.push(CompletionSource::Exec(substitute(&r.exec)));
    }
    for (r, format, bin) in specs().filter(|(r, ..)| r.source() == Some(ResourceSource::Exec)) {
        sources.push(CompletionSource::SpecExec {
            format,
            bin,
            argv: r.exec.clone(),
        });
    }
    sources
}

/// The active, installed tool called `name`, or the one providing an
/// executable called `name`.
async fn find_tool(
    config: &Arc<Config>,
    ts: &Toolset,
    name: &str,
) -> Result<(Arc<dyn Backend>, ToolVersion)> {
    if let Some(found) = ts.which(config, name).await {
        return Ok(found);
    }
    let by_name = ts
        .list_current_installed_versions(config)
        .into_iter()
        .find(|(b, _)| b.ba().short == name || b.tool_name() == name || b.id() == name);
    match by_name {
        Some(found) => Ok(found),
        None => bail!("{name} is not an active, installed tool or one of their executables"),
    }
}

/// Run one of the tool's own executables and return what it printed.
async fn run_tool(
    config: &Arc<Config>,
    backend: &Arc<dyn Backend>,
    tv: &ToolVersion,
    argv: &[String],
) -> Result<String> {
    let Some((program, args)) = argv.split_first() else {
        bail!("an exec entry with no command");
    };
    let Some(path) = backend.which(config, tv, program).await? else {
        bail!("{} has no executable called {program}", tv.style());
    };
    CmdLineRunner::new(path).args(args).read().await
}

/// Derive a completion script from a CLI spec with the consumer's own
/// tooling. Only the `usage` format is known.
async fn derive_from_spec(
    config: &Arc<Config>,
    ts: &Toolset,
    format: &str,
    bin: &str,
    spec: &Path,
    shell: &str,
) -> Result<String> {
    if format != "usage" {
        bail!("mise cannot derive completions from a {format} spec");
    }
    let usage = match ts.which_bin(config, "usage").await {
        Some(path) => path,
        None => file::which("usage").ok_or_else(|| {
            eyre!(
                "deriving a completion from the usage spec needs the `usage` command; install it with `mise use -g usage`"
            )
        })?,
    };
    CmdLineRunner::new(usage)
        .args(["generate", "completion", shell, bin, "--file"])
        .arg(spec)
        .read()
        .await
}

/// The `shell` completion script for `tool`, from the packslip of the
/// version that is active right now.
pub(crate) async fn completion_script(
    config: &Arc<Config>,
    tool: &str,
    shell: &str,
) -> Result<String> {
    let ts = config.get_toolset().await?;
    let (backend, tv) = find_tool(config, ts, tool).await?;
    let install_path = tv.install_path();
    let Some(statement) = statement(&install_path)? else {
        bail!(
            "{} was not installed from a packslip, so mise does not know its completions",
            tv.style()
        );
    };
    let sources = completion_sources(&statement, &install_path, shell);
    if sources.is_empty() {
        bail!(
            "the packslip of {} declares no {shell} completion",
            tv.style()
        );
    }
    let allow_exec = Settings::get().packslip.exec;
    let refused = |argv: &[String]| {
        format!(
            "`{}` would generate it, but running a tool at completion time is off; set `packslip.exec = true` to allow it",
            argv.join(" ")
        )
    };
    let mut skipped = Vec::new();
    for source in sources {
        let attempt = match source {
            CompletionSource::File(path) => file::read_to_string(&path),
            CompletionSource::Spec { format, bin, path } => {
                derive_from_spec(config, ts, &format, &bin, &path, shell).await
            }
            CompletionSource::Exec(argv) => {
                if !allow_exec {
                    skipped.push(refused(&argv));
                    continue;
                }
                run_tool(config, &backend, &tv, &argv).await
            }
            CompletionSource::SpecExec { format, bin, argv } => {
                if !allow_exec {
                    skipped.push(refused(&argv));
                    continue;
                }
                let spec = run_tool(config, &backend, &tv, &argv).await?;
                let dir = tempfile::tempdir()?;
                let path = dir.path().join(format!("{bin}.{format}.kdl"));
                file::write(&path, &spec)?;
                derive_from_spec(config, ts, &format, &bin, &path, shell).await
            }
        };
        match attempt {
            Ok(script) => return Ok(script),
            Err(err) => skipped.push(err.to_string()),
        }
    }
    bail!(
        "no usable {shell} completion for {}: {}",
        tv.style(),
        skipped.join("; ")
    )
}

/// A stub the shell loads by name, which asks mise for the real script at
/// completion time, so it follows whichever version of the tool is active.
/// It carries the marker usage's installer looks for, so re-installing
/// replaces it rather than refusing a foreign file.
///
/// In zsh and bash the vendor's script replaces the stub while it completes
/// and the stub is put back afterwards, so the next completion asks mise
/// again and a version switch in another directory is followed on the next
/// tab. fish and PowerShell load the script once per shell session.
pub(crate) fn stub(tool: &str, shell: usage_rs::complete::Shell) -> Result<String> {
    use usage_rs::complete::Shell;
    let note = format!("mise completes {tool} from the packslip of whichever version is active");
    let by = format!(
        "@generated by usage's installer for `mise completion {} --tool {tool} --install`",
        shell.as_str()
    );
    let stub = match shell {
        Shell::Zsh => format!(
            r#"#compdef {tool}
# {note}.
# {by}
# The vendor's script takes over this function while it completes; the stub
# is put back afterwards, so the next completion asks mise again.
local __mise_stub="${{functions[_{tool}]}}"
eval "$(command mise completion zsh --tool '{tool}' 2>/dev/null)"
local __mise_fn="${{_comps[{tool}]:-_{tool}}}"
local __mise_ret=0
if [[ "$__mise_fn" != _{tool} || "${{functions[_{tool}]}}" != "$__mise_stub" ]]; then
  "$__mise_fn" "$@"
  __mise_ret=$?
fi
functions[_{tool}]="$__mise_stub"
compdef _{tool} '{tool}'
return $__mise_ret
"#
        ),
        Shell::Bash => {
            let func = format!(
                "__mise_complete_{}",
                tool.replace(|c: char| !c.is_ascii_alphanumeric(), "_")
            );
            format!(
                r#"# {note}.
# {by}
# The vendor's script registers its own completer; this one is put back after
# each completion, so the next asks mise again.
{func}() {{
  eval "$(command mise completion bash --tool '{tool}' 2>/dev/null)"
  local __mise_spec __mise_fn __mise_ret=0
  __mise_spec=$(complete -p '{tool}' 2>/dev/null)
  case " $__mise_spec " in
    *" -F "*)
      __mise_fn=${{__mise_spec##*-F }}
      __mise_fn=${{__mise_fn%% *}}
      if [[ $__mise_fn != {func} ]]; then
        "$__mise_fn" "$@"
        __mise_ret=$?
      fi
      complete -F {func} '{tool}'
      return $__mise_ret
      ;;
  esac
  # The vendor registered something other than a function (complete -C, -W,
  # ...). Leave it in place and have bash retry the completion with it; the
  # stub is gone for this session, so a version switch waits for a new shell.
  [[ -n $__mise_spec && $__mise_spec != *{func}* ]] && return 124
  complete -F {func} '{tool}'
  return 0
}}
complete -F {func} '{tool}'
"#
            )
        }
        Shell::Fish => format!(
            "# {note}.\n# {by}\ncommand mise completion fish --tool '{tool}' 2>/dev/null | source\n"
        ),
        Shell::PowerShell => format!(
            "# {note}.\n# {by}\n& mise completion powershell --tool '{tool}' 2>$null | Out-String | Invoke-Expression\n"
        ),
        _ => bail!(
            "{} loads completions eagerly, so mise cannot leave it a stub; redirect `mise completion {} --tool {tool}` yourself",
            shell.as_str(),
            shell.as_str()
        ),
    };
    Ok(stub)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn statement_with(resources: &str) -> Statement {
        let json = format!(
            r#"{{"_type":"https://in-toto.io/Statement/v1","subject":[{{"name":"t-linux-x64.tar.xz","digest":{{"sha256":"{a}"}}}},{{"name":"t-skill.tar.gz","digest":{{"sha256":"{b}"}}}}],"predicateType":"https://packslip.dev/release/v1","predicate":{{"project":"github.com/o/r","version":"1","published_at":"2026-09-01T00:00:00Z","source":{{"repo":"https://github.com/o/r","commit":"{c}"}},"artifacts":[{{"name":"t-linux-x64.tar.xz","os":"linux","arch":"x86_64","libc":"gnu","size":5,"format":"tar.xz","bin":["t"]}}],"resources":{resources},"identity":{{"scheme":"sigstore-oidc","key_id":"https://github.com/o/r/.github/workflows/r.yml@refs/tags/v1","issuer":"https://token.actions.githubusercontent.com"}}}}}}"#,
            a = "a".repeat(64),
            b = "b".repeat(64),
            c = "c".repeat(40),
        );
        let statement: Statement = serde_json::from_str(&json).unwrap();
        statement.validate().unwrap();
        statement
    }

    /// A statement whose second subject is accounted for by a skill asset.
    fn basic() -> Statement {
        statement_with(r#"[{"kind":"skill","name":"t","asset":"t-skill.tar.gz"}]"#)
    }

    #[test]
    fn statement_is_read_back_and_validated() {
        let dir = tempfile::tempdir().unwrap();
        assert!(statement(dir.path()).unwrap().is_none());
        let s = basic();
        file::write(
            dir.path().join(STATEMENT_FILE),
            serde_json::to_string(&s).unwrap(),
        )
        .unwrap();
        assert_eq!(statement(dir.path()).unwrap(), Some(s));
        file::write(dir.path().join(STATEMENT_FILE), "{}").unwrap();
        assert!(statement(dir.path()).is_err());
    }

    #[test]
    fn raw_file_urls_pin_the_commit() {
        let s = basic();
        assert_eq!(
            raw_file_url(&s, "completions/t.fish").unwrap(),
            format!(
                "https://raw.githubusercontent.com/o/r/{}/completions/t.fish",
                "c".repeat(40)
            )
        );
        let mut gitlab = s.clone();
        gitlab.predicate.source.as_mut().unwrap().repo = "https://gitlab.com/g/p.git".into();
        assert_eq!(
            raw_file_url(&gitlab, "x").unwrap(),
            format!("https://gitlab.com/g/p/-/raw/{}/x", "c".repeat(40))
        );
        let mut other = s.clone();
        other.predicate.source.as_mut().unwrap().repo = "https://example.com/r".into();
        assert_eq!(raw_file_url(&other, "x"), None);
        let mut no_commit = s;
        no_commit.predicate.source.as_mut().unwrap().commit = None;
        assert_eq!(raw_file_url(&no_commit, "x"), None);
    }

    #[test]
    fn vendor_paths_never_leave_the_install() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let outside = root.join("outside");
        std::fs::write(&outside, "").unwrap();
        let mut s = statement_with(
            r#"[{"kind":"completion","shell":"zsh","asset":"t-skill.tar.gz"},
                {"kind":"man","repo":"man/t.1"}]"#,
        );
        // Tamper after validation, as a hostile file on disk could.
        s.predicate.resources[0].asset = Some("../outside".into());
        s.predicate.resources[1].repo = Some("/etc/passwd".into());
        for r in &s.predicate.resources {
            assert_eq!(resource_path(root, r), None, "{r:?}");
        }
        assert!(outside.exists());
    }

    #[test]
    fn completion_sources_follow_the_spec_order() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("share")).unwrap();
        std::fs::write(root.join("share/_t"), "#compdef t").unwrap();
        std::fs::create_dir_all(root.join(RESOURCES_DIR).join("repo/completions")).unwrap();
        std::fs::write(root.join(RESOURCES_DIR).join("repo/completions/t.zsh"), "").unwrap();
        std::fs::write(root.join("t.kdl"), "").unwrap();
        let s = statement_with(
            r#"[
            {"kind":"completion","shell":"zsh","exec":["t","completion","zsh"]},
            {"kind":"completion","shells":["bash","zsh"],"exec":["t","completions","{shell}"]},
            {"kind":"completion","shell":"zsh","repo":"completions/t.zsh"},
            {"kind":"completion","shell":"zsh","asset":"t-skill.tar.gz"},
            {"kind":"cli-spec","format":"usage","bin":"t","exec":["t","usage"]},
            {"kind":"cli-spec","format":"usage","bin":"t","archive":"t.kdl"},
            {"kind":"completion","shell":"fish","archive":"share/t.fish"},
            {"kind":"completion","shell":"zsh","archive":"share/_t"}
        ]"#,
        );
        let sources = completion_sources(&s, root, "zsh");
        assert_eq!(
            sources,
            vec![
                CompletionSource::File(root.join("share/_t")),
                CompletionSource::File(root.join(RESOURCES_DIR).join("repo/completions/t.zsh")),
                CompletionSource::Spec {
                    format: "usage".into(),
                    bin: "t".into(),
                    path: root.join("t.kdl"),
                },
                CompletionSource::Exec(vec!["t".into(), "completion".into(), "zsh".into()]),
                CompletionSource::Exec(vec!["t".into(), "completions".into(), "zsh".into()]),
                CompletionSource::SpecExec {
                    format: "usage".into(),
                    bin: "t".into(),
                    argv: vec!["t".into(), "usage".into()],
                },
            ],
            "shipped files first, an unfetched asset skipped, then the spec, then anything that runs the tool"
        );
        let fish = completion_sources(&s, root, "fish");
        assert!(
            matches!(fish.first(), Some(CompletionSource::Spec { .. })),
            "the fish file is not in the archive, so the spec comes first: {fish:?}"
        );
        assert!(
            completion_sources(&s, root, "nu")
                .iter()
                .all(|c| !matches!(c, CompletionSource::File(_) | CompletionSource::Exec(_)))
        );
    }

    #[test]
    fn stubs_carry_the_installer_marker_and_defer_to_mise() {
        use usage_rs::complete::Shell;
        for shell in [Shell::Zsh, Shell::Bash, Shell::Fish, Shell::PowerShell] {
            let stub = stub("rg", shell).unwrap();
            assert!(stub.contains("@generated by usage"), "{stub}");
            assert!(
                stub.contains(&format!("mise completion {} --tool 'rg'", shell.as_str())),
                "{stub}"
            );
        }
        let zsh = stub("rg", Shell::Zsh).unwrap();
        assert!(zsh.starts_with("#compdef rg\n"), "{zsh}");
        assert!(
            zsh.contains("compdef _rg 'rg'"),
            "put back after completing: {zsh}"
        );
        let bash = stub("cargo-nextest", Shell::Bash).unwrap();
        assert!(
            bash.contains("complete -F __mise_complete_cargo_nextest 'cargo-nextest'"),
            "{bash}"
        );
        assert!(
            bash.contains("return 124"),
            "non-function registrations: {bash}"
        );
        assert!(stub("rg", Shell::Nu).is_err());
    }
}
