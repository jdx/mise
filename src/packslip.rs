//! What a tool installed from a packslip declares beyond its executables.
//!
//! The backend keeps the verified statement beside each install. This
//! module reads it back and turns the `resources` it lists into things
//! mise can hand a shell: a completion script for whichever version of the
//! tool is active, from the most verifiable source the vendor offered.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use eyre::{Result, WrapErr, bail, eyre};
use packslip::model::{Artifact, Resource, ResourceSource, Statement, resource_fits};
use reqwest::header::{HeaderMap, HeaderValue};

use crate::backend::packslip::{
    STATEMENT_FILE, is_safe_relative, locate_dir_in_install, locate_in_install,
    selected_artifact,
};
use crate::backend::{Backend, MISE_BINS_DIR};
use crate::cmd::CmdLineRunner;
use crate::config::{Config, Settings};
use crate::file;
use crate::github;
use crate::http::{HTTP, HTTP_FETCH};
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

/// The name of a skill, if it is a plain file name and not the file
/// `sync_skills` keeps its own state in.
fn skill_name(resource: &Resource) -> Option<&str> {
    resource
        .name
        .as_deref()
        .filter(|name| file::is_plain_file_name(name) && *name != SYNC_STATE)
}

/// Where a directory resource, a skill, is inside the install, if it is
/// there: in the unpacked artifact, or where [`fetch_files`] put it.
pub(crate) fn resource_dir(install_path: &Path, resource: &Resource) -> Option<PathBuf> {
    let fetched = |sub: &str, rel: &str| {
        Some(install_path.join(RESOURCES_DIR).join(sub).join(rel)).filter(|p| p.is_dir())
    };
    match resource.source()? {
        ResourceSource::Archive => {
            locate_dir_in_install(install_path, resource.archive.as_deref()?)
        }
        ResourceSource::Asset | ResourceSource::Exec => fetched("skills", skill_name(resource)?),
        ResourceSource::Repo => fetched("repo", repo_path(resource)?),
    }
}

/// The `owner/repo` of a release built from a github.com repository.
fn github_repo(statement: &Statement) -> Option<String> {
    let repo = statement.predicate.source.as_ref()?.repo.as_str();
    let path = repo
        .trim_end_matches('/')
        .trim_end_matches(".git")
        .strip_prefix("https://github.com/")?;
    (path.matches('/').count() == 1).then(|| path.to_string())
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
    artifact: Option<&Artifact>,
    pr: &dyn SingleReport,
) -> Result<()> {
    let base = tv.install_path().join(RESOURCES_DIR);
    for resource in &statement.predicate.resources {
        // An entry scoped to another platform is not for this install.
        if let Some(artifact) = artifact
            && !resource_fits(resource, artifact)
        {
            continue;
        }
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
                // The tool is installed by now and the asset is an extra: one
                // that cannot be fetched is reported, not fatal. One that
                // arrives with the wrong digest is another matter.
                if let Err(err) = HTTP
                    .download_file_with_headers(url, &dest, &headers_for(url)?, Some(pr))
                    .await
                {
                    let _ = file::remove_all(&dest);
                    warn!("{}: could not fetch {name}: {err}", tv.style());
                    continue;
                }
                let (actual, _) = packslip::digest_file(&dest)?;
                let expected = statement.digest_of(name);
                if expected != Some(actual.as_str()) {
                    let _ = file::remove_all(&dest);
                    bail!(
                        "{name}: sha256 is {actual}, the packslip says {}",
                        expected.unwrap_or("it is not a subject")
                    );
                }
                if resource.kind == "skill"
                    && let Some(skill) = &resource.name
                {
                    let dir = base.join("skills").join(skill);
                    if !dir.is_dir() {
                        unpack_skill(&dest, &dir, pr)?;
                    }
                }
            }
            Some(ResourceSource::Repo) if resource.kind == "skill" => {
                let commit = statement
                    .predicate
                    .source
                    .as_ref()
                    .and_then(|s| s.commit.as_deref());
                let (Some(rel), Some(commit)) = (repo_path(resource), commit) else {
                    warn!(
                        "{}: skill {:?} in the source repository is not pinned by a commit, or its path is not safe to fetch",
                        tv.style(),
                        resource.repo.as_deref().unwrap_or_default()
                    );
                    continue;
                };
                let dest = base.join("repo").join(rel);
                if dest.exists() {
                    continue;
                }
                let Some(repo) = github_repo(statement) else {
                    warn!(
                        "{}: skill {rel} lives in the source repository, which mise can only read on github.com",
                        tv.style()
                    );
                    continue;
                };
                if let Err(err) = fetch_repo_dir(&repo, commit, rel, &dest, pr).await {
                    let _ = file::remove_all(&dest);
                    warn!(
                        "{}: could not fetch skill {rel} from the source repository: {err}",
                        tv.style()
                    );
                }
            }
            Some(ResourceSource::Exec) if resource.kind == "skill" => {
                let Some(skill) = skill_name(resource) else {
                    continue;
                };
                let dir = base.join("skills").join(skill);
                if dir.join("SKILL.md").is_file() {
                    continue;
                }
                if !Settings::get().packslip.exec {
                    debug!(
                        "{}: skill {skill} is generated by running the tool; packslip.exec is off",
                        tv.style()
                    );
                    continue;
                }
                let Some((program, args)) = resource.exec.split_first() else {
                    continue;
                };
                let Some(path) = installed_bin(&tv.install_path(), program) else {
                    warn!(
                        "{}: skill {skill} is generated by {program}, which the install does not hold",
                        tv.style()
                    );
                    continue;
                };
                pr.set_message(format!("generate skill {skill}"));
                match CmdLineRunner::new(path).args(args).read().await {
                    Ok(text) => {
                        file::create_dir_all(&dir)?;
                        file::write(dir.join("SKILL.md"), text)?;
                    }
                    Err(err) => warn!("{}: could not generate skill {skill}: {err}", tv.style()),
                }
            }
            Some(ResourceSource::Repo) => {
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

/// An executable of the install, by the name the packslip gave it.
fn installed_bin(install_path: &Path, program: &str) -> Option<PathBuf> {
    let linked = install_path.join(MISE_BINS_DIR).join(program);
    if linked.exists() {
        return Some(linked);
    }
    locate_in_install(install_path, program)
}

/// Unpack a skill shipped as its own archive, dropping a lone top-level
/// directory the way artifacts are unpacked.
fn unpack_skill(archive: &Path, dir: &Path, pr: &dyn SingleReport) -> Result<()> {
    let name = archive.file_name().unwrap_or_default().to_string_lossy();
    let format = file::ExtractionFormat::from_file_name(&name);
    if !format.is_archive() {
        bail!("skill asset {name} is not an archive mise can unpack");
    }
    let strip_components = usize::from(file::should_strip_components(archive, format)?);
    file::create_dir_all(dir)?;
    file::extract_archive(
        archive,
        dir,
        format,
        &file::ExtractOptions {
            strip_components,
            pr: Some(pr),
            ..Default::default()
        },
    )
}

/// Fetch a directory of the source repository at `commit` into `dest`,
/// through the GitHub contents API. Entries that are not plain files or
/// directories (symlinks, submodules) are left out.
async fn fetch_repo_dir(
    repo: &str,
    commit: &str,
    rel: &str,
    dest: &Path,
    pr: &dyn SingleReport,
) -> Result<()> {
    let url = format!("https://api.github.com/repos/{repo}/contents/{rel}?ref={commit}");
    let listing: serde_json::Value = HTTP_FETCH
        .json_with_headers(&url, &github::get_headers(&url)?)
        .await?;
    let Some(entries) = listing.as_array() else {
        bail!("{rel} is not a directory of the repository");
    };
    file::create_dir_all(dest)?;
    for entry in entries {
        let Some(name) = entry["name"].as_str() else {
            continue;
        };
        if name.contains('/') || name == ".." || name == "." {
            continue;
        }
        match entry["type"].as_str() {
            Some("dir") => {
                Box::pin(fetch_repo_dir(
                    repo,
                    commit,
                    &format!("{rel}/{name}"),
                    &dest.join(name),
                    pr,
                ))
                .await?;
            }
            Some("file") => {
                let Some(download) = entry["download_url"].as_str() else {
                    continue;
                };
                pr.set_message(format!("download {rel}/{name}"));
                HTTP.download_file_with_headers(
                    download,
                    &dest.join(name),
                    &headers_for(download)?,
                    Some(pr),
                )
                .await?;
            }
            _ => {}
        }
    }
    Ok(())
}

/// A skill one of the active tools declares: a directory holding
/// `SKILL.md`, for the exact version that is active here.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub(crate) struct Skill {
    pub name: String,
    pub tool: String,
    pub version: String,
    pub path: PathBuf,
}

/// Where `sync_skills` records which links in a directory it made, so
/// only those are ever replaced or pruned. A link's target alone would not
/// tell a link mise made from one a person pointed into mise's installs.
pub(crate) const SYNC_STATE: &str = ".mise-skills.json";

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
struct SyncState {
    #[serde(default)]
    links: std::collections::BTreeSet<String>,
}

fn read_sync_state(dir: &Path) -> SyncState {
    let path = dir.join(SYNC_STATE);
    match file::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
        Err(_) => SyncState::default(),
    }
}

fn write_sync_state(dir: &Path, state: &SyncState) -> Result<()> {
    let path = dir.join(SYNC_STATE);
    if state.links.is_empty() {
        if path.exists() {
            file::remove_file(&path)?;
        }
        return Ok(());
    }
    file::write(&path, serde_json::to_string_pretty(state)?)
}

/// The skills a statement declares that are present in the install.
pub(crate) fn skills_of(
    statement: &Statement,
    install_path: &Path,
    tool: &str,
    version: &str,
) -> Vec<Skill> {
    statement
        .predicate
        .resources
        .iter()
        .filter(|r| r.kind == "skill")
        .filter_map(|r| {
            Some(Skill {
                name: r.name.clone()?,
                tool: tool.to_string(),
                version: version.to_string(),
                path: resource_dir(install_path, r)?,
            })
        })
        .collect()
}

/// The skills of every tool active in the current directory.
pub(crate) async fn active_skills(config: &Arc<Config>) -> Result<Vec<Skill>> {
    let ts = config.get_toolset().await?;
    let mut skills = Vec::new();
    for (backend, tv) in ts.list_current_installed_versions(config) {
        let install_path = tv.install_path();
        let statement = match statement(&install_path) {
            Ok(Some(statement)) => statement,
            Ok(None) => continue,
            Err(err) => {
                warn!("{}: {err}", tv.style());
                continue;
            }
        };
        skills.extend(skills_of(
            &statement,
            &install_path,
            &backend.ba().short,
            &tv.version,
        ));
    }
    Ok(skills)
}

/// What [`sync_skills`] did.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct SyncReport {
    pub linked: Vec<String>,
    pub unchanged: Vec<String>,
    pub pruned: Vec<String>,
    /// Skills not linked, with why.
    pub skipped: Vec<(String, String)>,
}

/// Link each skill into `dir` under its name. Only links mise made, which
/// it records in [`SYNC_STATE`] beside them and which point into
/// `installs`, are ever replaced or, with `prune`, removed; anything else
/// at a skill's name is left alone.
pub(crate) fn sync_skills(
    dir: &Path,
    skills: &[Skill],
    installs: &Path,
    prune: bool,
) -> Result<SyncReport> {
    let mut report = SyncReport::default();
    let mut state = read_sync_state(dir);
    let before = state.links.clone();
    let mut wanted: BTreeMap<&str, &Skill> = BTreeMap::new();
    for skill in skills {
        match wanted.get(skill.name.as_str()) {
            Some(first) => report.skipped.push((
                skill.name.clone(),
                format!(
                    "{} also provides a skill called {}; keeping that one",
                    first.tool, skill.name
                ),
            )),
            None => {
                wanted.insert(&skill.name, skill);
            }
        }
    }
    let ours = |name: &str, link: &Path| {
        state.links.contains(name)
            && file::is_symlink_or_junction(link)
            && file::is_symlink_target_within(link, installs).unwrap_or(false)
    };
    if !wanted.is_empty() {
        file::create_dir_all(dir)?;
    }
    for (name, skill) in &wanted {
        let link = dir.join(name);
        if file::is_symlink_to(&link, &skill.path) {
            report.unchanged.push(name.to_string());
            continue;
        }
        if link.exists() || link.is_symlink() {
            if !ours(name, &link) {
                report.skipped.push((
                    name.to_string(),
                    format!("{} exists and is not a link mise made", link.display()),
                ));
                continue;
            }
            file::remove_all(&link)?;
        }
        file::make_symlink(&skill.path, &link)?;
        report.linked.push(name.to_string());
    }
    let mut made: std::collections::BTreeSet<String> = report
        .linked
        .iter()
        .chain(&report.unchanged)
        .cloned()
        .collect();
    if prune && dir.is_dir() {
        for entry in file::ls(dir)? {
            let Some(name) = entry.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if !wanted.contains_key(name) && ours(name, &entry) {
                file::remove_all(&entry)?;
                report.pruned.push(name.to_string());
            }
        }
    } else {
        // Without pruning, links made earlier stay mise's as long as they exist.
        made.extend(
            state
                .links
                .iter()
                .filter(|name| dir.join(name).is_symlink())
                .cloned(),
        );
    }
    state.links = made;
    if state.links != before {
        write_sync_state(dir, &state)?;
    }
    Ok(report)
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
/// Whether the statement offers this shell a completion at all, however the
/// install turned out. A declared file that never reached the install drops
/// out of [`completion_sources`], and the two cases want different answers:
/// one is the vendor declaring nothing, the other is a fetch that failed or
/// was skipped, and reporting the second as the first hides it.
pub(crate) fn declares_completion(statement: &Statement, shell: &str) -> bool {
    statement.predicate.resources.iter().any(|r| {
        r.kind == "cli-spec"
            || (r.kind == "completion"
                && (r.shell.as_deref() == Some(shell) || r.shells.iter().any(|s| s == shell)))
    })
}

pub(crate) fn completion_sources(
    statement: &Statement,
    install_path: &Path,
    shell: &str,
    artifact: Option<&Artifact>,
    tool: Option<&str>,
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
    // A release with several executables carries a spec for each; the one
    // for the executable being completed is the only one that completes
    // it. When none names it (the tool was asked for by id), every spec
    // stays a candidate.
    fn plain(name: &str) -> &str {
        name.strip_suffix(".exe").unwrap_or(name)
    }
    let tool = tool.map(plain);
    let describes = |r: &Resource| r.bin.as_deref().map(plain).is_some_and(|b| Some(b) == tool);
    let spec_entries: Vec<&Resource> = if spec_entries.iter().any(|r| describes(r)) {
        spec_entries.into_iter().filter(|r| describes(r)).collect()
    } else {
        spec_entries
    };
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
    let sources = completion_sources(
        &statement,
        &install_path,
        shell,
        artifact.as_ref(),
        Some(tool),
    );
    if sources.is_empty() {
        if declares_completion(&statement, shell) {
            bail!(
                "the packslip of {} declares a {shell} completion, but none of the files it names are in the install: the resource fetch failed or was skipped",
                tv.style()
            );
        }
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
    let ident = tool.replace(|c: char| !c.is_ascii_alphanumeric(), "_");
    let loader = format!("__mise_load_{ident}");
    let stub = match shell {
        Shell::Zsh => format!(
            r#"#compdef {tool}
# {note}.
# {by}
# The vendor's script takes over this function while it completes; the stub
# is put back afterwards, so the next completion asks mise again.
local __mise_stub="${{functions[_{tool}]}}"
local __mise_matches="${{compstate[nmatches]:-0}}"
# Loaded in a function of its own: a `return` in the vendor's script ends
# that function, not this one, so the stub is always put back below.
{loader}() {{
  eval "$(command mise completion zsh --tool '{tool}' 2>/dev/null)"
}}
{loader} "$@"
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
            let func = format!("__mise_complete_{ident}");
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
            "# {note}.\n# {by}\n$__mise_script = @(& mise completion powershell --tool '{tool}' 2>$null) -join \"`n\"\nif ($__mise_script) {{ Invoke-Expression $__mise_script }}\n"
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
            r#"{{"_type":"https://in-toto.io/Statement/v1","subject":[{{"name":"t-linux-x64.tar.xz","digest":{{"sha256":"{a}"}}}},{{"name":"t-skill.tar.gz","digest":{{"sha256":"{b}"}}}}],"predicateType":"https://packslip.dev/release/v1","predicate":{{"project":"github.com/o/r","version":"1.0.0","published_at":"2026-09-01T00:00:00Z","source":{{"repo":"https://github.com/o/r","commit":"{c}"}},"artifacts":[{{"name":"t-linux-x64.tar.xz","os":"linux","arch":"x86_64","libc":"gnu","size":5,"format":"tar.xz","bin":["t","u"]}}],"resources":{resources},"identity":{{"scheme":"sigstore-oidc","key_id":"https://github.com/o/r/.github/workflows/r.yml@refs/tags/v1","issuer":"https://token.actions.githubusercontent.com"}}}}}}"#,
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
    fn a_declared_completion_is_not_an_absent_one() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let s = statement_with(
            r#"[
            {"kind":"completion","bin":"t","shell":"zsh","archive":"_t"},
            {"kind":"skill","name":"t","asset":"t-skill.tar.gz"}
        ]"#,
        );
        let host = s.predicate.artifacts[0].clone();
        assert!(
            completion_sources(&s, root, "zsh", Some(&host), Some("t")).is_empty(),
            "the file it names was never fetched into the install"
        );
        assert!(
            declares_completion(&s, "zsh"),
            "so the failure is the fetch's, and must not be reported as the \
             vendor declaring nothing"
        );
        assert!(!declares_completion(&s, "fish"));
        let none = statement_with(r#"[{"kind":"skill","name":"t","asset":"t-skill.tar.gz"}]"#);
        assert!(!declares_completion(&none, "zsh"));
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
        let sources = completion_sources(&s, root, "zsh", Some(&host), None);
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
        let fish = completion_sources(&s, root, "fish", Some(&host), None);
        assert!(
            matches!(fish.first(), Some(CompletionSource::Spec { .. })),
            "the fish file is not in the archive, so the spec comes first: {fish:?}"
        );
        assert!(
            completion_sources(&s, root, "nu", Some(&host), None)
                .iter()
                .all(|c| !matches!(c, CompletionSource::File(_) | CompletionSource::Exec(_)))
        );
    }

    #[test]
    fn the_spec_for_the_completed_executable_wins() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("a.kdl"), "").unwrap();
        std::fs::write(root.join("b.kdl"), "").unwrap();
        let s = statement_with(
            r#"[
            {"kind":"cli-spec","format":"usage","bin":"t","archive":"a.kdl"},
            {"kind":"cli-spec","format":"usage","bin":"u","archive":"b.kdl"},
            {"kind":"skill","name":"t","asset":"t-skill.tar.gz"}
        ]"#,
        );
        let host = s.predicate.artifacts[0].clone();
        let bins = |tool: Option<&str>| -> Vec<String> {
            completion_sources(&s, root, "zsh", Some(&host), tool)
                .into_iter()
                .filter_map(|c| match c {
                    CompletionSource::Spec { bin, .. } => Some(bin),
                    _ => None,
                })
                .collect()
        };
        assert_eq!(bins(Some("u")), vec!["u"]);
        assert_eq!(
            bins(Some("u.exe")),
            vec!["u"],
            "the name as a Windows stub embeds it"
        );
        assert_eq!(bins(None), vec!["t", "u"]);
        assert_eq!(
            bins(Some("github.com/o/r")),
            vec!["t", "u"],
            "asked for by id"
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
            completion_sources(&s, root, "zsh", Some(&linux), None),
            vec![CompletionSource::File(root.join("_t.linux"))],
            "the most specific applicable entry wins"
        );
        let mut mac = linux.clone();
        mac.os = Some("darwin".into());
        mac.libc = None;
        assert_eq!(
            completion_sources(&s, root, "zsh", Some(&mac), None),
            vec![CompletionSource::File(root.join("_t.mac"))]
        );
        let mut windows = mac.clone();
        windows.os = Some("windows".into());
        assert_eq!(
            completion_sources(&s, root, "zsh", Some(&windows), None),
            vec![CompletionSource::File(root.join("_t.any"))],
            "nothing scoped fits, so the unscoped entry applies"
        );
        assert_eq!(
            completion_sources(&s, root, "zsh", None, None),
            vec![CompletionSource::File(root.join("_t.any"))],
            "with no artifact selected only unscoped entries apply"
        );
    }

    #[test]
    fn skills_are_found_where_the_install_holds_them() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("share/skills/t")).unwrap();
        std::fs::create_dir_all(root.join(RESOURCES_DIR).join("skills/packed")).unwrap();
        std::fs::create_dir_all(root.join(RESOURCES_DIR).join("repo/skills/fromrepo")).unwrap();
        let s = statement_with(
            r#"[
            {"kind":"skill","name":"t","archive":"top/share/skills/t"},
            {"kind":"skill","name":"packed","asset":"t-skill.tar.gz"},
            {"kind":"skill","name":"fromrepo","repo":"skills/fromrepo"},
            {"kind":"skill","name":"generated","exec":["t","skill"]},
            {"kind":"skill","name":"missing","archive":"nowhere"}
        ]"#,
        );
        let skills = skills_of(&s, root, "tool", "1");
        assert_eq!(
            skills.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
            ["t", "packed", "fromrepo"],
            "an exec skill not yet generated and a missing directory are absent"
        );
        assert_eq!(
            skills[0].path,
            root.join("share/skills/t"),
            "a stripped top dir"
        );
        assert_eq!(skills[0].tool, "tool");
        assert_eq!(github_repo(&s).as_deref(), Some("o/r"));
    }

    #[test]
    fn sync_links_only_what_mise_made() {
        let dir = tempfile::tempdir().unwrap();
        let installs = dir.path().join("installs");
        let v1 = installs.join("tool/1/skills/t");
        let v2 = installs.join("tool/2/skills/t");
        let other = installs.join("other/1/skills/o");
        for p in [&v1, &v2, &other] {
            std::fs::create_dir_all(p).unwrap();
        }
        let skill = |name: &str, tool: &str, version: &str, path: &Path| Skill {
            name: name.into(),
            tool: tool.into(),
            version: version.into(),
            path: path.to_path_buf(),
        };
        let target = dir.path().join("project/.claude/skills");

        let report =
            sync_skills(&target, &[skill("t", "tool", "1", &v1)], &installs, false).unwrap();
        assert_eq!(report.linked, ["t"]);
        assert!(file::is_symlink_to(&target.join("t"), &v1));

        // Same again: nothing to do. A version switch: the link follows.
        let report =
            sync_skills(&target, &[skill("t", "tool", "1", &v1)], &installs, false).unwrap();
        assert_eq!(report.unchanged, ["t"]);
        let report =
            sync_skills(&target, &[skill("t", "tool", "2", &v2)], &installs, false).unwrap();
        assert_eq!(report.linked, ["t"]);
        assert!(file::is_symlink_to(&target.join("t"), &v2));

        // A real directory, or a link mise did not make, is left alone.
        std::fs::create_dir_all(target.join("mine")).unwrap();
        let elsewhere = dir.path().join("elsewhere");
        std::fs::create_dir_all(&elsewhere).unwrap();
        file::make_symlink(&elsewhere, &target.join("theirs")).unwrap();
        let report = sync_skills(
            &target,
            &[
                skill("mine", "tool", "2", &v2),
                skill("theirs", "tool", "2", &v2),
                skill("o", "other", "1", &other),
                skill("o", "tool", "2", &v2),
            ],
            &installs,
            true,
        )
        .unwrap();
        assert_eq!(report.linked, ["o"]);
        assert_eq!(report.skipped.len(), 3, "{:?}", report.skipped);
        assert!(target.join("mine").is_dir());
        assert!(file::is_symlink_to(&target.join("theirs"), &elsewhere));
        assert_eq!(
            report.pruned,
            ["t"],
            "no longer active, and a link mise made"
        );
        assert!(!target.join("t").is_symlink());
        let state: serde_json::Value =
            serde_json::from_str(&file::read_to_string(target.join(SYNC_STATE)).unwrap()).unwrap();
        assert_eq!(state["links"], serde_json::json!(["o"]));

        // A link a person pointed into mise's installs is not mise's to touch,
        // even though its target says otherwise.
        file::make_symlink(&v1, &target.join("handmade")).unwrap();
        let report = sync_skills(
            &target,
            &[
                skill("handmade", "tool", "2", &v2),
                skill("o", "other", "1", &other),
            ],
            &installs,
            true,
        )
        .unwrap();
        assert!(
            file::is_symlink_to(&target.join("handmade"), &v1),
            "left alone"
        );
        assert_eq!(report.pruned, Vec::<String>::new());
        assert_eq!(report.skipped.len(), 1, "{:?}", report.skipped);

        // Nothing to link creates nothing.
        let empty = dir.path().join("empty");
        let report = sync_skills(&empty, &[], &installs, false).unwrap();
        assert_eq!(report, SyncReport::default());
        assert!(!empty.exists());
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
            zsh.contains("__mise_load_rg() {"),
            "the vendor's script runs in a function of its own: {zsh}"
        );
        let pwsh = stub("rg", Shell::PowerShell).unwrap();
        assert!(pwsh.contains("if ($__mise_script)"), "{pwsh}");
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
