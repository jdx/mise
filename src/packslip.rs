//! What a tool installed from a packslip declares beyond its executables.
//!
//! The backend keeps the verified statement beside each install. This
//! module reads it back and turns the `resources` it lists into things
//! mise can hand a shell: a completion script for whichever version of the
//! tool is active, from the most verifiable source the vendor offered.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use eyre::{Result, WrapErr, bail, eyre};
use packslip::model::{Artifact, Resource, ResourceSource, Statement, resource_fits};
use reqwest::header::{HeaderMap, HeaderValue};

use crate::backend::Backend;
use crate::backend::packslip::{
    STATEMENT_FILE, is_safe_relative, locate_in_install, selected_artifact,
};
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

/// Where to fetch a repository file at the release's commit, and with what
/// headers, for the forges mise knows how to read. GitHub goes through the
/// contents API, so a token applies to a private repository and a missing
/// file is an error rather than a login page; GitLab's raw URL serves
/// public repositories.
pub(crate) fn repo_file_request(statement: &Statement, rel: &str) -> Option<(String, HeaderMap)> {
    let source = statement.predicate.source.as_ref()?;
    let commit = source.commit.as_deref()?;
    let repo = source.repo.trim_end_matches('/').trim_end_matches(".git");
    let rel = url_path(rel);
    if let Some(path) = repo.strip_prefix("https://github.com/") {
        let url = format!("https://api.github.com/repos/{path}/contents/{rel}?ref={commit}");
        let mut headers = github::get_headers(&url).ok()?;
        headers.insert(
            reqwest::header::ACCEPT,
            HeaderValue::from_static("application/vnd.github.raw+json"),
        );
        Some((url, headers))
    } else {
        repo.strip_prefix("https://gitlab.com/").map(|path| {
            (
                format!("https://gitlab.com/{path}/-/raw/{commit}/{rel}"),
                HeaderMap::new(),
            )
        })
    }
}

/// A repository path as URL path segments: each segment percent-encoded,
/// so a `?` or `#` in a name cannot rewrite the query or fragment and
/// reach past the commit the URL pins.
pub(crate) fn url_path(rel: &str) -> String {
    rel.split('/')
        .map(|segment| urlencoding::encode(segment).into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

fn headers_for(url: &str) -> Result<HeaderMap> {
    if url.starts_with("https://github.com/")
        || url.starts_with("https://api.github.com/")
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
                let Some((url, headers)) = repo_file_request(statement, rel) else {
                    warn!(
                        "{}: {rel} comes from the source repository, which mise cannot read files from",
                        tv.style()
                    );
                    continue;
                };
                pr.set_message(format!("download {rel}"));
                file::create_dir_all(dest.parent().unwrap_or(&base))?;
                if let Err(err) = HTTP
                    .download_file_with_headers(&url, &dest, &headers, Some(pr))
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

/// The entries of one kind that apply to the selected artifact, keeping
/// only the most specific of them: a resource may carry `os`, `arch`, or
/// `libc` when layouts differ by platform, and the one naming the most of
/// those wins. With no artifact selected, only unscoped entries apply.
pub(crate) fn applicable<'a>(
    resources: impl Iterator<Item = &'a Resource>,
    artifact: Option<&Artifact>,
) -> Vec<&'a Resource> {
    let specificity = |r: &Resource| {
        [&r.os, &r.arch, &r.libc]
            .into_iter()
            .filter(|f| f.is_some())
            .count()
    };
    let fits: Vec<&Resource> = resources
        .filter(|r| match artifact {
            Some(artifact) => resource_fits(r, artifact),
            None => specificity(r) == 0,
        })
        .collect();
    let best = fits.iter().map(|r| specificity(r)).max().unwrap_or(0);
    fits.into_iter()
        .filter(|r| specificity(r) == best)
        .collect()
}

/// Every way the statement offers a `shell` completion, in the order the
/// specification says a consumer takes them: the entries that apply to
/// the selected artifact, then shipped scripts, then a script derived
/// from a CLI spec, then anything that runs the tool.
pub(crate) fn completion_sources(
    statement: &Statement,
    install_path: &Path,
    shell: &str,
    artifact: Option<&Artifact>,
) -> Vec<CompletionSource> {
    let resources = &statement.predicate.resources;
    let for_shell_all =
        |r: &Resource| r.shell.as_deref() == Some(shell) || r.shells.iter().any(|s| s == shell);
    let completion_entries = applicable(
        resources
            .iter()
            .filter(|r| r.kind == "completion" && for_shell_all(r)),
        artifact,
    );
    let spec_entries = applicable(resources.iter().filter(|r| r.kind == "cli-spec"), artifact);
    let completions = || completion_entries.iter().copied();
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
        spec_entries
            .iter()
            .copied()
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
    let artifact = selected_artifact(
        &statement,
        tv.request.options().get_string("variant").as_deref(),
    );
    let sources = completion_sources(&statement, &install_path, shell, artifact.as_ref());
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
                // Any failure here is one more reason to try the next source,
                // not the end of the search. The spec is kept in the install:
                // a script derived from it names the file at completion time,
                // so it has to outlive this command.
                async {
                    if !file::is_plain_file_name(&bin) || !file::is_plain_file_name(&format) {
                        bail!("cli-spec entry names {bin:?} in format {format:?}");
                    }
                    let spec = run_tool(config, &backend, &tv, &argv).await?;
                    let dir = install_path.join(RESOURCES_DIR).join("specs");
                    file::create_dir_all(&dir)?;
                    let path = dir.join(format!("{bin}.{format}"));
                    file::write(&path, &spec)?;
                    derive_from_spec(config, ts, &format, &bin, &path, shell).await
                }
                .await
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
local __mise_matches="${{compstate[nmatches]:-0}}"
eval "$(command mise completion zsh --tool '{tool}' 2>/dev/null)"
local __mise_fn="${{_comps[{tool}]:-_{tool}}}"
local __mise_ret=0
if [[ "${{compstate[nmatches]:-0}}" != "$__mise_matches" ]]; then
  # The script completed on its own, as one that checks funcstack does
  # when it finds itself inside _{tool}; calling it again would double
  # every candidate.
  :
elif [[ "$__mise_fn" != _{tool} || "${{functions[_{tool}]}}" != "$__mise_stub" ]]; then
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
# The vendor's script registers its own completer, which handles this
# completion; the stub is put back at the next prompt, so the next asks mise
# again.
{func}() {{
  eval "$(command mise completion bash --tool '{tool}' 2>/dev/null)"
  local __mise_spec
  __mise_spec=$(complete -p '{tool}' 2>/dev/null)
  if [[ -n $__mise_spec && $__mise_spec != *{func}* ]]; then
    # The vendor's registration is in place now, options and all. Hand this
    # completion to it: 124 makes bash retry with the current registration.
    # The stub comes back at the next prompt, so later completions ask mise
    # again and a version switch is followed.
    {func}_restub() {{
      # Runs first at the prompt, so this is the last command's status,
      # which a prompt that shows it must get back unchanged.
      local __mise_status=$?
      complete -F {func} '{tool}'
      if declare -p PROMPT_COMMAND 2>/dev/null | grep -q '^declare -a'; then
        local __mise_i
        for __mise_i in "${{!PROMPT_COMMAND[@]}}"; do
          [[ ${{PROMPT_COMMAND[__mise_i]}} == "{func}_restub" ]] && unset 'PROMPT_COMMAND[__mise_i]'
        done
      else
        PROMPT_COMMAND=${{PROMPT_COMMAND//{func}_restub;/}}
      fi
      return $__mise_status
    }}
    if declare -p PROMPT_COMMAND 2>/dev/null | grep -q '^declare -a'; then
      PROMPT_COMMAND=("{func}_restub" "${{PROMPT_COMMAND[@]}}")
    else
      PROMPT_COMMAND="{func}_restub;${{PROMPT_COMMAND:-}}"
    fi
    return 124
  fi
  # Nothing usable came back; stay registered and offer nothing this time.
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
            "# {note}.\n# {by}\n@(& mise completion powershell --tool '{tool}' 2>$null) -join \"`n\" | Invoke-Expression\n"
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
            r#"{{"_type":"https://in-toto.io/Statement/v1","subject":[{{"name":"t-linux-x64.tar.xz","digest":{{"sha256":"{a}"}}}},{{"name":"t-skill.tar.gz","digest":{{"sha256":"{b}"}}}}],"predicateType":"https://packslip.dev/release/v1","predicate":{{"project":"github.com/o/r","version":"1.0.0","published_at":"2026-09-01T00:00:00Z","source":{{"repo":"https://github.com/o/r","commit":"{c}"}},"artifacts":[{{"name":"t-linux-x64.tar.xz","os":"linux","arch":"x86_64","libc":"gnu","size":5,"format":"tar.xz","bin":["t"]}}],"resources":{resources},"identity":{{"scheme":"sigstore-oidc","key_id":"https://github.com/o/r/.github/workflows/r.yml@refs/tags/v1","issuer":"https://token.actions.githubusercontent.com"}}}}}}"#,
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
    fn repo_file_requests_pin_the_commit() {
        let s = basic();
        assert_eq!(
            repo_file_request(&s, "docs/a?b#c.md").unwrap().0,
            format!(
                "https://api.github.com/repos/o/r/contents/docs/a%3Fb%23c.md?ref={}",
                "c".repeat(40)
            ),
            "a name cannot rewrite the query or fragment"
        );
        let (url, headers) = repo_file_request(&s, "completions/t.fish").unwrap();
        assert_eq!(
            url,
            format!(
                "https://api.github.com/repos/o/r/contents/completions/t.fish?ref={}",
                "c".repeat(40)
            )
        );
        assert_eq!(
            headers.get(reqwest::header::ACCEPT).unwrap(),
            "application/vnd.github.raw+json"
        );
        let mut gitlab = s.clone();
        gitlab.predicate.source.as_mut().unwrap().repo = "https://gitlab.com/g/p.git".into();
        assert_eq!(
            repo_file_request(&gitlab, "x").unwrap().0,
            format!("https://gitlab.com/g/p/-/raw/{}/x", "c".repeat(40))
        );
        let mut other = s.clone();
        other.predicate.source.as_mut().unwrap().repo = "https://example.com/r".into();
        assert!(repo_file_request(&other, "x").is_none());
        let mut no_commit = s;
        no_commit.predicate.source.as_mut().unwrap().commit = None;
        assert!(repo_file_request(&no_commit, "x").is_none());
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
        let host = s.predicate.artifacts[0].clone();
        let sources = completion_sources(&s, root, "zsh", Some(&host));
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
        let fish = completion_sources(&s, root, "fish", Some(&host));
        assert!(
            matches!(fish.first(), Some(CompletionSource::Spec { .. })),
            "the fish file is not in the archive, so the spec comes first: {fish:?}"
        );
        assert!(
            completion_sources(&s, root, "nu", Some(&host))
                .iter()
                .all(|c| !matches!(c, CompletionSource::File(_) | CompletionSource::Exec(_)))
        );
    }

    #[test]
    fn scoped_resources_follow_the_selected_artifact() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        for f in ["_t.linux", "_t.any", "_t.mac"] {
            std::fs::write(root.join(f), "").unwrap();
        }
        let s = statement_with(
            r#"[
            {"kind":"completion","shell":"zsh","archive":"_t.any"},
            {"kind":"completion","shell":"zsh","os":"linux","archive":"_t.linux"},
            {"kind":"completion","shell":"zsh","os":"darwin","archive":"_t.mac"},
            {"kind":"skill","name":"t","asset":"t-skill.tar.gz"}
        ]"#,
        );
        let linux = s.predicate.artifacts[0].clone();
        assert_eq!(
            completion_sources(&s, root, "zsh", Some(&linux)),
            vec![CompletionSource::File(root.join("_t.linux"))],
            "the most specific applicable entry wins"
        );
        let mut mac = linux.clone();
        mac.os = Some("darwin".into());
        mac.libc = None;
        assert_eq!(
            completion_sources(&s, root, "zsh", Some(&mac)),
            vec![CompletionSource::File(root.join("_t.mac"))]
        );
        let mut windows = mac.clone();
        windows.os = Some("windows".into());
        assert_eq!(
            completion_sources(&s, root, "zsh", Some(&windows)),
            vec![CompletionSource::File(root.join("_t.any"))],
            "nothing scoped fits, so the unscoped entry applies"
        );
        assert_eq!(
            completion_sources(&s, root, "zsh", None),
            vec![CompletionSource::File(root.join("_t.any"))],
            "with no artifact selected only unscoped entries apply"
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
            zsh.contains("compstate[nmatches]"),
            "a script that completes on its own is not called again: {zsh}"
        );
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
        assert!(
            bash.contains("__mise_complete_cargo_nextest_restub"),
            "the stub comes back at the next prompt: {bash}"
        );
        assert!(stub("rg", Shell::Nu).is_err());
    }
}
